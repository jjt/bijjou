use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};

use crate::config::{cfg, Activate, BatchSize, Pager, DEFAULT_STREAM_BATCH_SIZE};
use crate::render::{contains_bytes, emit_line, find_boundary, parse_content_columns, strip_trailing_nl};

pub fn run() -> io::Result<()> {
    let c = cfg();
    let (first_size, rest_size) = resolve_batch_sizes(&c.stream_batch_size);
    let mut sink = OutputSink::open();
    let mut reader = BufReader::new(io::stdin().lock());

    let first = read_batch(&mut reader, first_size)?;
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
    let prescan_page = matches!(c.stream_batch_size, BatchSize::HalfPager);

    if prescan_page {
        scan_widths(&first, &mut state);
    }
    process_batch(&first, &mut state, &mut sink)?;

    loop {
        let batch = read_batch(&mut reader, rest_size)?;
        if batch.is_empty() {
            break;
        }
        if prescan_page {
            scan_widths(&batch, &mut state);
        }
        process_batch(&batch, &mut state, &mut sink)?;
    }

    sink.close()
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
            let h = terminal_height().unwrap_or(DEFAULT_STREAM_BATCH_SIZE);
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
        fn ioctl(fd: i32, req: u64, arg: *mut WinSize) -> i32;
    }

    let fds = [
        io::stderr().as_raw_fd(),
        io::stdout().as_raw_fd(),
        io::stdin().as_raw_fd(),
    ];
    for fd in fds {
        let mut ws = WinSize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
        let r = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut WinSize) };
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
    for line in batch {
        let body = strip_trailing_nl(line).0;
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
            target_col,
            state.changeid_max,
            state.author_max,
            &mut out,
        );
    }
    sink_write(sink, &out)
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
    #[cfg(unix)]
    UnixPager {
        write_fd: i32,
        child_pid: i32,
        closed: bool,
    },
    #[cfg(not(unix))]
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
            #[cfg(unix)]
            OutputSink::UnixPager { closed, .. } => *closed = true,
            #[cfg(not(unix))]
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
            #[cfg(unix)]
            OutputSink::UnixPager { write_fd, closed, .. } => {
                if *closed {
                    return Ok(());
                }
                write_fd_all(*write_fd, buf)
            }
            #[cfg(not(unix))]
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
            #[cfg(unix)]
            OutputSink::UnixPager { write_fd, child_pid, closed: _ } => {
                unsafe { libc_close(*write_fd) };
                let mut status = 0i32;
                unsafe { libc_waitpid(*child_pid, &mut status as *mut i32, 0) };
            }
            #[cfg(not(unix))]
            OutputSink::Child { child, closed: _ } => {
                drop(child.stdin.take());
                let _ = child.wait();
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn spawn_pager(cmd_line: &str) -> Option<OutputSink> {
    use std::ffi::{c_char, CString};

    let parts: Vec<String> = cmd_line.split_whitespace().map(|s| s.to_string()).collect();
    let cmd = parts.first()?;
    let c_cmd = CString::new(cmd.as_str()).ok()?;
    let c_argv: Vec<CString> = parts
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap())
        .collect();
    let mut argv: Vec<*const c_char> = c_argv.iter().map(|c| c.as_ptr()).collect();
    argv.push(std::ptr::null());

    let mut fds: [i32; 2] = [0; 2];
    if unsafe { libc_pipe(fds.as_mut_ptr()) } != 0 {
        return None;
    }
    let (rfd, wfd) = (fds[0], fds[1]);

    let _ = io::stdout().flush();
    let _ = io::stderr().flush();

    let pid = unsafe { libc_fork() };
    if pid < 0 {
        unsafe {
            libc_close(rfd);
            libc_close(wfd);
        }
        return None;
    }
    if pid == 0 {
        // Child: stdin ← pipe read end; exec pager.
        unsafe { libc_close(wfd) };
        if unsafe { libc_dup2(rfd, 0) } < 0 {
            unsafe { libc_exit(127) };
        }
        unsafe { libc_close(rfd) };
        unsafe { libc_execvp(c_cmd.as_ptr(), argv.as_ptr()) };
        unsafe { libc_exit(127) };
    }
    // Parent
    unsafe { libc_close(rfd) };
    Some(OutputSink::UnixPager {
        write_fd: wfd,
        child_pid: pid,
        closed: false,
    })
}

#[cfg(not(unix))]
fn spawn_pager(cmd_line: &str) -> Option<OutputSink> {
    use std::process::{Command, Stdio};

    let mut parts = cmd_line.split_whitespace().map(|s| s.to_string());
    let cmd = parts.next()?;
    let args: Vec<String> = parts.collect();
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

#[cfg(unix)]
fn write_fd_all(fd: i32, buf: &[u8]) -> io::Result<()> {
    let mut written = 0usize;
    while written < buf.len() {
        let n = unsafe { libc_write(fd, buf.as_ptr().add(written), buf.len() - written) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "wrote 0 bytes"));
        }
        written += n as usize;
    }
    Ok(())
}

#[cfg(unix)]
extern "C" {
    #[link_name = "pipe"]
    fn libc_pipe(fds: *mut i32) -> i32;
    #[link_name = "fork"]
    fn libc_fork() -> i32;
    #[link_name = "dup2"]
    fn libc_dup2(oldfd: i32, newfd: i32) -> i32;
    #[link_name = "close"]
    fn libc_close(fd: i32) -> i32;
    #[link_name = "_exit"]
    fn libc_exit(status: i32) -> !;
    #[link_name = "write"]
    fn libc_write(fd: i32, buf: *const u8, count: usize) -> isize;
    #[link_name = "execvp"]
    fn libc_execvp(file: *const std::ffi::c_char, argv: *const *const std::ffi::c_char) -> i32;
    #[link_name = "waitpid"]
    fn libc_waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
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
