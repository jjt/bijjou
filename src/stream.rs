use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};

use crate::ansi::strip_sgr;
use crate::config::{cfg, color_enabled, Activate, BatchSize, Pager};
use crate::render::{
    contains_bytes, emit_line, find_boundary, is_vertical_only_line, parse_content_columns,
    parse_diff_stat, strip_trailing_nl,
};

pub fn run() -> io::Result<()> {
    let c = cfg();
    let (first_size, rest_size) = resolve_batch_sizes(&c.stream_batch_size);
    let count_visible = matches!(c.stream_batch_size, BatchSize::HalfPager);
    let mut sink = OutputSink::open();
    let mut reader = BufReader::new(io::stdin().lock());

    let first = read_batch_with_mode(&mut reader, first_size, count_visible)?;
    if first.is_empty() {
        return sink.close();
    }

    if c.activate == Activate::Auto {
        let marker = c.activation_marker.as_bytes();
        let mut concatenated_len = 0usize;
        for line in &first {
            concatenated_len += line.len();
        }
        let mut joined = Vec::with_capacity(concatenated_len);
        for line in &first {
            joined.extend_from_slice(line);
        }
        if !contains_bytes(&joined, marker) {
            sink_write(&mut sink, &joined)?;
            passthrough(&mut reader, &mut sink)?;
            return sink.close();
        }
    }

    let mut state = StreamState::default();
    let per_page = matches!(c.stream_batch_size, BatchSize::HalfPager);

    // Pre-scan the first batch so its target_col is set from the whole batch's
    // widest graph_col rather than growing per-line. Subsequent batches keep
    // bumping monotonically per-line — only the first batch is uniform.
    scan_widths(&first, &mut state);
    process_batch(&first, &mut state, &mut sink)?;

    loop {
        let batch = read_batch_with_mode(&mut reader, rest_size, count_visible)?;
        if batch.is_empty() {
            break;
        }
        if per_page {
            if !c.monotonic_alignment {
                state = StreamState::default();
            }
            scan_widths(&batch, &mut state);
        }
        process_batch(&batch, &mut state, &mut sink)?;
    }

    flush_trailing_diff_stat(&mut state, &mut sink)?;
    sink.close()
}

fn read_batch_with_mode<R: BufRead>(
    reader: &mut R,
    target_visible: usize,
    count_visible: bool,
) -> io::Result<Vec<Vec<u8>>> {
    if !count_visible {
        return read_batch(reader, target_visible);
    }
    let c = cfg();
    let mut out = Vec::with_capacity(target_visible);
    let mut visible = 0usize;
    while visible < target_visible {
        let mut buf = Vec::new();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let body = strip_trailing_nl(&buf).0;
        let parsed = find_boundary(body);
        let filtered = c.hide_vertical_only_lines
            && parsed.is_none()
            && is_vertical_only_line(body);
        if !filtered {
            visible += 1;
        }
        out.push(buf);
    }
    Ok(out)
}

fn scan_widths(batch: &[Vec<u8>], state: &mut StreamState) {
    for line in batch {
        let body = strip_trailing_nl(line).0;
        if let Some(p) = find_boundary(body) {
            if p.graph_col > state.graph_max {
                state.graph_max = p.graph_col;
            }
            if let Some(cols) = parse_content_columns(&body[p.content_start..]) {
                if cols.changeid_width > state.changeid_max {
                    state.changeid_max = cols.changeid_width;
                }
                if cols.author_width > state.author_max {
                    state.author_max = cols.author_width;
                }
            }
        }
    }
}

fn resolve_batch_sizes(bs: &BatchSize) -> (usize, usize) {
    match bs {
        BatchSize::Fixed(n) => {
            let n = (*n).max(1);
            (n, n)
        }
        BatchSize::HalfPager => {
            // Fallback to a small page when terminal_height can't be detected
            // (no TTY available). DEFAULT_STREAM_BATCH_SIZE (128) is too big
            // here — half-pager only makes sense at screen-scale, and a huge
            // first batch flattens column alignment across the whole log.
            let h = cfg()
                .debug_force_screen_height
                .or_else(terminal_height)
                .unwrap_or(24);
            let first = h.saturating_sub(1).max(1);
            let rest = (first / 2).max(1);
            (first, rest)
        }
    }
}

#[cfg(unix)]
fn terminal_height() -> Option<usize> {
    use std::os::unix::io::AsRawFd;

    #[repr(C)]
    struct WinSize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    #[cfg(target_os = "macos")]
    const TIOCGWINSZ: u64 = 0x40087468;
    #[cfg(target_os = "linux")]
    const TIOCGWINSZ: u64 = 0x5413;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    const TIOCGWINSZ: u64 = 0x5413;

    extern "C" {
        // ioctl is variadic on macOS/Linux; declaring a fixed third arg
        // uses the wrong ABI on ARM64 (the pointer lands in the wrong slot
        // and ws_row stays 0). Keep it variadic to match libc.
        fn ioctl(fd: i32, req: u64, ...) -> i32;
    }

    let std_fds = [
        io::stderr().as_raw_fd(),
        io::stdout().as_raw_fd(),
        io::stdin().as_raw_fd(),
    ];
    for fd in std_fds {
        let mut ws = WinSize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
        let r = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut WinSize) };
        if r == 0 && ws.ws_row > 0 {
            return Some(ws.ws_row as usize);
        }
    }
    if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") {
        let mut ws = WinSize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
        let r = unsafe { ioctl(file.as_raw_fd(), TIOCGWINSZ, &mut ws as *mut WinSize) };
        if r == 0 && ws.ws_row > 0 {
            return Some(ws.ws_row as usize);
        }
    }
    std::env::var("LINES").ok().and_then(|s| s.parse().ok())
}

#[cfg(not(unix))]
fn terminal_height() -> Option<usize> {
    std::env::var("LINES").ok().and_then(|s| s.parse().ok())
}

#[derive(Default)]
struct StreamState {
    graph_max: usize,
    changeid_max: usize,
    author_max: usize,
    pending_diff_stat: Vec<Vec<u8>>,
    pending_max_left: usize,
    pending_max_right: usize,
}

fn read_batch<R: BufRead>(reader: &mut R, batch_size: usize) -> io::Result<Vec<Vec<u8>>> {
    let mut out = Vec::with_capacity(batch_size);
    for _ in 0..batch_size {
        let mut buf = Vec::new();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        out.push(buf);
    }
    Ok(out)
}

// Per-line monotonic widening: graph_max, changeid_max, and author_max all
// bump the moment a wider value is seen and the new target widths take
// effect on that same line. Batching is purely flush-cadence — `batch_size`
// does not affect column alignment. This intentionally diverges from
// main.rs's batch path, which applies one global max to every line.
fn process_batch(
    batch: &[Vec<u8>],
    state: &mut StreamState,
    sink: &mut OutputSink,
) -> io::Result<()> {
    let c = cfg();
    let mut out: Vec<u8> = Vec::with_capacity(batch.iter().map(|l| l.len() + 8).sum());
    for line in batch.iter() {
        let body = strip_trailing_nl(line).0;
        if let Some(row) = parse_diff_stat(body) {
            state.pending_max_left = state.pending_max_left.max(row.left_width);
            state.pending_max_right = state.pending_max_right.max(row.right_width);
            state.pending_diff_stat.push(line.clone());
            continue;
        }

        if !state.pending_diff_stat.is_empty() {
            flush_pending_diff_stat(state, &mut out, c);
        }

        let parsed = find_boundary(body);
        if let Some(p) = &parsed {
            if p.graph_col > state.graph_max {
                state.graph_max = p.graph_col;
            }
            if let Some(cols) = parse_content_columns(&body[p.content_start..]) {
                if cols.changeid_width > state.changeid_max {
                    state.changeid_max = cols.changeid_width;
                }
                if cols.author_width > state.author_max {
                    state.author_max = cols.author_width;
                }
            }
        }
        let target_col = if c.align_enabled {
            state.graph_max + c.align_gap
        } else {
            parsed.as_ref().map(|p| p.graph_col).unwrap_or(0) + c.align_gap
        };
        emit_line(
            line,
            parsed.as_ref(),
            None,
            target_col,
            state.changeid_max,
            state.author_max,
            &mut out,
        );
    }
    if color_enabled() {
        sink_write(sink, &out)
    } else {
        sink_write(sink, &strip_sgr(&out))
    }
}

// Diff-stat lines stay buffered until the group closes (next non-diff-stat
// line, or end of stream) so per-commit alignment doesn't fracture at batch
// boundaries. Called when a non-diff-stat line arrives mid-stream, and again
// from `flush_trailing_diff_stat` at EOF.
fn flush_pending_diff_stat(
    state: &mut StreamState,
    out: &mut Vec<u8>,
    c: &crate::config::Config,
) {
    let ml = state.pending_max_left;
    let mr = state.pending_max_right;
    let lines = std::mem::take(&mut state.pending_diff_stat);
    state.pending_max_left = 0;
    state.pending_max_right = 0;
    for line in lines {
        let body = strip_trailing_nl(&line).0;
        let row = match parse_diff_stat(body) {
            Some(r) => r,
            None => continue,
        };
        let parsed = find_boundary(body);
        if let Some(p) = &parsed {
            if p.graph_col > state.graph_max {
                state.graph_max = p.graph_col;
            }
        }
        let target_col = if c.align_enabled {
            state.graph_max + c.align_gap
        } else {
            parsed.as_ref().map(|p| p.graph_col).unwrap_or(0) + c.align_gap
        };
        emit_line(
            &line,
            parsed.as_ref(),
            Some((&row, ml, mr)),
            target_col,
            state.changeid_max,
            state.author_max,
            out,
        );
    }
}

fn flush_trailing_diff_stat(state: &mut StreamState, sink: &mut OutputSink) -> io::Result<()> {
    if state.pending_diff_stat.is_empty() {
        return Ok(());
    }
    let c = cfg();
    let mut out: Vec<u8> = Vec::new();
    flush_pending_diff_stat(state, &mut out, c);
    if color_enabled() {
        sink_write(sink, &out)
    } else {
        sink_write(sink, &strip_sgr(&out))
    }
}

fn passthrough<R: Read>(reader: &mut R, sink: &mut OutputSink) -> io::Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        sink_write(sink, &buf[..n])?;
    }
}

fn sink_write(sink: &mut OutputSink, buf: &[u8]) -> io::Result<()> {
    match sink.write_all(buf) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
            sink.mark_closed();
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub enum OutputSink {
    Stdout(io::Stdout),
    Child {
        child: std::process::Child,
        closed: bool,
    },
}

impl OutputSink {
    pub fn open() -> Self {
        let is_tty = io::stdout().is_terminal();
        let pager_var = std::env::var("PAGER").ok().filter(|s| !s.trim().is_empty());
        let want_page = match cfg().pager {
            Pager::Never => false,
            Pager::Always => pager_var.is_some(),
            Pager::Auto => is_tty && pager_var.is_some(),
        };
        if want_page {
            if let Some(s) = pager_var {
                if let Some(sink) = spawn_pager(&s) {
                    return sink;
                }
            }
        }
        OutputSink::Stdout(io::stdout())
    }

    pub fn mark_closed(&mut self) {
        match self {
            OutputSink::Stdout(_) => {}
            OutputSink::Child { closed, .. } => *closed = true,
        }
    }

    pub fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            OutputSink::Stdout(stdout) => {
                let mut out = stdout.lock();
                out.write_all(buf)?;
                out.flush()
            }
            OutputSink::Child { child, closed } => {
                if *closed {
                    return Ok(());
                }
                let stdin = child.stdin.as_mut().expect("stdin piped");
                stdin.write_all(buf)?;
                stdin.flush()
            }
        }
    }

    pub fn close(mut self) -> io::Result<()> {
        match &mut self {
            OutputSink::Stdout(stdout) => {
                let mut out = stdout.lock();
                out.flush()?;
            }
            OutputSink::Child { child, closed: _ } => {
                drop(child.stdin.take());
                let _ = child.wait();
            }
        }
        Ok(())
    }
}

// Use std::process::Command (which calls posix_spawn on macOS) rather than
// a manual fork+exec. Calling fork() after the Rust runtime initializes the
// macOS frameworks/libdispatch can race the dispatch workqueue and get the
// process killed (SIGKILL) on Apple Silicon.
fn spawn_pager(cmd_line: &str) -> Option<OutputSink> {
    use std::process::{Command, Stdio};

    let mut parts = cmd_line.split_whitespace().map(|s| s.to_string());
    let cmd = parts.next()?;
    let args: Vec<String> = parts.collect();
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    let child = Command::new(&cmd)
        .args(&args)
        .stdin(Stdio::piped())
        .spawn()
        .ok()?;
    Some(OutputSink::Child {
        child,
        closed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_batch_returns_n_lines() {
        let input = b"a\nb\nc\nd\n";
        let mut r = Cursor::new(&input[..]);
        let batch = read_batch(&mut r, 3).unwrap();
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0], b"a\n");
        assert_eq!(batch[1], b"b\n");
        assert_eq!(batch[2], b"c\n");
    }

    #[test]
    fn read_batch_stops_at_eof() {
        let input = b"a\nb\n";
        let mut r = Cursor::new(&input[..]);
        let batch = read_batch(&mut r, 10).unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn read_batch_handles_trailing_no_newline() {
        let input = b"a\nb";
        let mut r = Cursor::new(&input[..]);
        let batch = read_batch(&mut r, 10).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0], b"a\n");
        assert_eq!(batch[1], b"b");
    }

    #[test]
    fn read_batch_empty_input_returns_empty() {
        let input: &[u8] = b"";
        let mut r = Cursor::new(input);
        let batch = read_batch(&mut r, 5).unwrap();
        assert!(batch.is_empty());
    }
}
