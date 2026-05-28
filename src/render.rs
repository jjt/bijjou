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
    match parsed {
        Some(p) => {
            emit_dim_graph(&body[..p.graph_end], out);
            out.extend_from_slice(&body[p.graph_end..]);
        }
        None if has_graph_char(body) => {
            emit_dim_graph(body, out);
        }
        None => out.extend_from_slice(body),
    }
    if trailing_nl {
        out.push(b'\n');
    }
}

const WC_CP: u32 = 0x40; // @
const MUTABLE_CP: u32 = 0x25CB; // ○
const ALTERNATE_CP: u32 = 0x25CF; // ●
const IMMUTABLE_CP: u32 = 0x25C6; // ◆
const CONFLICT_CP: u32 = 0xD7; // ×
const ELISION_CP: u32 = 0x7E; // ~

#[allow(dead_code)]
pub struct Parsed {
    pub graph_end: usize,
    pub content_start: usize,
    pub graph_col: usize,
    pub last_is_edge: bool,
}

fn is_graph_char(cp: u32) -> bool {
    if matches!(cp, 0x2500..=0x257F | ELISION_CP) {
        return true;
    }
    is_node_char(cp)
}

// A "node" is any codepoint that a jj template might emit at the rightmost
// graph column. Includes jj's built-in node chars (back-compat when no
// template override is applied) plus every configured node icon — so bijjou
// recognizes whatever the user's template emits as part of the graph
// prefix, not as content.
fn is_node_char(cp: u32) -> bool {
    if matches!(
        cp,
        WC_CP | MUTABLE_CP | ALTERNATE_CP | IMMUTABLE_CP | CONFLICT_CP
    ) {
        return true;
    }
    let c = cfg();
    let icons = [
        &c.wc_icon,
        &c.mutable_icon,
        &c.immutable_icon,
        &c.conflict_icon,
        &c.empty_icon,
        &c.wc_empty_icon,
        &c.empty_immutable_icon,
        &c.hidden_icon,
        &c.fallback_icon,
    ];
    icons
        .iter()
        .filter_map(|s| s.chars().next())
        .any(|ch| ch as u32 == cp)
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
    let mut had_graph = false;
    let mut last_is_edge = false;

    while i < line.len() {
        if let Some(after) = skip_csi(line, i) {
            i = after;
            continue;
        }

        if line[i] == b' ' {
            let sep_start_byte = i;
            let sep_start_col = vis_col;
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
            let (cp, _len) = decode_utf8(line, k);
            if is_graph_char(cp) {
                i = k;
                vis_col += space_count;
            } else {
                if !had_graph {
                    return None;
                }
                return Some(Parsed {
                    graph_end: sep_start_byte,
                    content_start: last_space_end,
                    graph_col: sep_start_col,
                    last_is_edge,
                });
            }
        } else {
            let (cp, len) = decode_utf8(line, i);
            if is_graph_char(cp) {
                had_graph = true;
                last_is_edge = !is_node_char(cp);
                i += len;
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
        if is_graph_char(cp) {
            return true;
        }
        i += len;
    }
    false
}

// Node bytes pass through unchanged: jj's template (or upstream emitter) is
// responsible for picking the right icon and label color. Bijjou forwards
// the bytes plus their surrounding ANSI verbatim. See `bijjou jj-config`
// for a template that wires bijjou's icon config into jj.
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
pub fn emit_dim_graph(bytes: &[u8], out: &mut Vec<u8>) {
    let c = cfg();
    let mut i = 0;
    let mut seen_node = false;
    let mut prev_was_node = false;
    let mut run_start: Option<usize> = None;
    let mut run_space_count: usize = 0;

    while i < bytes.len() {
        let ansi_start = i;
        while let Some(after) = skip_csi(bytes, i) {
            i = after;
        }
        let ansi = &bytes[ansi_start..i];

        if i >= bytes.len() {
            flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, false, false, c);
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            break;
        }

        if bytes[i] == b'\n' {
            flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, false, false, c);
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            out.push(b'\n');
            seen_node = false;
            prev_was_node = false;
            i += 1;
            continue;
        }

        if bytes[i] == b' ' {
            if run_start.is_none() {
                run_start = Some(out.len());
            }
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            out.push(b' ');
            run_space_count += 1;
            i += 1;
            continue;
        }

        let (cp, len) = decode_utf8(bytes, i);
        let raw = &bytes[i..i + len];
        let cp_is_graph = is_graph_char(cp);
        let cp_is_node = is_node_char(cp);
        flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, cp_is_graph, cp_is_node, c);
        if cp_is_node {
            emit_node(raw, ansi, out);
            seen_node = true;
            prev_was_node = true;
        } else {
            emit_edge(cp, raw, ansi, out);
            prev_was_node = false;
        }
        i += len;
    }

    flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, false, false, c);
}

// Replace a pending internal space run with dashes when the line has already
// seen its node and the run is followed by another graph char.
//
// Rules (per the dash spec):
//   - The cell immediately right of a node gets `dash_start` if and only if
//     that cell is also to the left of whitespace OR a graph edge — i.e.,
//     the run is multi-cell, or it's a single cell between a node and an
//     edge. A single-cell run between two nodes emits NO dash at all (the
//     space is preserved).
//   - `dash_end` is never emitted here: intra-graph runs always terminate
//     at another graph char (never content), and the closing cap is meant
//     to attach the run to the content boundary on the right. The DSL's
//     content-side pad is responsible for that cap.
fn flush_internal_run(
    out: &mut Vec<u8>,
    run_start: &mut Option<usize>,
    run_space_count: &mut usize,
    seen_node: bool,
    left_was_node: bool,
    right_is_graph: bool,
    right_is_node: bool,
    c: &crate::config::Config,
) {
    let Some(start) = run_start.take() else {
        return;
    };
    let count = std::mem::replace(run_space_count, 0);
    if !(seen_node && right_is_graph && count > 0) {
        return;
    }
    // Single-cell gap between two nodes: emit no dash; keep the space.
    if count == 1 && left_was_node && right_is_node {
        return;
    }
    let original: Vec<u8> = out[start..].to_vec();
    out.truncate(start);
    out.extend_from_slice(&c.dim_on);
    // After the early return above, any run with `left_was_node` either has
    // count > 1 (next cell is whitespace) or terminates at an edge — both
    // qualify for `dash_start`.
    let head_cap = left_was_node && !c.dash_start.is_empty();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DEFAULT_EDGE_DIM_ON, DEFAULT_GRAPH_VERTICAL};

    #[test]
    fn is_node_char_recognizes_all_nodes() {
        for &cp in &[WC_CP, MUTABLE_CP, ALTERNATE_CP, IMMUTABLE_CP, CONFLICT_CP] {
            assert!(is_node_char(cp), "cp={:#x} should be node char", cp);
        }
    }

    #[test]
    fn is_node_char_rejects_non_nodes() {
        for &cp in &[0x2502u32, 0x2500, 0x41, 0x20] {
            assert!(!is_node_char(cp), "cp={:#x} should not be node char", cp);
        }
    }

    #[test]
    fn is_graph_char_includes_box_drawing() {
        assert!(is_graph_char(0x2500));
        assert!(is_graph_char(0x2502));
        assert!(is_graph_char(0x256D));
        assert!(is_graph_char(0x257F));
    }

    #[test]
    fn is_graph_char_includes_node_chars() {
        assert!(is_graph_char(WC_CP));
        assert!(is_graph_char(MUTABLE_CP));
        assert!(is_graph_char(IMMUTABLE_CP));
        assert!(is_graph_char(CONFLICT_CP));
        assert!(is_graph_char(ELISION_CP));
    }

    #[test]
    fn is_graph_char_rejects_letters() {
        assert!(!is_graph_char(0x41));
        assert!(!is_graph_char(0x61));
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
        emit_dim_graph(graph, &mut out);
        out
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
