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
use std::sync::LazyLock;

use crate::ansi::{is_fg_color_sgr, sgr_params, skip_csi, FG_RESET};
use crate::config::{cfg, HydraColors, HydraPrefixReplace, HydraPrefixes};
use crate::render::{emit_dim_graph, graph_nodes_to_verticals, node_cell};

pub const BOOKMARKS_FIELD: &str = "bookmarks";

// The bookmark names a hydra uses here, expanded from `hydra.prefixes` once,
// each paired with what it reads as under `hydra.prefixes-replace`.
#[derive(Clone)]
struct Topology {
    // `HYS-` — the marker bookmark that opens a stack.
    stack_prefix: String,
    // `HYWC-` — a stack's working copy, which sits outside the stack.
    wc_prefix: String,
    // `HYB` / `HYH` / `HYCR`.
    anchors: Vec<String>,
    // What each of those leaders renders as, `None` where the matching
    // `hydra.prefixes-replace` key is unset — that name passes through as jj
    // printed it. `anchor_replace` is index-aligned with `anchors`.
    stack_replace: Option<Vec<u8>>,
    wc_replace: Option<Vec<u8>>,
    anchor_replace: Vec<Option<Vec<u8>>>,
}

impl Topology {
    fn from_prefixes(p: &HydraPrefixes, r: &HydraPrefixReplace) -> Topology {
        Topology {
            stack_prefix: p.stack_marker(),
            wc_prefix: p.working_copy(),
            anchors: p.anchors(),
            stack_replace: p.stack_marker_replace(r).map(String::into_bytes),
            wc_replace: p.working_copy_replace(r).map(String::into_bytes),
            anchor_replace: p
                .anchors_replace(r)
                .into_iter()
                .map(|v| v.map(String::into_bytes))
                .collect(),
        }
    }

    // Nothing to stand in for any name: the rewrite only has colours to do.
    fn replaces(&self) -> bool {
        self.stack_replace.is_some()
            || self.wc_replace.is_some()
            || self.anchor_replace.iter().any(Option::is_some)
    }

    // What one bookmark token renders as: the stand-in bytes plus how many
    // bytes of the name they replace. `None` leaves the token alone — no key
    // for its leader, or no hydra bookmark at all.
    fn replacement_of(&self, token: &[u8]) -> Option<(&[u8], usize)> {
        // jj's out-of-sync `*` is not part of the name, and a remote ref is
        // not a local hydra bookmark — the same rules `mark_of` applies.
        let name = token.strip_suffix(b"*").unwrap_or(token);
        if name.contains(&b'@') {
            return None;
        }
        for (prefix, replace) in [
            (&self.stack_prefix, &self.stack_replace),
            (&self.wc_prefix, &self.wc_replace),
        ] {
            if let Some(rest) = name.strip_prefix(prefix.as_bytes()) {
                if !rest.is_empty() {
                    return replace.as_deref().map(|r| (r, prefix.len()));
                }
            }
        }
        self.anchors
            .iter()
            .zip(&self.anchor_replace)
            .find(|(anchor, _)| anchor.as_bytes() == name)
            .and_then(|(anchor, replace)| replace.as_deref().map(|r| (r, anchor.len())))
    }
}

// The topology in force for this run, built once: `hydra.prefixes` and
// `hydra.prefixes-replace` are config, so every pass reads the same names.
// `None` is `hydra.enable = false` — nothing is classified or renamed.
static TOPOLOGY: LazyLock<Option<Topology>> = LazyLock::new(|| {
    let c = cfg();
    c.hydra_enable
        .then(|| Topology::from_prefixes(&c.hydra_prefixes, &c.hydra_prefixes_replace))
});

// Pass 1 needs the row's `bookmarks` field at its rendered width, and
// `hydra.prefixes-replace` changes that width, so the same substitution runs
// there — without the colours, which cost no width. Returns false when the
// row has no name to stand in for, leaving the caller with jj's field.
pub fn replace_names(
    fields: &HashMap<String, Vec<u8>>,
    scratch: &mut Vec<u8>,
    out: &mut Vec<u8>,
) -> bool {
    let Some(topo) = TOPOLOGY.as_ref().filter(|topo| topo.replaces()) else {
        return false;
    };
    let raw = fields
        .get(BOOKMARKS_FIELD)
        .map(Vec::as_slice)
        .unwrap_or_default();
    rewrite_bookmarks(topo, None, scratch, raw, out)
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
    // Reused buffer for the rewritten `bookmarks` field.
    bookmarks: Vec<u8>,
}

impl Walk {
    pub fn start() -> Walk {
        Walk {
            topo: TOPOLOGY.as_ref().cloned(),
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
        let rewritten = match self.topo.as_ref() {
            Some(topo) if cfg().hydra_color_bookmarks => {
                rewrite_bookmarks(topo, Some(&mut self.seen), &mut self.scratch, raw, &mut buf)
            }
            // Colours off, names still replaced: `hydra.prefixes-replace` is
            // independent of `hydra.color-bookmarks`.
            Some(topo) if topo.replaces() => {
                rewrite_bookmarks(topo, None, &mut self.scratch, raw, &mut buf)
            }
            _ => false,
        };
        self.bookmarks = buf;

        let node = if from_wc { &self.wc_color } else { &self.color };
        Markup {
            node: (!node.is_empty()).then_some(node.as_slice()),
            bookmarks: rewritten.then_some(self.bookmarks.as_slice()),
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

// Rewrite the row's `bookmarks` field into `out`: every hydra bookmark on it
// carries its stack's colour instead of jj's, and reads under the stand-in
// `hydra.prefixes-replace` gives its leader. Bookmarks are whitespace
// separated and each comes wrapped in jj's own SGR, so a name we take over
// has its foreground codes dropped and the stack's put in front; every other
// bookmark on the row is copied byte-for-byte. `seen` carries the palette
// order and is `None` when only the names are being replaced — pass 1, or
// `hydra.color-bookmarks = false`. Returns false when the row has no name to
// take over, which leaves the caller with jj's field untouched.
fn rewrite_bookmarks(
    topo: &Topology,
    mut seen: Option<&mut Vec<String>>,
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
        let color = seen.as_deref_mut().and_then(|seen| {
            stack_of(topo, scratch).map(|name| {
                let index = index_of(seen, &name);
                stack_color(&name, index)
            })
        });
        let replace = topo.replacement_of(scratch);
        match color {
            Some(sgr) if !sgr.is_empty() => {
                out.extend_from_slice(&sgr);
                emit_token(token, true, replace, out);
                out.extend_from_slice(FG_RESET);
                hit = true;
            }
            // Nothing to recolour, but the name still reads as its stand-in,
            // in whichever colour jj gave it.
            _ if replace.is_some() => {
                emit_token(token, false, replace, out);
                hit = true;
            }
            _ => out.extend_from_slice(token),
        }
    }
    hit
}

// Copy one bookmark token into `out`. `drop_fg` drops jj's foreground SGRs,
// leaving the caller's colour in force. `replace` is the stand-in for the
// token's leader plus the byte count it stands in for, counted over the
// name's own bytes — the CSI sequences jj wrapped it in are copied either
// way, so the colour around the name survives the substitution.
fn emit_token(token: &[u8], drop_fg: bool, replace: Option<(&[u8], usize)>, out: &mut Vec<u8>) {
    let (stand_in, mut drop_left) = replace.unwrap_or((&[], 0));
    let mut pending = replace.is_some();
    let mut i = 0;
    while i < token.len() {
        if let Some(end) = skip_csi(token, i) {
            let fg = sgr_params(&token[i..end]).is_some_and(is_fg_color_sgr);
            if !(drop_fg && fg) {
                out.extend_from_slice(&token[i..end]);
            }
            i = end;
            continue;
        }
        if pending {
            out.extend_from_slice(stand_in);
            pending = false;
        }
        if drop_left > 0 {
            drop_left -= 1;
            i += 1;
            continue;
        }
        out.push(token[i]);
        i += 1;
    }
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
    let hue = hue_of((h % HUE_SPACE) as u32);
    let (r, g, b) = hsl_to_rgb(hue, 0.68, stack_lightness(hue));
    format!("\x1b[38;2;{};{};{}m", r, g, b).into_bytes()
}

// Blues around 241° read dark against the terminal. Lift the lightness on a
// linear ramp inside a 20° band either side of 241°: 0.62 at the band edges,
// up to 0.70 at 241° itself. Outside the band the fixed 0.62 stands.
fn stack_lightness(hue: u32) -> f64 {
    const PEAK_HUE: f64 = 241.0;
    const BAND: f64 = 20.0;
    const BASE: f64 = 0.62;
    const PEAK: f64 = 0.70;
    let distance = (hue as f64 - PEAK_HUE).abs();
    if distance >= BAND {
        BASE
    } else {
        BASE + (PEAK - BASE) * (BAND - distance) / BAND
    }
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
        Topology::from_prefixes(&HydraPrefixes::default(), &HydraPrefixReplace::default())
    }

    // The same naming with `hydra.prefixes-replace` in force.
    fn topo_with(r: HydraPrefixReplace) -> Topology {
        Topology::from_prefixes(&HydraPrefixes::default(), &r)
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
        let t = Topology::from_prefixes(
            &HydraPrefixes {
                prefix: "ZZ".to_string(),
                stack_head: "ST".to_string(),
                ..HydraPrefixes::default()
            },
            &HydraPrefixReplace::default(),
        );
        assert_eq!(t.stack_prefix, "ZZST-");
        assert_eq!(t.wc_prefix, "ZZWC-");
        assert_eq!(t.anchors, vec!["ZZB", "ZZH", "ZZCR"]);
    }

    // The row's `bookmarks` field as it renders, names replaced and nothing
    // recoloured; `None` is "no name to take over", which leaves the caller
    // with jj's own field.
    fn rewritten(topo: &Topology, bookmarks: &str) -> Option<String> {
        let mut scratch = Vec::new();
        let mut out = Vec::new();
        let hit = rewrite_bookmarks(topo, None, &mut scratch, bookmarks.as_bytes(), &mut out);
        hit.then(|| String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_per_bookmark_key_stands_in_for_the_whole_leader() {
        let t = topo_with(HydraPrefixReplace {
            base: Some("◆".to_string()),
            stack_head: Some("Ψ".to_string()),
            stack_working_copy: Some("ψ".to_string()),
            ..HydraPrefixReplace::default()
        });
        // The dash belongs to the leader, so it goes with it.
        assert_eq!(rewritten(&t, "HYS-foo").as_deref(), Some("Ψfoo"));
        assert_eq!(rewritten(&t, "HYWC-foo").as_deref(), Some("ψfoo"));
        // An anchor is a whole name, so the stand-in is the whole name.
        assert_eq!(rewritten(&t, "HYB main").as_deref(), Some("◆ main"));
        // jj's out-of-sync flag is not part of the name and rides along.
        assert_eq!(rewritten(&t, "HYS-foo*").as_deref(), Some("Ψfoo*"));
        // A key left unset leaves the bookmarks it names alone.
        assert_eq!(rewritten(&t, "HYH"), None);
        assert_eq!(rewritten(&t, "HYCR"), None);
    }

    #[test]
    fn prefix_alone_stands_in_for_the_shared_leader() {
        let t = topo_with(HydraPrefixReplace {
            prefix: Some("⋔".to_string()),
            ..HydraPrefixReplace::default()
        });
        assert_eq!(rewritten(&t, "HYS-foo").as_deref(), Some("⋔S-foo"));
        assert_eq!(rewritten(&t, "HYWC-foo").as_deref(), Some("⋔WC-foo"));
        assert_eq!(rewritten(&t, "HYB").as_deref(), Some("⋔B"));
        assert_eq!(rewritten(&t, "HYH").as_deref(), Some("⋔H"));
        assert_eq!(rewritten(&t, "HYCR").as_deref(), Some("⋔CR"));
    }

    #[test]
    fn a_per_bookmark_key_wins_over_prefix() {
        let t = topo_with(HydraPrefixReplace {
            prefix: Some("⋔".to_string()),
            stack_head: Some("Ψ".to_string()),
            ..HydraPrefixReplace::default()
        });
        assert_eq!(rewritten(&t, "HYS-foo").as_deref(), Some("Ψfoo"));
        assert_eq!(rewritten(&t, "HYWC-foo").as_deref(), Some("⋔WC-foo"));
    }

    #[test]
    fn only_this_repos_hydra_bookmarks_are_replaced() {
        let t = topo_with(HydraPrefixReplace {
            prefix: Some("⋔".to_string()),
            ..HydraPrefixReplace::default()
        });
        assert_eq!(rewritten(&t, "main jjt/foo"), None);
        // A marker that only exists on a remote is not a local bookmark.
        assert_eq!(rewritten(&t, "HYS-foo@origin"), None);
        // A bare leader names no stack.
        assert_eq!(rewritten(&t, "HYS-"), None);
        // Neighbours on the row pass through beside the replaced name.
        assert_eq!(
            rewritten(&t, "main HYS-foo v1").as_deref(),
            Some("main ⋔S-foo v1")
        );
    }

    #[test]
    fn a_replaced_name_still_takes_its_stack_colour() {
        let t = topo_with(HydraPrefixReplace {
            stack_head: Some("Ψ".to_string()),
            ..HydraPrefixReplace::default()
        });
        let mut seen = Vec::new();
        let mut scratch = Vec::new();
        let mut out = Vec::new();
        let hit = rewrite_bookmarks(
            &t,
            Some(&mut seen),
            &mut scratch,
            b"\x1b[38;5;5mHYS-foo\x1b[39m",
            &mut out,
        );
        assert!(hit);
        // jj's foreground drops out, the stack's colour leads the stand-in.
        let mut want = stack_color("foo", 0);
        want.extend_from_slice("Ψfoo".as_bytes());
        want.extend_from_slice(FG_RESET);
        assert_eq!(out, want);
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
