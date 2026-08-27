use crate::ansi::{decode_utf8, emit_filtered_ansi, is_fg_color_sgr, skip_csi, FG_RESET};
use crate::config::cfg;

pub fn strip_trailing_nl(line: &[u8]) -> (&[u8], bool) {
    if line.last() == Some(&b'\n') {
        (&line[..line.len() - 1], true)
    } else {
        (line, false)
    }
}

pub fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

// Render one input line into `out`. Graph prefix is rewritten edge-by-edge
// (dim color + glyph swap); everything past the graph prefix is copied
// byte-for-byte. Lines with no graph prefix pass through verbatim.
pub fn emit_line(line: &[u8], parsed: Option<&Parsed>, out: &mut Vec<u8>) {
    let (body, trailing_nl) = strip_trailing_nl(line);
    let collapse = cfg().graph_collapse;
    match parsed {
        Some(p) => {
            emit_dim_graph(&body[..p.graph_end], collapse, out);
            out.extend_from_slice(&body[p.graph_end..]);
        }
        // No boundary: this is either a pure connector row (`├─╯`, `│`, `~`)
        // or prose that merely contains a box-drawing char. Only the former
        // may collapse — dropping every second cell of prose shreds it.
        None if has_graph_char(body) => {
            emit_dim_graph(body, collapse && is_graph_only(body), out);
        }
        None => out.extend_from_slice(body),
    }
    if trailing_nl {
        out.push(b'\n');
    }
}

const ELISION_CP: u32 = 0x7E; // ~

pub struct Parsed {
    pub graph_end: usize,
    pub content_start: usize,
    // Cells the graph prefix occupies as jj drew it, plus the count that
    // survives `graph.collapse` (pad cells dropped) and the kind of the last
    // glyph in each case. `classify_row` picks one pair per the config so the
    // graph→content gap matches what `emit_dim_graph` actually emitted.
    pub graph_col: usize,
    pub graph_col_collapsed: usize,
    pub last_is_edge: bool,
    pub last_is_edge_collapsed: bool,
}

// Graph edges are box-drawing glyphs plus the elision `~`. This set is fixed
// and jj-stable, so unlike nodes it can be recognized by codepoint.
fn is_edge_char(cp: u32) -> bool {
    matches!(cp, 0x2500..=0x257F | ELISION_CP)
}

// jj draws the graph in fixed two-cell columns: the glyph in the even cell,
// the inter-column gap in the odd one. That gap holds a space, or a
// horizontal when a connector runs through it. `graph.collapse` drops exactly
// those cells, so column N lands at cell N instead of cell 2N.
//
// Parity is what makes this safe. A bare "drop every horizontal and space"
// rule also deletes glyph cells: `├───╯` spans three columns and two of its
// glyphs are horizontals, and an inactive column is two spaces of which one
// is a glyph cell — losing either slides the rest of the row out of its
// column and off the verticals above and below it. The character check on top
// of parity is defensive: an odd cell holding anything else (corner, tee,
// node) is kept, costing one cell of width rather than losing a glyph.
fn is_horizontal_char(cp: u32) -> bool {
    matches!(cp, 0x2500 | 0x2504 | 0x2508) // ─ ┄ ┈
}

fn is_pad_cell(cell: usize, cp: u32) -> bool {
    cell % 2 == 1 && (cp == b' ' as u32 || is_horizontal_char(cp))
}

// A graph "node" is the commit marker jj draws at the rightmost graph column
// (`@ ○ ● ◆ ×`, or any glyph a custom `log_node` template emits — □, Nerd
// Font PUA, etc.). bijjou does NOT enumerate node glyphs: a node is any
// non-edge, non-space glyph in the graph region, recognized structurally by
// being followed (after any CSI) by a space or an edge. jj pads every graph
// column, so a node is always followed by its column gap (a space) or, on a
// merge tip, an edge; real content is always preceded by the gap, so its
// first glyph is never in this position (that's what keeps plain text from
// being misread as a graph row). The node glyph is forwarded unchanged — jj's
// template owns the glyph and its color.
fn is_node_at(line: &[u8], pos: usize, cp: u32, len: usize) -> bool {
    if cp == b' ' as u32 || is_edge_char(cp) {
        return false;
    }
    let mut j = pos + len;
    while let Some(after) = skip_csi(line, j) {
        j = after;
    }
    if j >= line.len() {
        return false;
    }
    if line[j] == b' ' {
        return true;
    }
    let (next_cp, _) = decode_utf8(line, j);
    is_edge_char(next_cp)
}

fn map_graph_char(cp: u32) -> Option<&'static str> {
    let c = cfg();
    match char::from_u32(cp)? {
        '─' | '┄' | '┈' => Some(c.graph_horizontal.as_str()),
        '│' => Some(c.graph_vertical.as_str()),
        '┌' | '╭' => Some(c.graph_top_left.as_str()),
        '┐' | '╮' => Some(c.graph_top_right.as_str()),
        '└' | '╰' => Some(c.graph_bottom_left.as_str()),
        '┘' | '╯' => Some(c.graph_bottom_right.as_str()),
        '├' => Some(c.graph_tee_right.as_str()),
        '┤' => Some(c.graph_tee_left.as_str()),
        '┬' => Some(c.graph_tee_down.as_str()),
        '┴' => Some(c.graph_tee_up.as_str()),
        '┼' => Some(c.graph_cross.as_str()),
        '~' => Some(c.graph_elision.as_str()),
        _ => None,
    }
}

pub fn find_boundary(line: &[u8]) -> Option<Parsed> {
    let mut i = 0;
    let mut vis_col: usize = 0;
    let mut kept_col: usize = 0;
    let mut had_graph = false;
    let mut last_is_edge = false;
    let mut last_is_edge_collapsed = false;

    while i < line.len() {
        if let Some(after) = skip_csi(line, i) {
            i = after;
            continue;
        }

        if line[i] == b' ' {
            let sep_start_byte = i;
            let sep_start_col = vis_col;
            let sep_start_kept = kept_col;
            let mut k = i;
            let mut space_count = 0;
            let mut last_space_end = i;
            loop {
                while let Some(after) = skip_csi(line, k) {
                    k = after;
                }
                if k < line.len() && line[k] == b' ' {
                    space_count += 1;
                    k += 1;
                    last_space_end = k;
                } else {
                    break;
                }
            }
            if k >= line.len() {
                return None;
            }
            let (cp, len) = decode_utf8(line, k);
            if is_edge_char(cp) || is_node_at(line, k, cp, len) {
                i = k;
                // Odd cells in the run are column pads and vanish under
                // collapse; even ones are an inactive column's own cell.
                kept_col += (vis_col..vis_col + space_count)
                    .filter(|&cell| !is_pad_cell(cell, b' ' as u32))
                    .count();
                vis_col += space_count;
            } else {
                if !had_graph {
                    return None;
                }
                return Some(Parsed {
                    graph_end: sep_start_byte,
                    content_start: last_space_end,
                    graph_col: sep_start_col,
                    graph_col_collapsed: sep_start_kept,
                    last_is_edge,
                    last_is_edge_collapsed,
                });
            }
        } else {
            let (cp, len) = decode_utf8(line, i);
            let edge = is_edge_char(cp);
            if edge || is_node_at(line, i, cp, len) {
                had_graph = true;
                last_is_edge = edge;
                i += len;
                if !is_pad_cell(vis_col, cp) {
                    kept_col += 1;
                    last_is_edge_collapsed = edge;
                }
                vis_col += 1;
            } else {
                return None;
            }
        }
    }
    None
}

pub fn has_graph_char(body: &[u8]) -> bool {
    let mut i = 0;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if is_edge_char(cp) {
            return true;
        }
        i += len;
    }
    false
}

// True when every visible cell is a graph edge or a space: the connector rows
// jj draws between commits (`├─╯`, `│`, `~`). Nodes never appear here — a row
// with a node carries content, so it has a boundary. Text containing a stray
// box-drawing char fails this, which is what keeps collapse off prose.
pub fn is_graph_only(body: &[u8]) -> bool {
    let mut i = 0;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if cp != b' ' as u32 && !is_edge_char(cp) {
            return false;
        }
        i += len;
    }
    true
}

// Node bytes pass through unchanged: jj's template (or upstream emitter) is
// responsible for picking the right glyph and label color. Bijjou forwards
// the bytes plus their surrounding ANSI verbatim.
fn emit_node(raw: &[u8], ansi: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(ansi);
    out.extend_from_slice(raw);
}

fn emit_edge(cp: u32, raw: &[u8], ansi: &[u8], out: &mut Vec<u8>) {
    let c = cfg();
    emit_filtered_ansi(ansi, out, is_fg_color_sgr);
    out.extend_from_slice(&c.edge_dim_on);
    match map_graph_char(cp) {
        Some(replacement) => out.extend_from_slice(replacement.as_bytes()),
        None => out.extend_from_slice(raw),
    }
    out.extend_from_slice(FG_RESET);
}

// Emit bytes with all visible non-space chars wrapped in dim SGR, except
// commit-node chars (○ ● ◆ @ ×) which pass through with normal intensity.
// Strips jj's fg-color codes; preserves other ANSI sequences.
//
// Once a node char has appeared on the line, any space run between two
// graph chars (node or edge) is filled with the dash glyph, one dash per
// space. Runs before the first node, or trailing past the last graph char,
// stay as plain spaces.
// What abuts an internal space run on its right when the run is flushed.
// `NonGraph` covers a newline, end of buffer, or end of the graph prefix —
// the run does not terminate at a graph char, so it is never dashed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RightSide {
    Node,
    Edge,
    NonGraph,
}

// Tracks an in-progress run of spaces inside the graph prefix so it can be
// rewritten as dashes on flush. `seen_node`/`left_was_node` describe the graph
// context to the run's left; both reset at each newline via `reset_line`.
struct GraphRun {
    seen_node: bool,
    left_was_node: bool,
    start: Option<usize>,
    spaces: usize,
}

impl GraphRun {
    fn new() -> Self {
        GraphRun { seen_node: false, left_was_node: false, start: None, spaces: 0 }
    }

    fn reset_line(&mut self) {
        self.seen_node = false;
        self.left_was_node = false;
    }

    // Replace the pending internal space run with dashes when the line has
    // already seen its node and the run is followed by another graph char.
    //
    // Rules (per the dash spec):
    //   - The cell immediately right of a node gets `dash_start` if and only
    //     if that cell is also to the left of whitespace OR a graph edge —
    //     i.e., the run is multi-cell, or it's a single cell between a node
    //     and an edge. A single-cell run between two nodes emits NO dash at
    //     all (the space is preserved).
    //   - `dash_end` is never emitted here: intra-graph runs always terminate
    //     at another graph char (never content), and the closing cap is meant
    //     to attach the run to the content boundary on the right. The DSL's
    //     content-side pad is responsible for that cap.
    fn flush(&mut self, out: &mut Vec<u8>, right: RightSide, c: &crate::config::Config) {
        let Some(start) = self.start.take() else {
            return;
        };
        let count = std::mem::replace(&mut self.spaces, 0);
        let right_is_graph = matches!(right, RightSide::Node | RightSide::Edge);
        if !(self.seen_node && right_is_graph && count > 0) {
            return;
        }
        // Single-cell gap between two nodes: emit no dash; keep the space.
        if count == 1 && self.left_was_node && right == RightSide::Node {
            return;
        }
        let original: Vec<u8> = out[start..].to_vec();
        out.truncate(start);
        out.extend_from_slice(&c.dim_on);
        // After the early return above, any run with `left_was_node` either
        // has count > 1 (next cell is whitespace) or terminates at an edge —
        // both qualify for `dash_start`.
        let head_cap = self.left_was_node && !c.dash_start.is_empty();
        for idx in 0..count {
            if head_cap && idx == 0 {
                out.extend_from_slice(c.dash_start.as_bytes());
            } else {
                out.extend_from_slice(c.dash.as_bytes());
            }
        }
        out.extend_from_slice(FG_RESET);
        // CSI bytes never contain literal space, so keeping non-space bytes
        // preserves any colour setup that was buffered between the spaces.
        for &b in &original {
            if b != b' ' {
                out.push(b);
            }
        }
    }
}

// `collapse` drops jj's inter-column pad cells (see `is_pad_cell`), pulling
// every graph column one cell left of the last. Dropped cells still forward
// their ANSI so colour state survives; only the glyph goes.
pub fn emit_dim_graph(bytes: &[u8], collapse: bool, out: &mut Vec<u8>) {
    let c = cfg();
    let mut i = 0;
    let mut cell: usize = 0;
    let mut run = GraphRun::new();

    while i < bytes.len() {
        let ansi_start = i;
        while let Some(after) = skip_csi(bytes, i) {
            i = after;
        }
        let ansi = &bytes[ansi_start..i];

        if i >= bytes.len() {
            run.flush(out, RightSide::NonGraph, c);
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            break;
        }

        if bytes[i] == b'\n' {
            run.flush(out, RightSide::NonGraph, c);
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            out.push(b'\n');
            run.reset_line();
            cell = 0;
            i += 1;
            continue;
        }

        if bytes[i] == b' ' {
            if collapse && is_pad_cell(cell, b' ' as u32) {
                emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            } else {
                if run.start.is_none() {
                    run.start = Some(out.len());
                }
                emit_filtered_ansi(ansi, out, is_fg_color_sgr);
                out.push(b' ');
                run.spaces += 1;
            }
            cell += 1;
            i += 1;
            continue;
        }

        let (cp, len) = decode_utf8(bytes, i);
        let raw = &bytes[i..i + len];
        if collapse && is_pad_cell(cell, cp) {
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            cell += 1;
            i += len;
            continue;
        }
        // emit_dim_graph only ever receives a graph prefix, so every glyph
        // here is graph: anything that isn't an edge is the node.
        let cp_is_edge = is_edge_char(cp);
        let right = if cp_is_edge { RightSide::Edge } else { RightSide::Node };
        run.flush(out, right, c);
        if cp_is_edge {
            emit_edge(cp, raw, ansi, out);
            run.left_was_node = false;
        } else {
            emit_node(raw, ansi, out);
            run.seen_node = true;
            run.left_was_node = true;
        }
        i += len;
        cell += 1;
    }

    run.flush(out, RightSide::NonGraph, c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DEFAULT_EDGE_DIM_ON, DEFAULT_GRAPH_BOTTOM_RIGHT, DEFAULT_GRAPH_HORIZONTAL,
        DEFAULT_GRAPH_TEE_RIGHT, DEFAULT_GRAPH_VERTICAL,
    };

    #[test]
    fn is_edge_char_includes_box_drawing_and_elision() {
        assert!(is_edge_char(0x2500));
        assert!(is_edge_char(0x2502));
        assert!(is_edge_char(0x256D));
        assert!(is_edge_char(0x257F));
        assert!(is_edge_char(ELISION_CP));
    }

    #[test]
    fn is_edge_char_rejects_nodes_letters_space() {
        // Node glyphs (built-in and custom) are NOT edges.
        for &cp in &[0x40u32, 0x25CB, 0x25CF, 0x25C6, 0xD7, 0x25A1, 0xF28D] {
            assert!(!is_edge_char(cp), "cp={:#x} should not be edge", cp);
        }
        // Letters and space are not edges.
        for &cp in &[0x41u32, 0x61, 0x20] {
            assert!(!is_edge_char(cp), "cp={:#x} should not be edge", cp);
        }
    }

    #[test]
    fn map_graph_char_box_drawings() {
        assert!(map_graph_char(0x2500).is_some());
        assert!(map_graph_char(0x2502).is_some());
        assert!(map_graph_char(0x256D).is_some());
        assert!(map_graph_char(0x2570).is_some());
        assert!(map_graph_char(0x251C).is_some());
        assert!(map_graph_char(0x253C).is_some());
        assert!(map_graph_char(ELISION_CP).is_some());
    }

    #[test]
    fn map_graph_char_unknown_returns_none() {
        assert!(map_graph_char(0x41).is_none());
    }

    #[test]
    fn boundary_single_node_then_content() {
        let line = b"\xe2\x97\x8b  abc";
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 1);
        assert_eq!(p.graph_end, 3);
        assert_eq!(p.content_start, 5);
    }

    #[test]
    fn boundary_returns_none_when_no_graph() {
        assert!(find_boundary(b"plain text").is_none());
    }

    #[test]
    fn boundary_custom_node_white_square() {
        // U+25A1 □ is not a built-in jj node, but jj emits it as a custom
        // node glyph: `□  content`. Structural detection must recognize it.
        let line = "□  abc".as_bytes();
        let p = find_boundary(line).expect("expected boundary for custom node");
        assert_eq!(p.graph_col, 1);
        assert_eq!(p.graph_end, "□".len());
        assert_eq!(p.content_start, "□  ".len());
        assert!(!p.last_is_edge, "node, not edge");
    }

    #[test]
    fn boundary_custom_node_pua() {
        // Nerd Font Private Use Area glyph (U+F28D) as a node.
        let line = "\u{f28d}  abc".as_bytes();
        let p = find_boundary(line).expect("expected boundary for PUA node");
        assert_eq!(p.graph_col, 1);
        assert!(!p.last_is_edge);
    }

    #[test]
    fn boundary_node_followed_by_edge() {
        // Merge tip `●─` then content: node is followed by an edge, not a
        // space, and must still be recognized as a node.
        let line = "●─ abc".as_bytes();
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 2);
        assert!(p.last_is_edge, "last graph glyph is the ─ edge");
    }

    #[test]
    fn boundary_glyph_abutting_letter_is_content_not_node() {
        // A non-edge glyph directly followed by a letter (no gap) is content,
        // not a node — guards against eating the payload. No graph → None.
        let line = "□bc".as_bytes();
        assert!(find_boundary(line).is_none());
    }

    #[test]
    fn boundary_custom_node_after_edge_column() {
        // `│ □  content`: edge column, then custom node, then gap.
        let line = "│ □  abc".as_bytes();
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 3);
        assert!(!p.last_is_edge);
    }

    #[test]
    fn boundary_skips_csi_around_graph() {
        let line = b"\x1b[31m\xe2\x97\x8b\x1b[39m  abc";
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 1);
    }

    #[test]
    fn boundary_multi_graph_columns() {
        let line = b"\xe2\x94\x82 \xe2\x94\x82 \xe2\x97\x8b  abc";
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 5);
    }

    #[test]
    fn boundary_requires_at_least_one_graph_char() {
        assert!(find_boundary(b"   abc").is_none());
    }

    fn run_emit(graph: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        emit_dim_graph(graph, false, &mut out);
        out
    }

    fn run_emit_collapsed(graph: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        emit_dim_graph(graph, true, &mut out);
        out
    }

    fn dim(glyph: &str) -> Vec<u8> {
        let mut v = DEFAULT_EDGE_DIM_ON.to_vec();
        v.extend_from_slice(glyph.as_bytes());
        v.extend_from_slice(FG_RESET);
        v
    }

    #[test]
    fn collapse_drops_column_pads_keeps_glyphs() {
        // `├─╯` is two columns: `├` + pad `─`, then `╯`.
        let mut expected = dim(DEFAULT_GRAPH_TEE_RIGHT);
        expected.extend_from_slice(&dim(DEFAULT_GRAPH_BOTTOM_RIGHT));
        assert_eq!(run_emit_collapsed("├─╯".as_bytes()), expected);
    }

    #[test]
    fn collapse_keeps_horizontals_sitting_in_glyph_cells() {
        // `├───╯` spans three columns; the middle column's own glyph is a
        // horizontal (cell 2) and must survive, or `╯` slides off its column.
        let mut expected = dim(DEFAULT_GRAPH_TEE_RIGHT);
        expected.extend_from_slice(&dim(DEFAULT_GRAPH_HORIZONTAL));
        expected.extend_from_slice(&dim(DEFAULT_GRAPH_BOTTOM_RIGHT));
        assert_eq!(run_emit_collapsed("├───╯".as_bytes()), expected);
    }

    #[test]
    fn collapse_keeps_one_cell_per_inactive_column() {
        // `│   ○`: vertical, an inactive column (two spaces), then the node.
        // One space survives so the node stays in column 2.
        let mut expected = dim(DEFAULT_GRAPH_VERTICAL);
        expected.extend_from_slice(b" ");
        expected.extend_from_slice("○".as_bytes());
        assert_eq!(run_emit_collapsed("│   ○".as_bytes()), expected);
    }

    #[test]
    fn collapse_keeps_unexpected_glyph_in_a_pad_cell() {
        // Defensive: a non-horizontal glyph in an odd cell is not a pad. It
        // is kept (one cell wider) rather than deleted.
        let mut expected = dim(DEFAULT_GRAPH_VERTICAL);
        expected.extend_from_slice(&dim(DEFAULT_GRAPH_VERTICAL));
        assert_eq!(run_emit_collapsed("││".as_bytes()), expected);
    }

    #[test]
    fn collapse_forwards_ansi_of_dropped_cells() {
        // The pad's own SGR must survive even though its glyph does not:
        // dropping a reset would leak colour into the rest of the line.
        let out = run_emit_collapsed("│\x1b[1m \x1b[22m○".as_bytes());
        let mut expected = dim(DEFAULT_GRAPH_VERTICAL);
        expected.extend_from_slice(b"\x1b[1m\x1b[22m");
        expected.extend_from_slice("○".as_bytes());
        assert_eq!(out, expected);
    }

    #[test]
    fn collapsed_graph_col_counts_kept_cells() {
        // `│ │ ○  abc`: 5 cells drawn, 3 after collapse.
        let line = "│ │ ○  abc".as_bytes();
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 5);
        assert_eq!(p.graph_col_collapsed, 3);
        assert!(!p.last_is_edge);
        assert!(!p.last_is_edge_collapsed);
    }

    #[test]
    fn collapsed_last_glyph_kind_ignores_dropped_pad() {
        // `○─ abc`: the trailing `─` is a pad cell, so the collapsed prefix
        // ends on the node and its gap gets a node-side dash cap.
        let line = "○─ abc".as_bytes();
        let p = find_boundary(line).expect("expected boundary");
        assert!(p.last_is_edge);
        assert!(!p.last_is_edge_collapsed);
        assert_eq!(p.graph_col_collapsed, 1);
    }

    #[test]
    fn is_graph_only_separates_connector_rows_from_prose() {
        assert!(is_graph_only("├─╯".as_bytes()));
        assert!(is_graph_only("│ │".as_bytes()));
        assert!(is_graph_only("~".as_bytes()));
        // A description that happens to contain a box-drawing char.
        assert!(!is_graph_only("fix: draw ─ separators".as_bytes()));
    }

    #[test]
    fn dim_node_chars_pass_through_verbatim() {
        assert_eq!(run_emit(b"\xe2\x97\x8b"), b"\xe2\x97\x8b"); // ○
        assert_eq!(run_emit(b"\xe2\x97\x86"), b"\xe2\x97\x86"); // ◆
        assert_eq!(run_emit(b"@"), b"@");
        let with_ansi = b"\x1b[1m\x1b[38;5;2m@\x1b[0m";
        assert_eq!(run_emit(with_ansi), with_ansi);
    }

    #[test]
    fn dim_custom_node_passthrough() {
        // A graph prefix is all bijjou ever feeds emit_dim_graph, so any
        // non-edge glyph in it is a node and must pass through verbatim
        // (jj owns the glyph + its color), not get the edge-dim wrapper.
        assert_eq!(run_emit("□".as_bytes()), "□".as_bytes()); // U+25A1
        assert_eq!(run_emit("\u{f28d}".as_bytes()), "\u{f28d}".as_bytes()); // PUA
        assert_eq!(run_emit("■".as_bytes()), "■".as_bytes()); // U+25A0
    }

    #[test]
    fn dim_box_drawing_gets_edge_dim() {
        let out = run_emit(b"\xe2\x94\x82");
        let mut expected = DEFAULT_EDGE_DIM_ON.to_vec();
        expected.extend_from_slice(DEFAULT_GRAPH_VERTICAL.as_bytes());
        expected.extend_from_slice(FG_RESET);
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_spaces_passthrough() {
        assert_eq!(run_emit(b"   "), b"   ");
    }
}
