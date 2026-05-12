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

mod ansi;
mod config;
mod output;
mod render;

use std::io::{self, Read, Write};

use crate::ansi::FG_RESET;
use crate::config::{cfg, Activate, Config};
use crate::output::write_output;
use crate::render::{
    emit_dim_graph, find_boundary, has_graph_char, has_node_char, is_vertical_only_line,
    line_flags, write_stripping_marker, Parsed,
};

fn main() {
    let mut cfg_obj = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bijjou: {}", e);
            std::process::exit(2);
        }
    };
    if let Err(e) = cfg_obj.apply_env() {
        eprintln!("bijjou: {}", e);
        std::process::exit(2);
    }
    if let Err(e) = cfg_obj.apply_cli(std::env::args().skip(1)) {
        eprintln!("bijjou: {}", e);
        std::process::exit(2);
    }
    config::init(cfg_obj);
    if let Err(e) = run() {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return;
        }
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn split_lines(input: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
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
    lines
}

fn strip_trailing_nl(line: &[u8]) -> (&[u8], bool) {
    if line.last() == Some(&b'\n') {
        (&line[..line.len() - 1], true)
    } else {
        (line, false)
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn run() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    let c = cfg();
    match c.activate {
        Activate::Never => {
            let mut out = io::stdout().lock();
            out.write_all(&input)?;
            out.flush()?;
            return Ok(());
        }
        Activate::Always => {}
        Activate::Auto => {
            let marker = c.activation_marker.as_bytes();
            if !contains_bytes(&input, marker) {
                let mut out = io::stdout().lock();
                out.write_all(&input)?;
                out.flush()?;
                return Ok(());
            }
        }
    }

    let lines = split_lines(&input);
    let parsed: Vec<Option<Parsed>> = lines
        .iter()
        .map(|line| find_boundary(strip_trailing_nl(line).0))
        .collect();

    let max_graph = parsed
        .iter()
        .filter_map(|p| p.as_ref().map(|p| p.graph_col))
        .max()
        .unwrap_or(0);
    let target_col = max_graph + 2;

    let mut out: Vec<u8> = Vec::with_capacity(input.len() + lines.len() * 8);
    let mut emitted_lines = 0usize;

    for (line, p) in lines.iter().zip(parsed.iter()) {
        let (body, trailing_nl) = strip_trailing_nl(line);

        if c.hide_vertical_only_lines && p.is_none() && is_vertical_only_line(body) {
            continue;
        }

        match p {
            Some(p) => {
                let (is_empty, is_immutable) = line_flags(body);
                let graph = &body[..p.graph_end];
                emit_dim_graph(graph, &mut out, is_empty, is_immutable);
                let dashed = has_node_char(graph);
                write_gap(&mut out, p, target_col, &c.dash, &c.dash_arrow, &c.dim_on, dashed)?;
                write_stripping_marker(&body[p.content_start..], &mut out);
            }
            None if has_graph_char(body) => {
                let (is_empty, is_immutable) = line_flags(body);
                emit_dim_graph(body, &mut out, is_empty, is_immutable);
            }
            None => out.write_all(body)?,
        }

        if trailing_nl {
            out.write_all(b"\n")?;
        }
        emitted_lines += 1;
    }

    write_output(&out, emitted_lines)
}

fn write_gap(
    out: &mut Vec<u8>,
    p: &Parsed,
    target_col: usize,
    dash: &str,
    dash_arrow: &str,
    dim_on: &[u8],
    dashed: bool,
) -> io::Result<()> {
    let gap = target_col - p.graph_col;

    if dashed && gap >= 3 {
        let fill = gap - 2;
        out.write_all(b" ")?;
        out.write_all(dim_on)?;
        let dash_count = if dash_arrow.is_empty() { fill } else { fill - 1 };
        for _ in 0..dash_count {
            out.write_all(dash.as_bytes())?;
        }
        if !dash_arrow.is_empty() {
            out.write_all(dash_arrow.as_bytes())?;
        }
        out.write_all(FG_RESET)?;
        out.write_all(b" ")?;
    } else {
        for _ in 0..gap {
            out.write_all(b" ")?;
        }
    }
    Ok(())
}
