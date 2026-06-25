use std::borrow::Cow;
use std::io::{self, IsTerminal, Write};

use crate::ansi::strip_sgr;
use crate::config::{cfg, color_enabled, Pager};

pub fn write_output(buf: &[u8], _line_count: usize) -> io::Result<()> {
    let filtered: Cow<[u8]> = if color_enabled() {
        Cow::Borrowed(buf)
    } else {
        Cow::Owned(strip_sgr(buf))
    };
    let mut sink = OutputSink::open();
    sink.write_all(filtered.as_ref())?;
    sink.close()
}

// Output destination: either stdout or a spawned pager child. Shared by the
// buffered path (`write_output`) and the streaming path (`stream.rs`).
pub enum OutputSink {
    Stdout(io::Stdout),
    Child {
        child: std::process::Child,
        closed: bool,
    },
}

impl OutputSink {
    // Spawn a pager when configured + a usable PAGER is set, else write to
    // stdout. Auto only pages on a TTY; Always pages whenever PAGER is set;
    // Never never pages. A failed spawn falls back to stdout.
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
