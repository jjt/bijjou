// 2026-05-08 Notes on combinations of revision state:
//
// Forbidden pairs:
//   - hidden + working copy — WC always visible (it @, reachable)
//   - hidden + divergent — divergent need ≥2 visible commits same change_id
//
//   All other combos legal. So:
//
//   - conflicted, empty, working copy — orthogonal. Mix any subset freely.
//   - hidden — combine w/ conflicted, empty only.
//   - divergent — combine w/ conflicted, empty, working copy (one of divergent pair can be @).
//
//   Examples valid:
//   - empty working copy (default jj new)
//   - conflicted working copy (rebase conflict in @)
//   - divergent conflicted empty working copy (all four except hidden)
//   - hidden conflicted empty (abandoned dead end)
//
//   Examples invalid:
//   - hidden working copy
//   - hidden divergent (anything)
//
//   Since C,E,WC,I are most common we use the log node:
//   - WC is the hex icon and green
//   - I is a lock icon, takes precedence over WC icon
//   - C is the color red, takes precedence over the WC color
//   - E is a hollow version of the icon

use std::io::{self, IsTerminal, Read, Write};

// const DASH: &str = "\u{2504}"; // ┄ BOX DRAWINGS LIGHT TRIPLE DASH HORIZONTAL
const DASH: &str = "━"; // ┄ BOX DRAWINGS LIGHT TRIPLE DASH HORIZONTAL
const DIM_ON: &[u8] = b"\x1b[38;5;8m";
const DIM_OFF: &[u8] = b"\x1b[39m";
const EDGE_DIM_ON: &[u8] = b"\x1b[38;5;240m";
const EDGE_DIM_OFF: &[u8] = b"\x1b[39m";
const MUTABLE_NODE_COLOR: &[u8] = b"\x1b[38;5;245m";
const MUTABLE_NODE_OFF: &[u8] = b"\x1b[39m";
const EMPTY_ICON: &str = "";
const WC_EMPTY_ICON: &str = "";
const EMPTY_IMMUTABLE_ICON: &str = "";
const WC_ICON: &str = "󰋘";
const MUTABLE_ICON: &str = "";
const IMMUTABLE_ICON: &str = "";
const CONFLICT_ICON: &str = "";
const ALTERNATE_ICON: &str = "";
const EMPTY_MARKER: u32 = 0x1D640; // 𝙀
const EMPTY_MARKER_BYTES: &[u8] = b"\xf0\x9d\x99\x80";
const IMMUTABLE_MARKER: u32 = 0x1D644; // 𝙄
const IMMUTABLE_MARKER_BYTES: &[u8] = b"\xf0\x9d\x99\x84";

struct Parsed {
    graph_end: usize,
    content_start: usize,
    graph_col: usize,
}

fn skip_csi(bytes: &[u8], i: usize) -> Option<usize> {
    if i + 1 >= bytes.len() || bytes[i] != 0x1b || bytes[i + 1] != b'[' {
        return None;
    }
    let mut j = i + 2;
    while j < bytes.len() {
        let b = bytes[j];
        j += 1;
        if (0x40..=0x7e).contains(&b) {
            return Some(j);
        }
    }
    Some(j)
}

fn decode_utf8(bytes: &[u8], i: usize) -> (u32, usize) {
    let b = bytes[i];
    if b < 0x80 {
        return (b as u32, 1);
    }
    if b < 0xc0 {
        return (b as u32, 1);
    }
    if b < 0xe0 && i + 1 < bytes.len() {
        let cp = ((b & 0x1f) as u32) << 6 | (bytes[i + 1] & 0x3f) as u32;
        return (cp, 2);
    }
    if b < 0xf0 && i + 2 < bytes.len() {
        let cp = ((b & 0x0f) as u32) << 12
            | ((bytes[i + 1] & 0x3f) as u32) << 6
            | (bytes[i + 2] & 0x3f) as u32;
        return (cp, 3);
    }
    if i + 3 < bytes.len() {
        let cp = ((b & 0x07) as u32) << 18
            | ((bytes[i + 1] & 0x3f) as u32) << 12
            | ((bytes[i + 2] & 0x3f) as u32) << 6
            | (bytes[i + 3] & 0x3f) as u32;
        return (cp, 4);
    }
    (b as u32, 1)
}

fn is_graph_char(cp: u32) -> bool {
    matches!(
        cp,
        0x2500
            ..=0x257F      // box drawing block
        | 0x40               // @
        | 0x7E               // ~
        | 0xD7               // ×
        | 0x25CB             // ○
        | 0x25CF             // ●
        | 0x25C6 // ◆
    )
}

fn find_boundary(line: &[u8]) -> Option<Parsed> {
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

fn is_node_char(cp: u32) -> bool {
    matches!(cp, 0x40 | 0x25CB | 0x25CF | 0x25C6 | 0xD7)
}

// Replace jj's commit-node glyphs with Nerd Font icons.
fn map_node_char(cp: u32) -> Option<&'static str> {
    match cp {
        0x40 => Some(WC_ICON),
        0x25CB => Some(MUTABLE_ICON),
        0x25C6 => Some(IMMUTABLE_ICON),
        0xD7 => Some(CONFLICT_ICON),
        0x25CF => Some(ALTERNATE_ICON),
        _ => None,
    }
}

// Map jj box-drawing graph chars to Unicode 16.0 Large Type Pieces
// (U+1CE1A..U+1CE50). Single-cell visual equivalents.
fn map_graph_char(cp: u32) -> Option<&'static str> {
    match char::from_u32(cp)? {
        '─' | '┄' | '┈' => Some("𜸟"),
        '│' => Some("𜸩"),
        '┌' | '╭' => Some("𜸚"),
        '┐' | '╮' => Some("𜸤"),
        '└' | '╰' => Some("𜸾"),
        '┘' | '╯' => Some("𜹃"),
        '├' => Some("𜸨"),
        '┤' => Some("𜸶"),
        '┬' => Some("𜸠"),
        '┴' => Some("𜹀"),
        '┼' => Some("𜸺"),
        '~' => Some("⌇"),
        _ => None,
    }
}

// Emit ANSI sequences from `bytes`, optionally filtering params.
// `filter` returns true for params we should DROP.
fn emit_filtered_ansi(bytes: &[u8], out: &mut Vec<u8>, filter: impl Fn(&str) -> bool) {
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = skip_csi(bytes, i) {
            if bytes[i] == 0x1b
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'['
                && end > 0
                && bytes[end - 1] == b'm'
            {
                let params = std::str::from_utf8(&bytes[i + 2..end - 1]).unwrap_or("");
                if !filter(params) {
                    out.extend_from_slice(&bytes[i..end]);
                }
            } else {
                out.extend_from_slice(&bytes[i..end]);
            }
            i = end;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
}

// Emit bytes with all visible non-space chars wrapped in dim SGR, except
// commit-node chars (○ ● ◆ @ ×) which pass through with normal intensity.
// Strips jj's fg-color codes; preserves other ANSI sequences.
fn emit_dim_graph(bytes: &[u8], out: &mut Vec<u8>, is_empty: bool, is_immutable: bool) {
    let mut i = 0;
    while i < bytes.len() {
        let ansi_start = i;
        while let Some(after) = skip_csi(bytes, i) {
            i = after;
        }
        let ansi_bytes = &bytes[ansi_start..i];

        if i >= bytes.len() {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            break;
        }

        if bytes[i] == b' ' || bytes[i] == b'\n' {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            out.push(bytes[i]);
            i += 1;
            continue;
        }

        let (cp, len) = decode_utf8(bytes, i);
        if cp == EMPTY_MARKER || cp == IMMUTABLE_MARKER {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            i += len;
            continue;
        }
        if is_node_char(cp) {
            // Mutable (○) and immutable (◆, or @ rendered as immutable) share
            // the darker color override; other nodes preserve jj's original ANSI.
            let darken = cp == 0x25CB || cp == 0x25C6 || (cp == 0x40 && is_immutable);
            if darken {
                emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
                out.extend_from_slice(MUTABLE_NODE_COLOR);
            } else {
                out.extend_from_slice(ansi_bytes);
            }
            if is_empty {
                let icon = match cp {
                    0x40 if is_immutable => EMPTY_IMMUTABLE_ICON,
                    0x40 => WC_EMPTY_ICON,
                    0x25C6 => EMPTY_IMMUTABLE_ICON,
                    _ => EMPTY_ICON,
                };
                out.extend_from_slice(icon.as_bytes());
            } else if cp == 0x40 && is_immutable {
                out.extend_from_slice(IMMUTABLE_ICON.as_bytes());
            } else {
                match map_node_char(cp) {
                    Some(replacement) => out.extend_from_slice(replacement.as_bytes()),
                    None => out.extend_from_slice(&bytes[i..i + len]),
                }
            }
            if darken {
                out.extend_from_slice(MUTABLE_NODE_OFF);
            }
        } else {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            out.extend_from_slice(EDGE_DIM_ON);
            match map_graph_char(cp) {
                Some(replacement) => out.extend_from_slice(replacement.as_bytes()),
                None => out.extend_from_slice(&bytes[i..i + len]),
            }
            out.extend_from_slice(EDGE_DIM_OFF);
        }
        i += len;
    }
}

fn line_flags(body: &[u8]) -> (bool, bool) {
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

fn write_stripping_marker(content: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < content.len() {
        if content[i..].starts_with(EMPTY_MARKER_BYTES) {
            i += EMPTY_MARKER_BYTES.len();
            continue;
        }
        if content[i..].starts_with(IMMUTABLE_MARKER_BYTES) {
            i += IMMUTABLE_MARKER_BYTES.len();
            continue;
        }
        out.push(content[i]);
        i += 1;
    }
}

fn is_fg_color_sgr(params: &str) -> bool {
    let parts: Vec<&str> = params.split(';').collect();
    match parts
        .first()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(999)
    {
        30..=37 | 39 | 90..=97 => true,
        38 => parts
            .get(1)
            .map(|p| *p == "5" || *p == "2")
            .unwrap_or(false),
        _ => false,
    }
}

fn main() {
    if let Err(e) = run() {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return;
        }
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    let mut lines: Vec<&[u8]> = Vec::new();
    let mut start = 0;
    for (i, &b) in input.iter().enumerate() {
        if b == b'\n' {
            lines.push(&input[start..=i]);
            start = i + 1;
        }
    }
    if start < input.len() {
        lines.push(&input[start..]);
    }

    let parsed: Vec<Option<Parsed>> = lines
        .iter()
        .map(|line| {
            let trimmed = if line.last() == Some(&b'\n') {
                &line[..line.len() - 1]
            } else {
                *line
            };
            find_boundary(trimmed)
        })
        .collect();

    let max_graph = parsed
        .iter()
        .filter_map(|p| p.as_ref().map(|p| p.graph_col))
        .max()
        .unwrap_or(0);
    let target_col = max_graph + 2;

    let mut out: Vec<u8> = Vec::with_capacity(input.len() + lines.len() * 8);

    for (line, p) in lines.iter().zip(parsed.iter()) {
        let trailing_nl = line.last() == Some(&b'\n');
        let body = if trailing_nl {
            &line[..line.len() - 1]
        } else {
            *line
        };

        let (is_empty, is_immutable) = line_flags(body);
        match p {
            Some(p) => {
                let graph = &body[..p.graph_end];
                let content = &body[p.content_start..];

                emit_dim_graph(graph, &mut out, is_empty, is_immutable);

                let gap = target_col - p.graph_col;
                let mut peek = p.content_start;
                while let Some(after) = skip_csi(body, peek) {
                    peek = after;
                }
                let first_byte = body.get(peek).copied();
                let has_change_id = first_byte
                    .map(|b| b.is_ascii_alphanumeric())
                    .unwrap_or(false);
                if gap >= 3 && has_change_id {
                    out.write_all(b" ")?;
                    out.write_all(DIM_ON)?;
                    for _ in 0..(gap - 2) {
                        out.write_all(DASH.as_bytes())?;
                    }
                    out.write_all(DIM_OFF)?;
                    out.write_all(b" ")?;
                } else {
                    for _ in 0..gap {
                        out.write_all(b" ")?;
                    }
                }

                write_stripping_marker(content, &mut out);
            }
            None => {
                emit_dim_graph(body, &mut out, is_empty, is_immutable);
            }
        }

        if trailing_nl {
            out.write_all(b"\n")?;
        }
    }

    write_output(&out, lines.len())
}

fn terminal_height() -> Option<u16> {
    use core::ffi::c_ulong;
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    extern "C" {
        fn ioctl(fd: i32, request: c_ulong, ...) -> i32;
    }

    #[cfg(target_os = "macos")]
    const TIOCGWINSZ: c_ulong = 0x40087468;
    #[cfg(target_os = "linux")]
    const TIOCGWINSZ: c_ulong = 0x5413;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return None;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut ws: Winsize = unsafe { std::mem::zeroed() };
        let fd = io::stdout().as_raw_fd();
        let r = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut Winsize) };
        if r == 0 && ws.ws_row > 0 {
            Some(ws.ws_row)
        } else {
            None
        }
    }
}

fn write_output(buf: &[u8], line_count: usize) -> io::Result<()> {
    let stdout = io::stdout();
    let is_tty = stdout.is_terminal();
    let height = terminal_height();
    let should_page = is_tty && height.map_or(false, |h| line_count > h as usize);

    if should_page {
        if let Some(()) = try_pager(buf)? {
            return Ok(());
        }
    }

    let mut out = stdout.lock();
    out.write_all(buf)?;
    out.flush()?;
    Ok(())
}

fn try_pager(buf: &[u8]) -> io::Result<Option<()>> {
    use std::process::{Command, Stdio};

    let pager_env = std::env::var("PAGER").ok().filter(|s| !s.trim().is_empty());
    let Some(s) = pager_env else {
        return Ok(None);
    };
    let mut parts = s.split_whitespace().map(|s| s.to_string());
    let Some(cmd) = parts.next() else {
        return Ok(None);
    };
    let args: Vec<String> = parts.collect();

    let mut child = match Command::new(&cmd).args(&args).stdin(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
            Err(e) => return Err(e),
        }
    }
    let _ = child.wait();
    Ok(Some(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- skip_csi -----------------------------------------------------------

    #[test]
    fn skip_csi_basic_sgr() {
        assert_eq!(skip_csi(b"\x1b[31mfoo", 0), Some(5));
    }

    #[test]
    fn skip_csi_no_escape_returns_none() {
        assert_eq!(skip_csi(b"foo", 0), None);
    }

    #[test]
    fn skip_csi_with_multi_param_sgr() {
        assert_eq!(skip_csi(b"\x1b[38;5;245mX", 0), Some(11));
    }

    #[test]
    fn skip_csi_unterminated_returns_buffer_end() {
        assert_eq!(skip_csi(b"\x1b[38;5", 0), Some(6));
    }

    #[test]
    fn skip_csi_at_offset() {
        // Skip past the 'X', start at the CSI.
        assert_eq!(skip_csi(b"X\x1b[1m", 1), Some(5));
    }

    // --- decode_utf8 --------------------------------------------------------

    #[test]
    fn decode_utf8_ascii() {
        assert_eq!(decode_utf8(b"A", 0), (0x41, 1));
    }

    #[test]
    fn decode_utf8_two_byte() {
        // U+00A3 £
        assert_eq!(decode_utf8(b"\xc2\xa3", 0), (0xa3, 2));
    }

    #[test]
    fn decode_utf8_three_byte_circle() {
        // U+25CB ○
        assert_eq!(decode_utf8(b"\xe2\x97\x8b", 0), (0x25CB, 3));
    }

    #[test]
    fn decode_utf8_three_byte_diamond() {
        // U+25C6 ◆
        assert_eq!(decode_utf8(b"\xe2\x97\x86", 0), (0x25C6, 3));
    }

    #[test]
    fn decode_utf8_four_byte_empty_marker() {
        assert_eq!(decode_utf8(EMPTY_MARKER_BYTES, 0), (EMPTY_MARKER, 4));
    }

    #[test]
    fn decode_utf8_four_byte_immutable_marker() {
        assert_eq!(decode_utf8(IMMUTABLE_MARKER_BYTES, 0), (IMMUTABLE_MARKER, 4));
    }

    // --- char classifiers ---------------------------------------------------

    #[test]
    fn is_node_char_recognizes_all_nodes() {
        for &cp in &[0x40u32, 0x25CB, 0x25CF, 0x25C6, 0xD7] {
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
        assert!(is_graph_char(0x2500)); // ─
        assert!(is_graph_char(0x2502)); // │
        assert!(is_graph_char(0x256D)); // ╭
        assert!(is_graph_char(0x257F)); // upper bound
    }

    #[test]
    fn is_graph_char_includes_node_chars() {
        assert!(is_graph_char(0x40));
        assert!(is_graph_char(0x25CB));
        assert!(is_graph_char(0x25C6));
        assert!(is_graph_char(0xD7));
        assert!(is_graph_char(0x7E)); // ~ elided marker
    }

    #[test]
    fn is_graph_char_rejects_letters() {
        assert!(!is_graph_char(0x41));
        assert!(!is_graph_char(0x61));
    }

    // --- node/graph icon mapping --------------------------------------------

    #[test]
    fn map_node_char_covers_each_node() {
        assert_eq!(map_node_char(0x40), Some(WC_ICON));
        assert_eq!(map_node_char(0x25CB), Some(MUTABLE_ICON));
        assert_eq!(map_node_char(0x25C6), Some(IMMUTABLE_ICON));
        assert_eq!(map_node_char(0xD7), Some(CONFLICT_ICON));
        assert_eq!(map_node_char(0x25CF), Some(ALTERNATE_ICON));
    }

    #[test]
    fn map_node_char_returns_none_for_other() {
        assert_eq!(map_node_char(0x41), None);
        assert_eq!(map_node_char(0x2502), None);
    }

    #[test]
    fn map_graph_char_box_drawings() {
        assert!(map_graph_char(0x2500).is_some()); // ─
        assert!(map_graph_char(0x2502).is_some()); // │
        assert!(map_graph_char(0x256D).is_some()); // ╭
        assert!(map_graph_char(0x2570).is_some()); // ╰
        assert!(map_graph_char(0x251C).is_some()); // ├
        assert!(map_graph_char(0x253C).is_some()); // ┼
        assert!(map_graph_char(0x7E).is_some());   // ~
    }

    #[test]
    fn map_graph_char_unknown_returns_none() {
        assert!(map_graph_char(0x41).is_none());
    }

    // --- is_fg_color_sgr ----------------------------------------------------

    #[test]
    fn fg_color_basic_30s() {
        for code in 30u16..=37 {
            assert!(is_fg_color_sgr(&code.to_string()));
        }
        assert!(is_fg_color_sgr("39")); // default fg
    }

    #[test]
    fn fg_color_bright_90s() {
        for code in 90u16..=97 {
            assert!(is_fg_color_sgr(&code.to_string()));
        }
    }

    #[test]
    fn fg_color_extended_256_and_truecolor() {
        assert!(is_fg_color_sgr("38;5;245"));
        assert!(is_fg_color_sgr("38;2;255;199;83"));
    }

    #[test]
    fn fg_color_rejects_bg_and_attrs() {
        assert!(!is_fg_color_sgr("0"));      // reset
        assert!(!is_fg_color_sgr("1"));      // bold
        assert!(!is_fg_color_sgr("3"));      // italic
        assert!(!is_fg_color_sgr("40"));     // bg color
        assert!(!is_fg_color_sgr("49"));     // default bg
        assert!(!is_fg_color_sgr("48;5;1")); // 256-color bg
    }

    #[test]
    fn fg_color_handles_empty_and_garbage() {
        assert!(!is_fg_color_sgr(""));
        assert!(!is_fg_color_sgr("xyz"));
    }

    // --- line_flags ---------------------------------------------------------

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
        // CSI bytes must not be misread as content.
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

    // --- write_stripping_marker --------------------------------------------

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

    // --- emit_filtered_ansi -------------------------------------------------

    #[test]
    fn filtered_ansi_drops_fg_keeps_attrs_and_text() {
        let mut out = Vec::new();
        emit_filtered_ansi(
            b"\x1b[1m\x1b[31mhello\x1b[39m\x1b[0m",
            &mut out,
            is_fg_color_sgr,
        );
        assert_eq!(out, b"\x1b[1mhello\x1b[0m");
    }

    #[test]
    fn filtered_ansi_passthrough_plain_text() {
        let mut out = Vec::new();
        emit_filtered_ansi(b"plain", &mut out, is_fg_color_sgr);
        assert_eq!(out, b"plain");
    }

    #[test]
    fn filtered_ansi_drops_truecolor_fg() {
        let mut out = Vec::new();
        emit_filtered_ansi(
            b"\x1b[38;2;255;199;83mtext\x1b[39m",
            &mut out,
            is_fg_color_sgr,
        );
        assert_eq!(out, b"text");
    }

    // --- find_boundary ------------------------------------------------------

    #[test]
    fn boundary_single_node_then_content() {
        let line = b"\xe2\x97\x8b  abc"; // ○  abc
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
        // │ │ ○  abc — two leading │ separated by spaces, then ○, then content.
        let line = b"\xe2\x94\x82 \xe2\x94\x82 \xe2\x97\x8b  abc";
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 5);
    }

    #[test]
    fn boundary_requires_at_least_one_graph_char() {
        // Spaces with no graph char before content: returns None.
        assert!(find_boundary(b"   abc").is_none());
    }

    // --- emit_dim_graph -----------------------------------------------------

    fn run_emit(graph: &[u8], is_empty: bool, is_immutable: bool) -> Vec<u8> {
        let mut out = Vec::new();
        emit_dim_graph(graph, &mut out, is_empty, is_immutable);
        out
    }

    fn darken(body: &[u8]) -> Vec<u8> {
        let mut v = MUTABLE_NODE_COLOR.to_vec();
        v.extend_from_slice(body);
        v.extend_from_slice(MUTABLE_NODE_OFF);
        v
    }

    #[test]
    fn dim_mutable_circle_gets_darken_and_icon() {
        let out = run_emit(b"\xe2\x97\x8b", false, false);
        assert_eq!(out, darken(MUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_gets_darken_and_lock() {
        let out = run_emit(b"\xe2\x97\x86", false, true);
        assert_eq!(out, darken(IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_darkens_even_without_flag() {
        // ◆ should darken regardless of is_immutable line flag.
        let out = run_emit(b"\xe2\x97\x86", false, false);
        assert_eq!(out, darken(IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_mutable_wc_preserves_jj_color() {
        // Mutable @ keeps jj's bold green; just swaps glyph for WC_ICON.
        let input = b"\x1b[1m\x1b[38;5;2m@\x1b[0m";
        let out = run_emit(input, false, false);
        let mut expected = b"\x1b[1m\x1b[38;5;2m".to_vec();
        expected.extend_from_slice(WC_ICON.as_bytes());
        expected.extend_from_slice(b"\x1b[0m");
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_empty_wc_uses_empty_icon() {
        let out = run_emit(b"@", true, false);
        assert_eq!(out, WC_EMPTY_ICON.as_bytes());
    }

    #[test]
    fn dim_immutable_wc_darkens_and_uses_lock() {
        // @ on an immutable line renders as IMMUTABLE_ICON (lock takes precedence).
        let out = run_emit(b"@", false, true);
        assert_eq!(out, darken(IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_empty_uses_empty_immutable_icon() {
        let out = run_emit(b"\xe2\x97\x86", true, true);
        assert_eq!(out, darken(EMPTY_IMMUTABLE_ICON.as_bytes()));
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
        let out = run_emit(b"\xe2\x94\x82", false, false); // │
        let mut expected = EDGE_DIM_ON.to_vec();
        expected.extend_from_slice("𜸩".as_bytes());
        expected.extend_from_slice(EDGE_DIM_OFF);
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_spaces_passthrough() {
        assert_eq!(run_emit(b"   ", false, false), b"   ");
    }

    #[test]
    fn dim_strips_fg_color_around_mutable_node() {
        // jj's fg color must be filtered out before the darken color is applied.
        let out = run_emit(b"\x1b[38;5;14m\xe2\x97\x8b\x1b[39m", false, false);
        // No leading [38;5;14m; trailing [39m preserved (not stripped, it's default-fg reset
        // but is_fg_color_sgr flags it as fg → also dropped).
        assert_eq!(out, darken(MUTABLE_ICON.as_bytes()));
    }
}
