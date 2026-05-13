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

fn write_gap(out: &mut Vec<u8>, p: &Parsed, target_col: usize, dashed: bool, left_is_node: bool) {
    let c = cfg();
    let gap = target_col - p.graph_col;
    let margin = c.dash_margin;
    let min_for_dashes = 2 * margin + 1;

    if dashed && gap >= min_for_dashes {
        let fill = gap - 2 * margin;
        for _ in 0..margin {
            out.push(b' ');
        }
        out.extend_from_slice(&c.dim_on);
        let arrow_in_use = !c.dash_arrow.is_empty();
        let dash_count = if arrow_in_use { fill - 1 } else { fill };
        let use_caps = margin == 0;
        let head_cap = use_caps && left_is_node && !c.dash_start.is_empty();
        let tail_cap = use_caps && !arrow_in_use && !c.dash_end.is_empty();
        for idx in 0..dash_count {
            let is_first = idx == 0;
            let is_last = idx + 1 == dash_count;
            if tail_cap && is_last {
                out.extend_from_slice(c.dash_end.as_bytes());
            } else if head_cap && is_first {
                out.extend_from_slice(c.dash_start.as_bytes());
            } else {
                out.extend_from_slice(c.dash.as_bytes());
            }
        }
        if arrow_in_use {
            out.extend_from_slice(c.dash_arrow.as_bytes());
        }
        out.extend_from_slice(FG_RESET);
        for _ in 0..margin {
            out.push(b' ');
        }
    } else {
        for _ in 0..gap {
            out.push(b' ');
        }
    }
}

// Render one input line into `out`. Returns true if a line was emitted, false
// if the configured filter dropped it.
pub fn emit_line(line: &[u8], parsed: Option<&Parsed>, target_col: usize, out: &mut Vec<u8>) -> bool {
    let (body, trailing_nl) = strip_trailing_nl(line);
    let c = cfg();

    if c.hide_vertical_only_lines && parsed.is_none() && is_vertical_only_line(body) {
        return false;
    }

    match parsed {
        Some(p) => {
            let (is_empty, is_divergent) = line_flags(body);
            let graph = &body[..p.graph_end];
            emit_dim_graph(graph, out, is_empty, is_divergent);
            let dashed = has_node_char(graph);
            let tail_is_node = graph_tail_is_node(graph);
            write_gap(out, p, target_col, dashed, tail_is_node);
            write_stripping_marker(&body[p.content_start..], out);
        }
        None if has_graph_char(body) => {
            let (is_empty, is_divergent) = line_flags(body);
            emit_dim_graph(body, out, is_empty, is_divergent);
        }
        None => out.extend_from_slice(body),
    }

    if trailing_nl {
        out.push(b'\n');
    }
    true
}

const WC_CP: u32 = 0x40; // @
const MUTABLE_CP: u32 = 0x25CB; // ○
const ALTERNATE_CP: u32 = 0x25CF; // ●
const IMMUTABLE_CP: u32 = 0x25C6; // ◆
const CONFLICT_CP: u32 = 0xD7; // ×
const ELISION_CP: u32 = 0x7E; // ~

pub struct Parsed {
    pub graph_end: usize,
    pub content_start: usize,
    pub graph_col: usize,
}

fn is_graph_char(cp: u32) -> bool {
    matches!(
        cp,
        0x2500..=0x257F   // box drawing block
        | WC_CP
        | ELISION_CP
        | CONFLICT_CP
        | MUTABLE_CP
        | ALTERNATE_CP
        | IMMUTABLE_CP
    )
}

fn is_node_char(cp: u32) -> bool {
    matches!(cp, WC_CP | MUTABLE_CP | ALTERNATE_CP | IMMUTABLE_CP | CONFLICT_CP)
}

fn map_node_char(cp: u32) -> Option<&'static str> {
    let c = cfg();
    match cp {
        WC_CP => Some(c.wc_icon.as_str()),
        MUTABLE_CP => Some(c.mutable_icon.as_str()),
        IMMUTABLE_CP => Some(c.immutable_icon.as_str()),
        CONFLICT_CP => Some(c.conflict_icon.as_str()),
        ALTERNATE_CP => Some(c.alternate_icon.as_str()),
        _ => None,
    }
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
                });
            }
        } else {
            let (cp, len) = decode_utf8(line, i);
            if is_graph_char(cp) {
                had_graph = true;
                i += len;
                vis_col += 1;
            } else {
                return None;
            }
        }
    }
    None
}

const VERTICAL_CP: u32 = 0x2502; // │

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

pub fn has_node_char(body: &[u8]) -> bool {
    let mut i = 0;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if is_node_char(cp) {
            return true;
        }
        i += len;
    }
    false
}

fn graph_tail_is_node(body: &[u8]) -> bool {
    let mut i = 0;
    let mut last_was_node = false;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        if body[i] == b' ' {
            i += 1;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if is_graph_char(cp) {
            last_was_node = is_node_char(cp);
        }
        i += len;
    }
    last_was_node
}

pub fn is_vertical_only_line(body: &[u8]) -> bool {
    let markers = strip_markers();
    let mut i = 0;
    let mut had_vertical = false;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        if body[i] == b' ' {
            i += 1;
            continue;
        }
        if let Some(skip) = match_marker(body, i, &markers) {
            i += skip;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if cp == VERTICAL_CP {
            had_vertical = true;
            i += len;
        } else {
            return false;
        }
    }
    had_vertical
}

pub fn line_flags(body: &[u8]) -> (bool, bool) {
    let c = cfg();
    let em = c.empty_marker.as_bytes();
    let dm = c.divergent_marker.as_bytes();
    let mut empty = false;
    let mut divergent = false;
    let mut i = 0;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        if !em.is_empty() && body[i..].starts_with(em) {
            empty = true;
            i += em.len();
            continue;
        }
        if !dm.is_empty() && body[i..].starts_with(dm) {
            divergent = true;
            i += dm.len();
            continue;
        }
        let (_, len) = decode_utf8(body, i);
        i += len;
    }
    (empty, divergent)
}

fn strip_markers() -> Vec<&'static [u8]> {
    let c = cfg();
    let mut v: Vec<&'static [u8]> = Vec::with_capacity(3);
    let em = c.empty_marker.as_bytes();
    if !em.is_empty() {
        v.push(em);
    }
    let dm = c.divergent_marker.as_bytes();
    if !dm.is_empty() {
        v.push(dm);
    }
    let act = c.activation_marker.as_bytes();
    if !act.is_empty() {
        v.push(act);
    }
    v
}

fn match_marker<'a>(bytes: &[u8], i: usize, markers: &'a [&'a [u8]]) -> Option<usize> {
    markers
        .iter()
        .find(|m| bytes[i..].starts_with(m))
        .map(|m| m.len())
}

pub fn write_stripping_marker(content: &[u8], out: &mut Vec<u8>) {
    let markers = strip_markers();
    let mut i = 0;
    while i < content.len() {
        if let Some(skip) = match_marker(content, i, &markers) {
            i += skip;
            continue;
        }
        out.push(content[i]);
        i += 1;
    }
}

fn pick_node_icon(cp: u32, is_empty: bool, is_divergent: bool) -> Option<&'static str> {
    let c = cfg();
    if is_empty {
        return Some(match cp {
            WC_CP if is_divergent => c.empty_immutable_icon.as_str(),
            WC_CP => c.wc_empty_icon.as_str(),
            IMMUTABLE_CP => c.empty_immutable_icon.as_str(),
            _ => c.empty_icon.as_str(),
        });
    }
    if cp == WC_CP && is_divergent {
        return Some(c.immutable_icon.as_str());
    }
    map_node_char(cp)
}

fn emit_node(
    cp: u32,
    raw: &[u8],
    ansi: &[u8],
    is_empty: bool,
    is_divergent: bool,
    out: &mut Vec<u8>,
) {
    let c = cfg();
    // Mutable (○) and immutable (◆, or @ flagged as divergent) share the
    // darker color override; other nodes preserve jj's original ANSI.
    let darken = cp == MUTABLE_CP || cp == IMMUTABLE_CP || (cp == WC_CP && is_divergent);
    if darken {
        emit_filtered_ansi(ansi, out, is_fg_color_sgr);
        out.extend_from_slice(&c.mutable_node_color);
    } else {
        out.extend_from_slice(ansi);
    }
    match pick_node_icon(cp, is_empty, is_divergent) {
        Some(icon) => out.extend_from_slice(icon.as_bytes()),
        None => out.extend_from_slice(raw),
    }
    if darken {
        out.extend_from_slice(FG_RESET);
    }
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
pub fn emit_dim_graph(bytes: &[u8], out: &mut Vec<u8>, is_empty: bool, is_divergent: bool) {
    let markers = strip_markers();
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
            flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, false, c);
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            break;
        }

        if bytes[i] == b'\n' {
            flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, false, c);
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

        if let Some(skip) = match_marker(bytes, i, &markers) {
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            i += skip;
            continue;
        }

        let (cp, len) = decode_utf8(bytes, i);
        let raw = &bytes[i..i + len];
        let cp_is_graph = is_graph_char(cp);
        flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, cp_is_graph, c);
        if is_node_char(cp) {
            emit_node(cp, raw, ansi, is_empty, is_divergent, out);
            seen_node = true;
            prev_was_node = true;
        } else {
            emit_edge(cp, raw, ansi, out);
            prev_was_node = false;
        }
        i += len;
    }

    flush_internal_run(out, &mut run_start, &mut run_space_count, seen_node, prev_was_node, false, c);
}

// Replace a pending internal space run with dashes when the line has already
// seen its node and the run is followed by another graph char. The first dash
// is swapped for `dash_start` when it abuts the node (dash_margin == 0).
fn flush_internal_run(
    out: &mut Vec<u8>,
    run_start: &mut Option<usize>,
    run_space_count: &mut usize,
    seen_node: bool,
    left_was_node: bool,
    right_is_graph: bool,
    c: &crate::config::Config,
) {
    let Some(start) = run_start.take() else {
        return;
    };
    let count = std::mem::replace(run_space_count, 0);
    if !(seen_node && right_is_graph && count > 0) {
        return;
    }
    let original: Vec<u8> = out[start..].to_vec();
    out.truncate(start);
    out.extend_from_slice(&c.dim_on);
    let head_cap = c.dash_margin == 0 && left_was_node && !c.dash_start.is_empty();
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
    use crate::ansi::{EMPTY_MARKER_BYTES, DIVERGENT_MARKER_BYTES};
    use crate::config::{
        DEFAULT_ALTERNATE_ICON, DEFAULT_CONFLICT_ICON, DEFAULT_EDGE_DIM_ON,
        DEFAULT_EMPTY_IMMUTABLE_ICON, DEFAULT_GRAPH_VERTICAL, DEFAULT_IMMUTABLE_ICON,
        DEFAULT_MUTABLE_ICON, DEFAULT_MUTABLE_NODE_COLOR, DEFAULT_WC_EMPTY_ICON, DEFAULT_WC_ICON,
    };

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
    fn map_node_char_covers_each_node() {
        assert_eq!(map_node_char(WC_CP), Some(DEFAULT_WC_ICON));
        assert_eq!(map_node_char(MUTABLE_CP), Some(DEFAULT_MUTABLE_ICON));
        assert_eq!(map_node_char(IMMUTABLE_CP), Some(DEFAULT_IMMUTABLE_ICON));
        assert_eq!(map_node_char(CONFLICT_CP), Some(DEFAULT_CONFLICT_ICON));
        assert_eq!(map_node_char(ALTERNATE_CP), Some(DEFAULT_ALTERNATE_ICON));
    }

    #[test]
    fn map_node_char_returns_none_for_other() {
        assert_eq!(map_node_char(0x41), None);
        assert_eq!(map_node_char(0x2502), None);
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
    fn line_flags_plain_line() {
        assert_eq!(line_flags(b"hello world"), (false, false));
    }

    #[test]
    fn line_flags_detects_empty_marker() {
        let mut buf = b"prefix ".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b" suffix");
        assert_eq!(line_flags(&buf), (true, false));
    }

    #[test]
    fn line_flags_detects_divergent_marker() {
        let mut buf = b"x".to_vec();
        buf.extend_from_slice(DIVERGENT_MARKER_BYTES);
        assert_eq!(line_flags(&buf), (false, true));
    }

    #[test]
    fn line_flags_detects_both() {
        let mut buf = Vec::new();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b" ");
        buf.extend_from_slice(DIVERGENT_MARKER_BYTES);
        assert_eq!(line_flags(&buf), (true, true));
    }

    #[test]
    fn line_flags_skips_csi_sequences() {
        let buf = b"\x1b[38;5;10m\x1b[39m";
        assert_eq!(line_flags(buf), (false, false));
    }

    #[test]
    fn line_flags_finds_marker_inside_colored_segment() {
        let mut buf = b"\x1b[38;5;10m".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"\x1b[39m");
        assert_eq!(line_flags(&buf), (true, false));
    }

    #[test]
    fn strip_no_markers_passthrough() {
        let mut out = Vec::new();
        write_stripping_marker(b"hello world", &mut out);
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn strip_empty_marker_only() {
        let mut buf = b"a".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"b");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"ab");
    }

    #[test]
    fn strip_divergent_marker_only() {
        let mut buf = DIVERGENT_MARKER_BYTES.to_vec();
        buf.extend_from_slice(b"x");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"x");
    }

    #[test]
    fn strip_both_markers_in_one_pass() {
        let mut buf = b"start".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"mid");
        buf.extend_from_slice(DIVERGENT_MARKER_BYTES);
        buf.extend_from_slice(b"end");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"startmidend");
    }

    #[test]
    fn strip_preserves_ansi_around_markers() {
        let mut buf = b"\x1b[38;5;10m".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"\x1b[39m");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"\x1b[38;5;10m\x1b[39m");
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

    fn run_emit(graph: &[u8], is_empty: bool, is_divergent: bool) -> Vec<u8> {
        let mut out = Vec::new();
        emit_dim_graph(graph, &mut out, is_empty, is_divergent);
        out
    }

    fn darken(body: &[u8]) -> Vec<u8> {
        let mut v = DEFAULT_MUTABLE_NODE_COLOR.to_vec();
        v.extend_from_slice(body);
        v.extend_from_slice(FG_RESET);
        v
    }

    #[test]
    fn dim_mutable_circle_gets_darken_and_icon() {
        let out = run_emit(b"\xe2\x97\x8b", false, false);
        assert_eq!(out, darken(DEFAULT_MUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_gets_darken_and_lock() {
        let out = run_emit(b"\xe2\x97\x86", false, true);
        assert_eq!(out, darken(DEFAULT_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_darkens_even_without_flag() {
        let out = run_emit(b"\xe2\x97\x86", false, false);
        assert_eq!(out, darken(DEFAULT_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_mutable_wc_preserves_jj_color() {
        let input = b"\x1b[1m\x1b[38;5;2m@\x1b[0m";
        let out = run_emit(input, false, false);
        let mut expected = b"\x1b[1m\x1b[38;5;2m".to_vec();
        expected.extend_from_slice(DEFAULT_WC_ICON.as_bytes());
        expected.extend_from_slice(b"\x1b[0m");
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_empty_wc_uses_empty_icon() {
        let out = run_emit(b"@", true, false);
        assert_eq!(out, DEFAULT_WC_EMPTY_ICON.as_bytes());
    }

    #[test]
    fn dim_divergent_wc_darkens_and_uses_lock() {
        let out = run_emit(b"@", false, true);
        assert_eq!(out, darken(DEFAULT_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_empty_uses_empty_immutable_icon() {
        let out = run_emit(b"\xe2\x97\x86", true, true);
        assert_eq!(out, darken(DEFAULT_EMPTY_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_strips_empty_marker() {
        assert_eq!(run_emit(EMPTY_MARKER_BYTES, false, false), b"");
    }

    #[test]
    fn dim_strips_divergent_marker() {
        assert_eq!(run_emit(DIVERGENT_MARKER_BYTES, false, false), b"");
    }

    #[test]
    fn dim_box_drawing_gets_edge_dim() {
        let out = run_emit(b"\xe2\x94\x82", false, false);
        let mut expected = DEFAULT_EDGE_DIM_ON.to_vec();
        expected.extend_from_slice(DEFAULT_GRAPH_VERTICAL.as_bytes());
        expected.extend_from_slice(FG_RESET);
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_spaces_passthrough() {
        assert_eq!(run_emit(b"   ", false, false), b"   ");
    }

    #[test]
    fn vertical_only_single_pipe() {
        assert!(is_vertical_only_line("│".as_bytes()));
    }

    #[test]
    fn vertical_only_pipes_with_spaces() {
        assert!(is_vertical_only_line("│ │ │".as_bytes()));
    }

    #[test]
    fn vertical_only_with_csi_wrapper() {
        assert!(is_vertical_only_line(b"\x1b[38;5;8m\xe2\x94\x82\x1b[39m"));
    }

    #[test]
    fn vertical_only_rejects_empty_line() {
        assert!(!is_vertical_only_line(b""));
        assert!(!is_vertical_only_line(b"   "));
    }

    #[test]
    fn vertical_only_rejects_node_line() {
        assert!(!is_vertical_only_line("│ ○".as_bytes()));
    }

    #[test]
    fn vertical_only_rejects_corner() {
        assert!(!is_vertical_only_line("├─╯".as_bytes()));
    }

    #[test]
    fn vertical_only_ignores_strip_markers() {
        let mut buf = "│".as_bytes().to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        assert!(is_vertical_only_line(&buf));
    }

    #[test]
    fn dim_strips_fg_color_around_mutable_node() {
        let out = run_emit(b"\x1b[38;5;14m\xe2\x97\x8b\x1b[39m", false, false);
        assert_eq!(out, darken(DEFAULT_MUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn graph_tail_is_node_detects_trailing_node() {
        assert!(graph_tail_is_node("│ │ ○".as_bytes()));
        assert!(!graph_tail_is_node("○ │ │".as_bytes()));
        assert!(!graph_tail_is_node("├─╯".as_bytes()));
    }
}
