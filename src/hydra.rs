// Hydra awareness: recognize the stacks of a jj hydra in the log and mark
// them up. A hydra merges several linear stacks as siblings off one base, so
// `jj log` prints one graph column per stack. Two things follow from that:
//
//   - The log's top stack has no `├─╯` row under it. jj closes a graph column
//     only when the branch to its *left* ends, so every stack but the topmost
//     gets a closing row; the topmost runs straight into its neighbour.
//     `hydra.top-stack-padding` draws the separator jj skipped.
//   - Each stack owns a column, so colouring its nodes per stack makes the
//     columns readable at a glance (`hydra.colors`).
//
// The bookmark naming comes from `hydra.prefixes`, so a row is classified
// from its `bookmarks` field alone: no subprocess, no repo lookup. A repo
// that renamed its hydra bookmarks says so in config; a repo with no hydra
// has no bookmark that matches, so it gets no markup at all.

use std::collections::HashMap;

use crate::ansi::skip_csi;
use crate::config::{cfg, HydraColors, HydraPrefixes};
use crate::render::{emit_dim_graph, graph_nodes_to_verticals};

const BOOKMARKS_FIELD: &str = "bookmarks";

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

// Per-row state, walked top-down with the log.
pub struct Walk {
    // `None` with `hydra.enable = false`: every row passes through untouched.
    topo: Option<Topology>,
    // Stack names in the order the log first named them, top first — a
    // stack's slot in `hydra.colors` when that is a palette.
    seen: Vec<String>,
    // SGR for the stack the walk is inside; empty outside any stack.
    color: Vec<u8>,
    // SGR for a single `HYWC-*` row, which belongs to a stack without being
    // in it: the colour applies to that row and is not carried down.
    wc_color: Vec<u8>,
    stacks_seen: usize,
    // Reused scratch for the de-ANSI'd bookmarks field.
    scratch: Vec<u8>,
}

impl Walk {
    pub fn start() -> Walk {
        Walk {
            topo: cfg()
                .hydra_enable
                .then(|| Topology::from_prefixes(&cfg().hydra_prefixes)),
            seen: Vec::new(),
            color: Vec::new(),
            wc_color: Vec::new(),
            stacks_seen: 0,
            scratch: Vec::new(),
        }
    }

    // Classify one commit row and return the SGR its graph node should carry
    // (`None` leaves the node exactly as jj drew it). `prefix` is the row's
    // graph prefix as jj drew it: when this row opens the second stack, the
    // top stack's missing separator is drawn from it into `out` first.
    //
    // Rows must arrive in log order — the walk carries a stack's colour down
    // from its marker through its content commits.
    pub fn node_color(
        &mut self,
        fields: &HashMap<String, Vec<u8>>,
        prefix: &[u8],
        out: &mut Vec<u8>,
    ) -> Option<&[u8]> {
        let bookmarks = fields
            .get(BOOKMARKS_FIELD)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        strip_ansi_into(bookmarks, &mut scratch);
        // `None` topology is `hydra.enable = false`: nothing to classify.
        let mark = self.topo.as_ref().map(|topo| mark_of(topo, &scratch));
        self.scratch = scratch;
        let mark = mark?;

        match mark {
            Mark::Outside => {
                self.color.clear();
                None
            }
            Mark::Inside => (!self.color.is_empty()).then_some(self.color.as_slice()),
            Mark::Stack(name) => {
                if self.stacks_seen == 1 && cfg().hydra_top_stack_padding {
                    emit_padding(prefix, out);
                }
                self.stacks_seen += 1;
                let index = self.index_of(&name);
                self.color = stack_color(&name, index);
                (!self.color.is_empty()).then_some(self.color.as_slice())
            }
            // A working copy sits above the head, outside every stack, so it
            // ends whichever stack the walk was in — but it is that stack's
            // row, so it takes the stack's colour.
            Mark::WorkingCopy(name) => {
                self.color.clear();
                let index = self.index_of(&name);
                self.wc_color = stack_color(&name, index);
                (!self.wc_color.is_empty()).then_some(self.wc_color.as_slice())
            }
        }
    }

    // Position in the graph, top first: the log itself is the order, so a
    // stack keeps its slot for the whole run once met.
    fn index_of(&mut self, name: &str) -> usize {
        if let Some(i) = self.seen.iter().position(|n| n == name) {
            return i;
        }
        self.seen.push(name.to_string());
        self.seen.len() - 1
    }
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

// A stack's colour has to be stable across runs and distinct from its
// neighbours'. FNV-1a over the name picks a hue; saturation and lightness are
// fixed so every stack lands in the same legible band. Hashing straight into
// rgb instead would hand out near-blacks and near-whites.
fn hash_color(name: &str) -> Vec<u8> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let (r, g, b) = hsl_to_rgb((h % 360) as u32, 0.68, 0.62);
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
            wc_color: Vec::new(),
            stacks_seen: 0,
            scratch: Vec::new(),
        }
    }

    // One commit row with `bookmarks` set, under the default config (hashed
    // colours) and an empty graph prefix, so no padding row is in play.
    fn row(walk: &mut Walk, bookmarks: &str) -> Option<Vec<u8>> {
        let mut fields = HashMap::new();
        fields.insert(BOOKMARKS_FIELD.to_string(), bookmarks.as_bytes().to_vec());
        let mut out = Vec::new();
        walk.node_color(&fields, b"", &mut out).map(<[u8]>::to_vec)
    }

    #[test]
    fn a_working_copy_takes_its_stack_colour_without_carrying_it() {
        let mut walk = walk();
        let wc = row(&mut walk, "HYWC-delta").expect("working copy is coloured");
        // The head sits between the working copies and the stacks: uncoloured,
        // and it carries nothing down from the working copy above it.
        assert_eq!(row(&mut walk, "HYH"), None);
        assert_eq!(row(&mut walk, ""), None);
        // The stack itself, and its content commits, match its working copy.
        let marker = row(&mut walk, "HYS-delta").expect("stack marker is coloured");
        assert_eq!(marker, wc);
        assert_eq!(row(&mut walk, ""), Some(marker));
    }

    #[test]
    fn palette_index_follows_log_order_then_first_sight() {
        let mut walk = walk();
        assert_eq!(walk.index_of("delta"), 0);
        assert_eq!(walk.index_of("gamma"), 1);
        // Later sightings append, stably.
        assert_eq!(walk.index_of("later"), 2);
        assert_eq!(walk.index_of("other"), 3);
        assert_eq!(walk.index_of("later"), 2);
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
    fn strip_ansi_leaves_the_bookmark_names() {
        let mut out = Vec::new();
        strip_ansi_into(b"\x1b[38;5;5mHYS-delta\x1b[39m", &mut out);
        assert_eq!(out, b"HYS-delta");
    }
}
