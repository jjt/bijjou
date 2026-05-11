use crate::ansi::{
    decode_utf8, emit_filtered_ansi, is_fg_color_sgr, skip_csi, EMPTY_MARKER, EMPTY_MARKER_BYTES,
    FG_RESET, IMMUTABLE_MARKER, IMMUTABLE_MARKER_BYTES,
};
use crate::config::cfg;

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
    let mut empty = false;
    let mut immutable = false;
    let mut i = 0;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if cp == EMPTY_MARKER {
            empty = true;
        } else if cp == IMMUTABLE_MARKER {
            immutable = true;
        }
        i += len;
    }
    (empty, immutable)
}

fn strip_markers() -> Vec<&'static [u8]> {
    let mut v: Vec<&'static [u8]> = vec![EMPTY_MARKER_BYTES, IMMUTABLE_MARKER_BYTES];
    let act = cfg().activation_marker.as_bytes();
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

fn pick_node_icon(cp: u32, is_empty: bool, is_immutable: bool) -> Option<&'static str> {
    let c = cfg();
    if is_empty {
        return Some(match cp {
            WC_CP if is_immutable => c.empty_immutable_icon.as_str(),
            WC_CP => c.wc_empty_icon.as_str(),
            IMMUTABLE_CP => c.empty_immutable_icon.as_str(),
            _ => c.empty_icon.as_str(),
        });
    }
    if cp == WC_CP && is_immutable {
        return Some(c.immutable_icon.as_str());
    }
    map_node_char(cp)
}

fn emit_node(
    cp: u32,
    raw: &[u8],
    ansi: &[u8],
    is_empty: bool,
    is_immutable: bool,
    out: &mut Vec<u8>,
) {
    let c = cfg();
    // Mutable (○) and immutable (◆, or @ rendered as immutable) share the
    // darker color override; other nodes preserve jj's original ANSI.
    let darken = cp == MUTABLE_CP || cp == IMMUTABLE_CP || (cp == WC_CP && is_immutable);
    if darken {
        emit_filtered_ansi(ansi, out, is_fg_color_sgr);
        out.extend_from_slice(&c.mutable_node_color);
    } else {
        out.extend_from_slice(ansi);
    }
    match pick_node_icon(cp, is_empty, is_immutable) {
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
pub fn emit_dim_graph(bytes: &[u8], out: &mut Vec<u8>, is_empty: bool, is_immutable: bool) {
    let markers = strip_markers();
    let mut i = 0;
    while i < bytes.len() {
        let ansi_start = i;
        while let Some(after) = skip_csi(bytes, i) {
            i = after;
        }
        let ansi = &bytes[ansi_start..i];

        if i >= bytes.len() {
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            break;
        }

        if bytes[i] == b' ' || bytes[i] == b'\n' {
            emit_filtered_ansi(ansi, out, is_fg_color_sgr);
            out.push(bytes[i]);
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
        if is_node_char(cp) {
            emit_node(cp, raw, ansi, is_empty, is_immutable, out);
        } else {
            emit_edge(cp, raw, ansi, out);
        }
        i += len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn line_flags_detects_immutable_marker() {
        let mut buf = b"x".to_vec();
        buf.extend_from_slice(IMMUTABLE_MARKER_BYTES);
        assert_eq!(line_flags(&buf), (false, true));
    }

    #[test]
    fn line_flags_detects_both() {
        let mut buf = Vec::new();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b" ");
        buf.extend_from_slice(IMMUTABLE_MARKER_BYTES);
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
    fn strip_immutable_marker_only() {
        let mut buf = IMMUTABLE_MARKER_BYTES.to_vec();
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
        buf.extend_from_slice(IMMUTABLE_MARKER_BYTES);
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

    fn run_emit(graph: &[u8], is_empty: bool, is_immutable: bool) -> Vec<u8> {
        let mut out = Vec::new();
        emit_dim_graph(graph, &mut out, is_empty, is_immutable);
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
    fn dim_immutable_wc_darkens_and_uses_lock() {
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
    fn dim_strips_immutable_marker() {
        assert_eq!(run_emit(IMMUTABLE_MARKER_BYTES, false, false), b"");
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
}
