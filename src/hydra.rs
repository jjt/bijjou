// Hydra awareness: recognize the stacks of a jj hydra in the log and mark
// them up. A hydra merges several linear stacks as siblings off one base, so
// `jj log` prints one graph column per stack. Two things follow from that:
//
//   - The log's top stack has no `├─╯` row under it. jj closes a graph column
//     only when the branch to its *left* ends, so every stack but the topmost
//     gets a closing row; the topmost runs straight into its neighbour.
//     `hydra.top-stack-padding` draws the separator jj skipped.
//   - Each stack owns a column, so colouring its nodes per stack makes the
//     columns readable at a glance (`hydra.colors`), and the stack's own
//     bookmark names read as part of that column when they carry the same
//     colour (`hydra.color-bookmarks`).
//
// The bookmark naming comes from `hydra.prefixes`, so a row is classified
// from its `bookmarks` field plus the graph prefix jj already drew: no
// subprocess, no repo lookup. A repo that renamed its hydra bookmarks says
// so in config; a repo with no hydra has no bookmark that matches, so it gets
// no markup at all.
//
// The column is what bounds a stack at the bottom. A marker opens its stack
// and the rows under it in the same column are its content, but a commit
// drawn in another column — an extra head off the base, sitting between the
// bottom stack and `HYB` — is nobody's stack and keeps jj's own colours.

use std::collections::HashMap;

use crate::ansi::{emit_filtered_ansi, is_fg_color_sgr, skip_csi, FG_RESET};
use crate::config::{cfg, HydraColors, HydraPrefixes};
use crate::render::{emit_dim_graph, graph_nodes_to_verticals, node_cell};

pub const BOOKMARKS_FIELD: &str = "bookmarks";

// The bookmark names a hydra uses here, expanded from `hydra.prefixes` once.
struct Topology {
    // `HYS-` — the marker bookmark that opens a stack.
    stack_prefix: String,
    // `HYWC-` — a stack's working copy, which sits outside the stack.
    wc_prefix: String,
    // `HYB` / `HYH` / `HYCR`.
    anchors: Vec<String>,
}

impl Topology {
    fn from_prefixes(p: &HydraPrefixes) -> Topology {
        Topology {
            stack_prefix: p.stack_marker(),
            wc_prefix: p.working_copy(),
            anchors: p.anchors(),
        }
    }
}

// What one row's hydra classification changes about its rendering.
pub struct Markup<'a> {
    // SGR for the row's graph node; `None` leaves the node as jj drew it.
    pub node: Option<&'a [u8]>,
    // The row's `bookmarks` field with every hydra bookmark on it recoloured
    // to its stack; `None` leaves the field as jj printed it.
    pub bookmarks: Option<&'a [u8]>,
}

// Per-row state, walked top-down with the log.
pub struct Walk {
    // `None` with `hydra.enable = false`: every row passes through untouched.
    topo: Option<Topology>,
    // Stack names in the order the log first named them, top first — a
    // stack's slot in `hydra.colors` when that is a palette.
    seen: Vec<String>,
    // SGR for the stack the walk is inside; empty outside any stack.
    color: Vec<u8>,
    // The graph cell that stack's nodes sit in, so a row drawn in another
    // column is recognized as some other branch. `None` outside any stack.
    column: Option<usize>,
    // SGR for a single `HYWC-*` row, which belongs to a stack without being
    // in it: the colour applies to that row and is not carried down.
    wc_color: Vec<u8>,
    stacks_seen: usize,
    // Reused scratch for the de-ANSI'd bookmarks field.
    scratch: Vec<u8>,
    // Reused buffer for the recoloured `bookmarks` field.
    bookmarks: Vec<u8>,
}

impl Walk {
    pub fn start() -> Walk {
        Walk {
            topo: cfg()
                .hydra_enable
                .then(|| Topology::from_prefixes(&cfg().hydra_prefixes)),
            seen: Vec::new(),
            color: Vec::new(),
            column: None,
            wc_color: Vec::new(),
            stacks_seen: 0,
            scratch: Vec::new(),
            bookmarks: Vec::new(),
        }
    }

    // Classify one commit row and return what its rendering takes from the
    // hydra. `prefix` is the row's graph prefix as jj drew it: when this row
    // opens the second stack, the top stack's missing separator is drawn
    // from it into `out` first.
    //
    // Rows must arrive in log order — the walk carries a stack's colour down
    // from its marker through its content commits, which are the rows below
    // it in its own graph column.
    pub fn markup(
        &mut self,
        fields: &HashMap<String, Vec<u8>>,
        prefix: &[u8],
        out: &mut Vec<u8>,
    ) -> Markup<'_> {
        let raw = fields
            .get(BOOKMARKS_FIELD)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        strip_ansi_into(raw, &mut scratch);
        // `None` topology is `hydra.enable = false`: nothing to classify.
        let mark = self.topo.as_ref().map(|topo| mark_of(topo, &scratch));
        self.scratch = scratch;
        let Some(mark) = mark else {
            return Markup {
                node: None,
                bookmarks: None,
            };
        };

        // A working copy sits above the head, outside every stack, so it ends
        // whichever stack the walk was in — but it is that stack's row, so it
        // takes the stack's colour without carrying it down.
        let from_wc = matches!(mark, Mark::WorkingCopy(_));
        match mark {
            Mark::Outside => self.leave(),
            // A stack owns one graph column, from its marker down to its last
            // content commit, so a row whose node sits in another column is
            // another branch entirely — an extra head off the base, say. The
            // stack ends above it rather than lending it a colour.
            Mark::Inside => {
                if !self.color.is_empty() && node_cell(prefix) != self.column {
                    self.leave();
                }
            }
            Mark::Stack(name) => {
                if self.stacks_seen == 1 && cfg().hydra_top_stack_padding {
                    emit_padding(prefix, out);
                }
                self.stacks_seen += 1;
                let index = index_of(&mut self.seen, &name);
                self.color = stack_color(&name, index);
                self.column = node_cell(prefix);
            }
            Mark::WorkingCopy(name) => {
                self.leave();
                let index = index_of(&mut self.seen, &name);
                self.wc_color = stack_color(&name, index);
            }
        }

        let mut buf = std::mem::take(&mut self.bookmarks);
        let recoloured = match self.topo.as_ref() {
            Some(topo) if cfg().hydra_color_bookmarks => {
                color_bookmarks(topo, &mut self.seen, &mut self.scratch, raw, &mut buf)
            }
            _ => false,
        };
        self.bookmarks = buf;

        let node = if from_wc { &self.wc_color } else { &self.color };
        Markup {
            node: (!node.is_empty()).then_some(node.as_slice()),
            bookmarks: recoloured.then_some(self.bookmarks.as_slice()),
        }
    }

    // Out of every stack: the walk is between the head and the markers, or
    // below the last one, and the next marker row starts the state over.
    fn leave(&mut self) {
        self.color.clear();
        self.column = None;
    }
}

// Position in the graph, top first: the log itself is the order, so a stack
// keeps its slot for the whole run once met.
fn index_of(seen: &mut Vec<String>, name: &str) -> usize {
    if let Some(i) = seen.iter().position(|n| n == name) {
        return i;
    }
    seen.push(name.to_string());
    seen.len() - 1
}

// Where a row sits relative to the stacks: opening one, carrying a stack's
// working copy, outside every one, or carrying on in whichever the walk is
// already in (a stack's own content commits name no bookmark at all).
enum Mark {
    Stack(String),
    WorkingCopy(String),
    Outside,
    Inside,
}

fn mark_of(topo: &Topology, bookmarks: &[u8]) -> Mark {
    let mut wc: Option<String> = None;
    let mut outside = false;
    for token in bookmarks.split(u8::is_ascii_whitespace) {
        // jj flags a bookmark out of sync with its remote with a trailing `*`
        // and prints remote refs as `name@remote`; only local names count.
        let token = token.strip_suffix(b"*").unwrap_or(token);
        if token.is_empty() || token.contains(&b'@') {
            continue;
        }
        if let Some(name) = token.strip_prefix(topo.stack_prefix.as_bytes()) {
            if !name.is_empty() {
                return Mark::Stack(String::from_utf8_lossy(name).into_owned());
            }
        }
        if let Some(name) = token.strip_prefix(topo.wc_prefix.as_bytes()) {
            if !name.is_empty() {
                wc = Some(String::from_utf8_lossy(name).into_owned());
                continue;
            }
        }
        if topo.anchors.iter().any(|a| a.as_bytes() == token) {
            outside = true;
        }
    }
    match (wc, outside) {
        (Some(name), _) => Mark::WorkingCopy(name),
        (None, true) => Mark::Outside,
        (None, false) => Mark::Inside,
    }
}

// Rewrite the row's `bookmarks` field into `out` so every hydra bookmark on
// it carries its stack's colour instead of jj's. Bookmarks are whitespace
// separated and each comes wrapped in jj's own SGR, so a name we take over
// has its foreground codes dropped and the stack's put in front; every other
// bookmark on the row is copied byte-for-byte. Returns false when the row
// names no hydra bookmark (or the colours are off), which leaves the caller
// with jj's field untouched.
fn color_bookmarks(
    topo: &Topology,
    seen: &mut Vec<String>,
    scratch: &mut Vec<u8>,
    raw: &[u8],
    out: &mut Vec<u8>,
) -> bool {
    out.clear();
    let mut hit = false;
    let mut i = 0;
    while i < raw.len() {
        // CSI sequences carry no whitespace, so splitting on raw bytes keeps
        // each name together with the colour codes around it.
        let start = i;
        let ws = raw[i].is_ascii_whitespace();
        while i < raw.len() && raw[i].is_ascii_whitespace() == ws {
            i += 1;
        }
        let token = &raw[start..i];
        if ws {
            out.extend_from_slice(token);
            continue;
        }
        scratch.clear();
        strip_ansi_into(token, scratch);
        let color = stack_of(topo, scratch).map(|name| {
            let index = index_of(seen, &name);
            stack_color(&name, index)
        });
        match color {
            Some(sgr) if !sgr.is_empty() => {
                out.extend_from_slice(&sgr);
                emit_filtered_ansi(token, out, is_fg_color_sgr);
                out.extend_from_slice(FG_RESET);
                hit = true;
            }
            _ => out.extend_from_slice(token),
        }
    }
    hit
}

// The stack a single bookmark name belongs to: both `HYS-<name>` and
// `HYWC-<name>` name their stack, and the same flag and remote-ref rules as
// `mark_of` apply.
fn stack_of(topo: &Topology, token: &[u8]) -> Option<String> {
    let token = token.strip_suffix(b"*").unwrap_or(token);
    if token.contains(&b'@') {
        return None;
    }
    [&topo.stack_prefix, &topo.wc_prefix]
        .iter()
        .filter_map(|prefix| token.strip_prefix(prefix.as_bytes()))
        .find(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
}

// The separator row jj skipped under the log's top stack, drawn from the next
// stack's marker row so every column lands where it does above and below.
fn emit_padding(prefix: &[u8], out: &mut Vec<u8>) {
    let verticals = graph_nodes_to_verticals(prefix);
    emit_dim_graph(&verticals, cfg().graph_collapse, None, out);
    out.push(b'\n');
}

fn stack_color(name: &str, index: usize) -> Vec<u8> {
    match &cfg().hydra_colors {
        HydraColors::Off => Vec::new(),
        HydraColors::Hash => hash_color(name),
        HydraColors::Palette(p) if p.is_empty() => Vec::new(),
        HydraColors::Palette(p) => p[index % p.len()].clone(),
    }
}

// Hues the hashed palette does not hand out: the hue of a colour a stack
// must not read as, plus 10° either side. `#a6e3a1` sits at 115° and
// `#f5c2e7` at 316°. Ascending and disjoint, which `hue_of` counts on.
const RESERVED_HUES: [(u32, u32); 2] = [(105, 125), (306, 326)];

// Hues left to hand out, the reserved bands taken off the circle.
const HUE_SPACE: u64 = {
    let mut left = 360;
    let mut i = 0;
    while i < RESERVED_HUES.len() {
        left -= RESERVED_HUES[i].1 - RESERVED_HUES[i].0 + 1;
        i += 1;
    }
    left as u64
};

// The `index`-th hue still on offer, `index` in `0..HUE_SPACE`. Reserved
// bands are skipped rather than clamped, so no hue is handed out twice as
// often as another.
fn hue_of(index: u32) -> u32 {
    let mut hue = index;
    for (lo, hi) in RESERVED_HUES {
        if hue >= lo {
            hue += hi - lo + 1;
        }
    }
    hue
}

// A stack's colour has to be stable across runs and distinct from its
// neighbours'. FNV-1a over the name picks a hue out of `HUE_SPACE`;
// saturation and lightness are fixed so every stack lands in the same
// legible band. Hashing straight into rgb instead would hand out
// near-blacks and near-whites.
fn hash_color(name: &str) -> Vec<u8> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let (r, g, b) = hsl_to_rgb(hue_of((h % HUE_SPACE) as u32), 0.68, 0.62);
    format!("\x1b[38;2;{};{};{}m", r, g, b).into_bytes()
}

fn hsl_to_rgb(hue: u32, sat: f64, light: f64) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * light - 1.0).abs()) * sat;
    let sector = hue / 60;
    let second = chroma * (1.0 - ((hue as f64 / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match sector {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = light - chroma / 2.0;
    let byte = |v: f64| ((v + base) * 255.0).round().clamp(0.0, 255.0) as u8;
    (byte(r), byte(g), byte(b))
}

// Copy `bytes` minus its CSI sequences, so bookmark names split on real
// whitespace rather than on the colour codes jj wraps them in.
fn strip_ansi_into(bytes: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < bytes.len() {
        if let Some(after) = skip_csi(bytes, i) {
            i = after;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topo() -> Topology {
        Topology::from_prefixes(&HydraPrefixes::default())
    }

    #[test]
    fn default_prefixes_expand_to_the_stock_bookmark_names() {
        let t = topo();
        assert_eq!(t.stack_prefix, "HYS-");
        assert_eq!(t.wc_prefix, "HYWC-");
        assert_eq!(t.anchors, vec!["HYB", "HYH", "HYCR"]);
    }

    #[test]
    fn renamed_prefixes_expand_to_the_repo_names() {
        let t = Topology::from_prefixes(&HydraPrefixes {
            prefix: "ZZ".to_string(),
            stack_head: "ST".to_string(),
            ..HydraPrefixes::default()
        });
        assert_eq!(t.stack_prefix, "ZZST-");
        assert_eq!(t.wc_prefix, "ZZWC-");
        assert_eq!(t.anchors, vec!["ZZB", "ZZH", "ZZCR"]);
    }

    fn mark(bookmarks: &str) -> Mark {
        mark_of(&topo(), bookmarks.as_bytes())
    }

    #[test]
    fn stack_marker_opens_its_stack() {
        assert!(matches!(mark("HYS-delta"), Mark::Stack(n) if n == "delta"));
        // jj's out-of-sync flag is not part of the name.
        assert!(matches!(mark("HYS-delta*"), Mark::Stack(n) if n == "delta"));
    }

    #[test]
    fn anchors_are_outside_every_stack() {
        assert!(matches!(mark("HYB main"), Mark::Outside));
        assert!(matches!(mark("HYH"), Mark::Outside));
        assert!(matches!(mark("HYCR"), Mark::Outside));
    }

    #[test]
    fn working_copies_name_their_stack() {
        assert!(matches!(mark("HYWC-delta"), Mark::WorkingCopy(n) if n == "delta"));
        assert!(matches!(mark("HYWC-delta*"), Mark::WorkingCopy(n) if n == "delta"));
    }

    #[test]
    fn content_commits_stay_in_the_stack_above_them() {
        assert!(matches!(mark(""), Mark::Inside));
        assert!(matches!(mark("jjt/delta"), Mark::Inside));
    }

    #[test]
    fn remote_refs_are_not_local_bookmarks() {
        // `commit.bookmarks()` carries remote refs; a stack marker that only
        // exists on a remote must not open a stack locally.
        assert!(matches!(mark("HYS-delta@origin"), Mark::Inside));
        assert!(matches!(mark("HYWC-delta@origin"), Mark::Inside));
    }

    fn walk() -> Walk {
        Walk {
            topo: Some(topo()),
            seen: Vec::new(),
            color: Vec::new(),
            column: None,
            wc_color: Vec::new(),
            stacks_seen: 0,
            scratch: Vec::new(),
            bookmarks: Vec::new(),
        }
    }

    // One commit row with `bookmarks` set, under the default config (hashed
    // colours) and an empty graph prefix, so no padding row is in play.
    fn row(walk: &mut Walk, bookmarks: &str) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let mut fields = HashMap::new();
        fields.insert(BOOKMARKS_FIELD.to_string(), bookmarks.as_bytes().to_vec());
        let mut out = Vec::new();
        let markup = walk.markup(&fields, b"", &mut out);
        (
            markup.node.map(<[u8]>::to_vec),
            markup.bookmarks.map(<[u8]>::to_vec),
        )
    }

    fn node(walk: &mut Walk, bookmarks: &str) -> Option<Vec<u8>> {
        row(walk, bookmarks).0
    }

    // The same, with the row's graph prefix, so the stack's column is in
    // play: `prefix` is jj's own drawing, one node glyph among the edges.
    fn node_at(walk: &mut Walk, prefix: &str, bookmarks: &str) -> Option<Vec<u8>> {
        let mut fields = HashMap::new();
        fields.insert(BOOKMARKS_FIELD.to_string(), bookmarks.as_bytes().to_vec());
        let mut out = Vec::new();
        walk.markup(&fields, prefix.as_bytes(), &mut out)
            .node
            .map(<[u8]>::to_vec)
    }

    #[test]
    fn a_commit_outside_the_stacks_takes_no_stack_colour() {
        let mut walk = walk();
        // The log's bottom stack: marker and content in column 0.
        let marker = node_at(&mut walk, "● │ ", "HYS-alpha").expect("marker is coloured");
        assert_eq!(node_at(&mut walk, "● │ ", ""), Some(marker));
        // An extra head off the base, drawn in its own column once the stack
        // closed above it: nobody's stack, so jj's colours stand — and the
        // stack does not resume below it either.
        assert_eq!(node_at(&mut walk, "│ ● ", ""), None);
        assert_eq!(node_at(&mut walk, "● │ ", ""), None);
    }

    #[test]
    fn a_working_copy_takes_its_stack_colour_without_carrying_it() {
        let mut walk = walk();
        let wc = node(&mut walk, "HYWC-delta").expect("working copy is coloured");
        // The head sits between the working copies and the stacks: uncoloured,
        // and it carries nothing down from the working copy above it.
        assert_eq!(node(&mut walk, "HYH"), None);
        assert_eq!(node(&mut walk, ""), None);
        // The stack itself, and its content commits, match its working copy.
        let marker = node(&mut walk, "HYS-delta").expect("stack marker is coloured");
        assert_eq!(marker, wc);
        assert_eq!(node(&mut walk, ""), Some(marker));
    }

    #[test]
    fn hydra_bookmark_names_take_their_stack_colour() {
        let mut walk = walk();
        let (node, bookmarks) = row(&mut walk, "\x1b[38;5;5mHYS-delta\x1b[39m");
        let sgr = node.expect("stack marker is coloured");
        let bookmarks = bookmarks.expect("its name is recoloured too");
        // jj's own foreground drops out, the stack's colour leads the name.
        let mut want = sgr.clone();
        want.extend_from_slice(b"HYS-delta");
        want.extend_from_slice(FG_RESET);
        assert_eq!(bookmarks, want);
        // The working copy of the same stack reads the same.
        let (_, wc) = row(&mut walk, "HYWC-delta");
        let wc = wc.expect("working copy name is recoloured");
        assert!(wc.starts_with(&sgr), "{:?}", String::from_utf8_lossy(&wc));
    }

    #[test]
    fn bookmarks_outside_the_hydra_keep_jjs_colours() {
        let mut walk = walk();
        // An anchor row, a plain bookmark and a remote-only stack marker are
        // none of them a stack's name, so the field passes through.
        assert_eq!(row(&mut walk, "\x1b[38;5;5mHYB main\x1b[39m").1, None);
        assert_eq!(row(&mut walk, "jjt/delta").1, None);
        assert_eq!(row(&mut walk, "HYS-delta@origin").1, None);
    }

    #[test]
    fn a_hydra_bookmark_is_recoloured_beside_its_neighbours() {
        let mut walk = walk();
        let (node, bookmarks) = row(&mut walk, "main HYS-delta* v1");
        let sgr = node.expect("stack marker is coloured");
        let bookmarks = bookmarks.expect("its name is recoloured");
        let mut want = b"main ".to_vec();
        want.extend_from_slice(&sgr);
        want.extend_from_slice(b"HYS-delta*");
        want.extend_from_slice(FG_RESET);
        want.extend_from_slice(b" v1");
        assert_eq!(bookmarks, want);
    }

    #[test]
    fn palette_index_follows_log_order_then_first_sight() {
        let mut seen = Vec::new();
        assert_eq!(index_of(&mut seen, "delta"), 0);
        assert_eq!(index_of(&mut seen, "gamma"), 1);
        // Later sightings append, stably.
        assert_eq!(index_of(&mut seen, "later"), 2);
        assert_eq!(index_of(&mut seen, "other"), 3);
        assert_eq!(index_of(&mut seen, "later"), 2);
    }

    #[test]
    fn hashed_colors_are_stable_and_per_name() {
        assert_eq!(hash_color("delta"), hash_color("delta"));
        assert_ne!(hash_color("delta"), hash_color("gamma"));
    }

    #[test]
    fn hashed_colors_are_truecolor_sgr() {
        let sgr = hash_color("delta");
        let text = String::from_utf8(sgr).unwrap();
        assert!(text.starts_with("\x1b[38;2;"), "{:?}", text);
        assert!(text.ends_with('m'), "{:?}", text);
    }

    #[test]
    fn hue_space_is_the_circle_less_the_reserved_bands() {
        assert_eq!(HUE_SPACE, 360 - 21 - 21);
    }

    #[test]
    fn every_hue_on_offer_clears_the_reserved_bands() {
        let mut last = None;
        for index in 0..HUE_SPACE as u32 {
            let hue = hue_of(index);
            assert!(hue < 360, "index {} left the circle at {}", index, hue);
            for (lo, hi) in RESERVED_HUES {
                assert!(
                    hue < lo || hue > hi,
                    "index {} landed on reserved hue {}",
                    index,
                    hue
                );
            }
            // Strictly increasing, so no hue is handed out twice.
            assert!(last < Some(hue), "index {} went back to {}", index, hue);
            last = Some(hue);
        }
        // The bands are skipped, not clipped: the circle's top is still in
        // play.
        assert_eq!(hue_of(0), 0);
        assert_eq!(hue_of(HUE_SPACE as u32 - 1), 359);
        // Either side of each band.
        assert_eq!(hue_of(104), 104);
        assert_eq!(hue_of(105), 126);
    }

    #[test]
    fn hashed_colors_avoid_the_reserved_hues() {
        // The reserved bands are on hue, so the check is on hue: rebuild it
        // from the rgb the hash actually emitted.
        // `report` hashed into the pink band before the bands existed.
        for name in [
            "alpha", "beta", "gamma", "delta", "report", "retry", "green", "pink",
        ] {
            let hue = hue_of_sgr(&hash_color(name));
            for (lo, hi) in RESERVED_HUES {
                assert!(
                    hue + 1.0 < lo as f64 || hue > hi as f64 + 1.0,
                    "{} hashed into reserved hue {}",
                    name,
                    hue
                );
            }
        }
    }

    // The hue of a `\x1b[38;2;r;g;bm` SGR, back out of the rgb.
    fn hue_of_sgr(sgr: &[u8]) -> f64 {
        let text = std::str::from_utf8(sgr).unwrap();
        let body = text
            .strip_prefix("\x1b[38;2;")
            .and_then(|t| t.strip_suffix('m'))
            .expect("truecolor sgr");
        let rgb: Vec<f64> = body
            .split(';')
            .map(|v| v.parse::<f64>().unwrap() / 255.0)
            .collect();
        let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        assert!(delta > 0.0, "grey has no hue: {:?}", rgb);
        let hue = if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        (hue + 360.0) % 360.0
    }

    #[test]
    fn strip_ansi_leaves_the_bookmark_names() {
        let mut out = Vec::new();
        strip_ansi_into(b"\x1b[38;5;5mHYS-delta\x1b[39m", &mut out);
        assert_eq!(out, b"HYS-delta");
    }
}
