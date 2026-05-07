use std::io::{self, IsTerminal, Read, Write};

const DASH: &str = "\u{2504}"; // ┄ BOX DRAWINGS LIGHT TRIPLE DASH HORIZONTAL
const DIM_ON: &[u8] = b"\x1b[38;5;8m";
const DIM_OFF: &[u8] = b"\x1b[39m";
const EDGE_DIM_ON: &[u8] = b"\x1b[2;38;5;245m";
const EDGE_DIM_OFF: &[u8] = b"\x1b[22;39m";

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
        0x40 => Some("󰛿"), // @ → working copy
        // 0x25CB => Some(""), // ○ → regular (mutable)
        0x25C6 => Some(""), // ◆ → immutable
        0xD7 => Some(""),   // × → conflicted
        0x25CF => Some(""), // ● → alternate
        _ => None,
    }
}

// Map jj box-drawing graph chars to Unicode 16.0 Large Type Pieces
// (U+1CE1A..U+1CE50). Single-cell visual equivalents.
fn map_graph_char(cp: u32) -> Option<&'static str> {
    match cp {
        0x2500 | 0x2504 | 0x2508 => Some("\u{1CE1F}"), // ─ ┄ ┈ → 𜸟
        0x2502 => Some("\u{1CE29}"),                   // │ → 𜸩
        0x250C | 0x256D => Some("\u{1CE1A}"),          // ┌ ╭ → 𜸚
        0x2510 | 0x256E => Some("\u{1CE24}"),          // ┐ ╮ → 𜸤
        0x2514 | 0x2570 => Some("\u{1CE3E}"),          // └ ╰ → 𜸾
        0x2518 | 0x256F => Some("\u{1CE43}"),          // ┘ ╯ → 𜹃
        0x251C => Some("\u{1CE28}"),                   // ├ → 𜸨
        0x2524 => Some("\u{1CE37}"),                   // ┤ → 𜸷
        0x252C => Some("\u{1CE20}"),                   // ┬ → 𜸠
        0x2534 => Some("\u{1CE40}"),                   // ┴ → 𜹀
        0x253C => Some("\u{1CE3A}"),                   // ┼ → 𜸺
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
fn emit_dim_graph(bytes: &[u8], out: &mut Vec<u8>) {
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
        if is_node_char(cp) {
            // Preserve jj's original ANSI; swap glyph for Nerd Font icon.
            out.extend_from_slice(ansi_bytes);
            match map_node_char(cp) {
                Some(replacement) => out.extend_from_slice(replacement.as_bytes()),
                None => out.extend_from_slice(&bytes[i..i + len]),
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

        match p {
            Some(p) => {
                let graph = &body[..p.graph_end];
                let content = &body[p.content_start..];

                emit_dim_graph(graph, &mut out);

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

                out.write_all(content)?;
            }
            None => {
                emit_dim_graph(body, &mut out);
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
