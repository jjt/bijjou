use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read};

use crate::ansi::strip_sgr;
use crate::config::{cfg, color_enabled, Activate, BatchSize, BIJJOU_TEMPLATE_NAME_FIELD};
use crate::dsl::collect_widths;
use crate::output::OutputSink;
use crate::{
    classify_row, compile_templates, emit_classified, CompiledTemplate, RowKind, TemplateMetrics,
};
use crate::render::{contains_bytes, strip_trailing_nl};

pub fn run() -> io::Result<()> {
    let c = cfg();
    let (first_size, rest_size) = resolve_batch_sizes(&c.stream_batch_size);
    let mut sink = OutputSink::open();
    let mut reader = BufReader::new(io::stdin().lock());
    let templates = compile_templates(&c.templates)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut metrics: HashMap<String, TemplateMetrics> = HashMap::new();
    let mut max_graph_col: usize = 0;

    let first = read_batch(&mut reader, first_size)?;
    if first.is_empty() {
        return sink.close();
    }

    if c.activate == Activate::Auto {
        let mut concatenated_len = 0usize;
        for line in &first {
            concatenated_len += line.len();
        }
        let mut joined = Vec::with_capacity(concatenated_len);
        for line in &first {
            joined.extend_from_slice(line);
        }
        if !contains_bytes(&joined, BIJJOU_TEMPLATE_NAME_FIELD.as_bytes()) {
            sink_write(&mut sink, &joined)?;
            passthrough(&mut reader, &mut sink)?;
            return sink.close();
        }
    }

    process_batch(&first, &templates, &mut metrics, &mut max_graph_col, &mut sink)?;
    loop {
        let batch = read_batch(&mut reader, rest_size)?;
        if batch.is_empty() {
            break;
        }
        process_batch(&batch, &templates, &mut metrics, &mut max_graph_col, &mut sink)?;
    }
    sink.close()
}

fn resolve_batch_sizes(bs: &BatchSize) -> (usize, usize) {
    match bs {
        BatchSize::Fixed(n) => {
            let n = (*n).max(1);
            (n, n)
        }
        BatchSize::HalfPager => {
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

fn process_batch(
    batch: &[Vec<u8>],
    templates: &HashMap<String, CompiledTemplate>,
    metrics: &mut HashMap<String, TemplateMetrics>,
    max_graph_col: &mut usize,
    sink: &mut OutputSink,
) -> io::Result<()> {
    let rows: Vec<RowKind> = batch
        .iter()
        .map(|l| classify_row(strip_trailing_nl(l).0))
        .collect();

    // Monotonic widen: widths and anchors only grow across batches so
    // already-emitted rows above remain valid (column targets never
    // shrink).
    for row in &rows {
        if let RowKind::Commit {
            graph_col,
            template_name,
            fields,
            ..
        } = row
        {
            if let Some(name) = template_name.as_deref() {
                if let Some(CompiledTemplate::Parsed(template)) = templates.get(name) {
                    let entry = metrics.entry(name.to_string()).or_default();
                    collect_widths(template, fields, &mut entry.widths, &mut entry.anchors);
                }
            }
            if *graph_col > *max_graph_col {
                *max_graph_col = *graph_col;
            }
        }
    }

    let mut out: Vec<u8> = Vec::with_capacity(batch.iter().map(|l| l.len() + 16).sum());
    for (line, row) in batch.iter().zip(rows.iter()) {
        emit_classified(line, row, templates, metrics, *max_graph_col, &mut out);
    }
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
