use std::collections::HashMap;
use std::io::{self, IsTerminal, Read, Write};
use std::process::Command;

const DASH: &str = "\u{2504}"; // ┄ BOX DRAWINGS LIGHT TRIPLE DASH HORIZONTAL
const DIM_ON: &[u8] = b"\x1b[38;5;8m";
const DIM_OFF: &[u8] = b"\x1b[39m";
const EDGE_DIM_ON: &[u8] = b"\x1b[2m";
const EDGE_DIM_OFF: &[u8] = b"\x1b[22m";

// 16-color palette for megamerge stacks (ANSI fg escape sequences).
// Chosen to be visually distinct on both light and dark backgrounds.
const STACK_COLORS: &[&[u8]] = &[
    b"\x1b[38;5;33m",  // blue
    b"\x1b[38;5;208m", // orange
    b"\x1b[38;5;40m",  // green
    b"\x1b[38;5;163m", // purple
    b"\x1b[38;5;196m", // red
    b"\x1b[38;5;51m",  // cyan
    b"\x1b[38;5;220m", // yellow
    b"\x1b[38;5;205m", // pink
    b"\x1b[38;5;130m", // brown
    b"\x1b[38;5;87m",  // light cyan
    b"\x1b[38;5;154m", // lime
    b"\x1b[38;5;99m",  // lavender
    b"\x1b[38;5;214m", // amber
    b"\x1b[38;5;219m", // light pink
    b"\x1b[38;5;93m",  // violet
    b"\x1b[38;5;46m",  // bright green
];
const COLOR_RESET: &[u8] = b"\x1b[39m";

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
    matches!(cp,
        0x2500..=0x257F      // box drawing block
        | 0x40               // @
        | 0x7E               // ~
        | 0xD7               // ×
        | 0x25CB             // ○
        | 0x25CF             // ●
        | 0x25C6             // ◆
    )
}

// A junction line is any graph-only line that has at least one horizontal connector
// (─, ├, ┤, ┬, ┴, ╭, ╮, ╯, ╰). Pure │ / ~ / space lines are connector lines.
fn is_pure_junction_line(bytes: &[u8]) -> bool {
    let mut i = 0;
    let mut has_horizontal = false;
    while i < bytes.len() {
        if let Some(after) = skip_csi(bytes, i) {
            i = after;
            continue;
        }
        if bytes[i] == b' ' || bytes[i] == b'\n' {
            i += 1;
            continue;
        }
        let (cp, len) = decode_utf8(bytes, i);
        match cp {
            0x2500 | 0x251C | 0x2524 | 0x252C | 0x2534
            | 0x256D | 0x256E | 0x256F | 0x2570 => {
                has_horizontal = true;
                i += len;
            }
            0x2502 | 0x7E => { i += len; } // │ or ~ — ok in junction context
            _ => return false,
        }
    }
    has_horizontal
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

// Find the commit node char (○ ● ◆ @) and its visual column in the graph portion.
fn find_node_in_graph(graph: &[u8]) -> Option<(usize, u32)> {
    let mut i = 0;
    let mut vis_col = 0usize;
    while i < graph.len() {
        if let Some(after) = skip_csi(graph, i) {
            i = after;
            continue;
        }
        if graph[i] == b' ' {
            i += 1;
            vis_col += 1;
            continue;
        }
        let (cp, len) = decode_utf8(graph, i);
        if matches!(cp, 0x40 | 0x25CB | 0x25CF | 0x25C6) {
            return Some((vis_col, cp));
        }
        i += len;
        vis_col += 1;
    }
    None
}

// Extract the visible change ID from the content area (first alphanumeric run,
// skipping embedded ANSI codes). jj colors the shortest unique prefix differently
// but both parts are alphanumeric lowercase, so we collect them all.
fn extract_change_id(content: &[u8]) -> Option<String> {
    let mut i = 0;
    let mut result = Vec::new();
    while i < content.len() {
        if let Some(after) = skip_csi(content, i) {
            // ANSI inside the change ID — keep scanning if we've started
            i = after;
            continue;
        }
        let b = content[i];
        if b == b' ' {
            if result.is_empty() {
                i += 1;
                continue; // skip leading spaces
            }
            break; // end of change ID
        }
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            result.push(b);
            i += 1;
        } else {
            break;
        }
    }
    if result.len() >= 4 {
        String::from_utf8(result).ok()
    } else {
        None
    }
}

// --- Megamerge stack discovery ---

struct StackInfo {
    entries: Vec<(String, usize)>, // (change_id_prefix, stack_index)
}

impl StackInfo {
    fn lookup(&self, displayed: &str) -> Option<usize> {
        for (stored, idx) in &self.entries {
            if stored.starts_with(displayed) || displayed.starts_with(stored.as_str()) {
                return Some(*idx);
            }
        }
        None
    }
}

fn run_jj(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("jj");
    cmd.arg("--no-pager").arg("--color=never");
    for a in args {
        cmd.arg(a);
    }
    let output = cmd.output().ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

fn query_megamerge_stacks() -> Option<StackInfo> {
    // Try both naming conventions: MS-* (user's custom prefix) and stack-*-head (skill canonical).
    let (stack_names, head_glob, wc_glob) = {
        let raw = run_jj(&["bookmark", "list", "glob:MS-*", "-T", "name ++ \"\\n\""])?;
        if !raw.trim().is_empty() {
            let names: Vec<String> = raw.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            (names, "glob:MS-*", "glob:MWC-*")
        } else {
            let raw2 = run_jj(&[
                "bookmark", "list", "glob:stack-*-head", "-T", "name ++ \"\\n\"",
            ])?;
            let names: Vec<String> = raw2.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if names.is_empty() { return None; }
            (names, "glob:stack-*-head", "glob:stack-*-wc")
        }
    };

    // MB = fork_point of all stack heads = the common ancestor just below them all
    let mb_raw = run_jj(&[
        "log", "--no-graph",
        "-r", &format!("fork_point(bookmarks({head_glob}))"),
        "-T", "change_id\n",
    ])?;
    let mb_id: String = mb_raw.trim().chars().take(20).collect();
    if mb_id.is_empty() { return None; }

    let mut entries: Vec<(String, usize)> = Vec::new();

    for (stack_idx, stack_head) in stack_names.iter().enumerate() {
        // Commits from MB (exclusive) through the stack head (inclusive)
        let revset = format!("{mb_id}::{stack_head}~{mb_id}");
        if let Some(raw) = run_jj(&["log", "--no-graph", "-r", &revset, "-T", "change_id ++ \"\\n\""]) {
            for line in raw.lines() {
                let id = line.trim();
                if !id.is_empty() {
                    entries.push((id.chars().take(20).collect(), stack_idx));
                }
            }
        }
    }

    // WC commits + descendants: match each MWC-X / stack-X-wc to its stack by name,
    // then tag MWC-X and everything descended from it with that stack's color.
    let wc_list = run_jj(&[
        "log", "--no-graph",
        "-r", &format!("bookmarks({wc_glob})"),
        "-T", "bookmarks ++ \"\\n\"",
    ]).unwrap_or_default();

    for line in wc_list.lines() {
        for bm in line.trim().split_whitespace() {
            let stack_name = extract_stack_name_from_wc_bookmark(bm);
            if stack_name.is_empty() { continue; }
            let idx = stack_names.iter().position(|sn| {
                sn.ends_with(&format!("-{stack_name}")) || sn == stack_name
            });
            let Some(i) = idx else { continue; };

            let revset = format!("bookmarks(exact:{bm})::");
            if let Some(raw) = run_jj(&[
                "log", "--no-graph",
                "-r", &revset,
                "-T", "change_id ++ \"\\n\"",
            ]) {
                for cid in raw.lines() {
                    let id = cid.trim();
                    if !id.is_empty() {
                        entries.push((id.chars().take(20).collect(), i));
                    }
                }
            }
        }
    }

    if entries.is_empty() { return None; }
    Some(StackInfo { entries })
}

fn extract_stack_name_from_wc_bookmark(bm: &str) -> &str {
    if let Some(s) = bm.strip_prefix("MWC-") { return s; }
    if let Some(s) = bm.strip_prefix("stack-").and_then(|s| s.strip_suffix("-wc")) { return s; }
    ""
}

// --- Column color state (bottom-up pass) ---

// In jj graphs, branches occupy visual columns at even positions (0, 2, 4, …).
// When viewed top-to-bottom (newest → oldest):
//   ╯ / ╰  at col C: branch C closes, cols > C shift left by 2
//   ╮ / ╭  at col C: new branch opens at C (cols >= C shifted right by 2 first)
//   ┬       at col C: same as ╮
//
// We compute col_colors from the BOTTOM UP (oldest → newest) so that │ chars on
// a line are colored by the stack they lead to below that line, not above.
// In the reversed traversal the open/close semantics flip:
//   ╯ / ╰  (was: close): NOW opens a branch going upward.
//              → shift existing cols >= C right by 2, then insert C = None
//   ╮ / ╭ / ┬ (was: open): NOW closes a branch going upward.
//              → remove C, shift existing cols > C left by 2
//   ├ (join point): col continues unchanged (no-op in reverse)
//
// Multiple closes in one junction line are processed right-to-left (descending C)
// to avoid interference; multiple opens are processed left-to-right (ascending C).
fn reverse_update_col_from_junction(
    line: &[u8],
    col_colors: &mut HashMap<usize, Option<usize>>,
) {
    // Collect all (vis_col, kind) pairs: 'O' = open (╯/╰), 'C' = close (╮/╭/┬)
    let mut actions: Vec<(usize, char)> = Vec::new();
    let mut i = 0;
    let mut vis_col = 0usize;

    while i < line.len() {
        if let Some(after) = skip_csi(line, i) {
            i = after;
            continue;
        }
        if line[i] == b' ' || line[i] == b'\n' {
            i += 1;
            vis_col += 1;
            continue;
        }
        let (cp, len) = decode_utf8(line, i);
        match cp {
            0x256F | 0x2570 => actions.push((vis_col, 'O')), // ╯ ╰
            0x256E | 0x256D | 0x252C => actions.push((vis_col, 'C')), // ╮ ╭ ┬
            _ => {}
        }
        i += len;
        vis_col += 1;
    }

    // Opens (╯/╰): process ascending — each open shifts cols to the right
    let opens: Vec<usize> = actions.iter()
        .filter(|(_, k)| *k == 'O')
        .map(|(c, _)| *c)
        .collect::<Vec<_>>();
    // When inserting at C, the position is in the ORIGINAL col space.
    // Each preceding open shifts subsequent ones right by 2, so process left-to-right
    // and accumulate offset.
    let mut shift = 0usize;
    for orig_c in opens {
        let c = orig_c + shift;
        // Shift existing cols >= c right by 2
        let shifted: HashMap<usize, Option<usize>> = col_colors
            .drain()
            .map(|(k, v)| if k >= c { (k + 2, v) } else { (k, v) })
            .collect();
        *col_colors = shifted;
        col_colors.insert(c, None);
        shift += 2;
    }

    // Closes (╮/╭/┬): process descending — each close may shift cols to the right
    let mut closes: Vec<usize> = actions.iter()
        .filter(|(_, k)| *k == 'C')
        .map(|(c, _)| *c)
        .collect();
    closes.sort_unstable_by(|a, b| b.cmp(a)); // descending
    for c in closes {
        col_colors.remove(&c);
        // Shift cols > c left by 2
        let shifted: HashMap<usize, Option<usize>> = col_colors
            .drain()
            .map(|(k, v)| if k > c { (k - 2, v) } else { (k, v) })
            .collect();
        *col_colors = shifted;
    }
}

// Forward (top-down) junction update.
// ├ at col L: marks the merge-survivor col; when a ╯ follows, L will be cleared.
// ╯/╰ at col C: remove C (shift > C left by 2), clear the most-recent ├ col.
// ╮/╭/┬ at col C: shift existing cols >= C right by 2, then insert C = None.
fn forward_update_col_from_junction(
    line: &[u8],
    col_colors: &mut HashMap<usize, Option<usize>>,
) {
    let mut i = 0;
    let mut vis_col = 0usize;
    let mut last_fork_col: Option<usize> = None; // last ├ seen to the left

    while i < line.len() {
        if let Some(after) = skip_csi(line, i) {
            i = after;
            continue;
        }
        if line[i] == b' ' || line[i] == b'\n' {
            i += 1;
            vis_col += 1;
            continue;
        }
        let (cp, len) = decode_utf8(line, i);
        match cp {
            0x251C => {
                // ├ — join/continue point; remember it for a subsequent ╯
                last_fork_col = Some(vis_col);
            }
            0x256F | 0x2570 => {
                // ╯ ╰ — branch at vis_col closes
                let c = vis_col;
                col_colors.remove(&c);
                // Shift cols right of c left by 2
                let shifted: HashMap<usize, Option<usize>> = col_colors
                    .drain()
                    .map(|(k, v)| if k > c { (k - 2, v) } else { (k, v) })
                    .collect();
                *col_colors = shifted;
                // Clear the merge-survivor col (the ├ to the left)
                if let Some(fc) = last_fork_col.take() {
                    col_colors.insert(fc, None);
                }
            }
            0x256E | 0x256D | 0x252C => {
                // ╮ ╭ ┬ — new branch opens at vis_col
                let c = vis_col;
                // Shift existing cols >= c right by 2
                let shifted: HashMap<usize, Option<usize>> = col_colors
                    .drain()
                    .map(|(k, v)| if k >= c { (k + 2, v) } else { (k, v) })
                    .collect();
                *col_colors = shifted;
                col_colors.insert(c, None);
            }
            _ => {}
        }
        i += len;
        vis_col += 1;
    }
}

// --- Graph colored emitter ---

// Emit the graph bytes with stack colors applied:
//   ○ ● ◆  at node_col → node_color_idx (the commit's own stack)
//   │      at any col   → col_colors[vis_col] (the stack passing through below)
//   @      → left as-is with original jj ANSI codes
//   everything else → pass through verbatim
fn emit_graph_colored(
    graph: &[u8],
    node_col: Option<usize>,
    node_color: Option<usize>,
    col_colors: &HashMap<usize, Option<usize>>,
    out: &mut Vec<u8>,
) {
    let mut i = 0;
    let mut vis_col = 0usize;

    while i < graph.len() {
        // Collect ANSI sequences before the next visible char
        let ansi_start = i;
        while let Some(after) = skip_csi(graph, i) {
            i = after;
        }
        let ansi_bytes = &graph[ansi_start..i];

        if i >= graph.len() {
            out.extend_from_slice(ansi_bytes);
            break;
        }

        if graph[i] == b' ' {
            out.extend_from_slice(ansi_bytes);
            out.push(b' ');
            i += 1;
            vis_col += 1;
            continue;
        }

        let (cp, len) = decode_utf8(graph, i);

        match cp {
            0x40 => {
                // @ — pass through verbatim with jj's ANSI codes
                out.extend_from_slice(ansi_bytes);
                out.extend_from_slice(&graph[i..i + len]);
            }
            0x25CB | 0x25CF | 0x25C6 => {
                let my_color = if Some(vis_col) == node_col {
                    node_color
                } else {
                    col_colors.get(&vis_col).and_then(|x| *x)
                };
                if let Some(cidx) = my_color {
                    emit_non_fg_ansi(ansi_bytes, out);
                    out.extend_from_slice(STACK_COLORS[cidx % STACK_COLORS.len()]);
                    out.extend_from_slice(&graph[i..i + len]);
                    out.extend_from_slice(COLOR_RESET);
                } else {
                    out.extend_from_slice(ansi_bytes);
                    out.extend_from_slice(&graph[i..i + len]);
                }
            }
            _ => {
                // ┄ (0x2504) on a stack commit line → node color (jj's WC alignment dashes)
                // all other graph chars (│ ─ ├ ╯ ╮ ~ ×…) → dim
                if cp == 0x2504 {
                    if let Some(cidx) = node_color {
                        emit_non_fg_ansi(ansi_bytes, out);
                        out.extend_from_slice(STACK_COLORS[cidx % STACK_COLORS.len()]);
                        out.extend_from_slice(&graph[i..i + len]);
                        out.extend_from_slice(COLOR_RESET);
                        i += len;
                        vis_col += 1;
                        continue;
                    }
                }
                emit_non_fg_ansi(ansi_bytes, out);
                out.extend_from_slice(EDGE_DIM_ON);
                out.extend_from_slice(&graph[i..i + len]);
                out.extend_from_slice(EDGE_DIM_OFF);
            }
        }

        i += len;
        vis_col += 1;
    }
}

// Emit ANSI sequences from `bytes` but skip any that set foreground color.
// This preserves bold, dim, italic, reset-all, etc., while letting our own
// fg color take effect for the graph char we're about to emit.
fn emit_non_fg_ansi(bytes: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = skip_csi(bytes, i) {
            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'['
                && end > 0 && bytes[end - 1] == b'm'
            {
                let params = std::str::from_utf8(&bytes[i + 2..end - 1]).unwrap_or("");
                if !is_fg_color_sgr(params) {
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

// Re-color the change ID unique prefix (the bold highlighted part) in the content stream.
// jj renders the unique prefix as: bold + fg_color + PREFIX_CHARS, then transitions to the
// suffix (dim grey) by emitting either [0m or a new fg color code. We replace the prefix's
// fg color with stack_color and leave everything else verbatim.
fn emit_content_with_id_color(content: &[u8], stack_color: &[u8], out: &mut Vec<u8>) {
    // state: 0=before first alphanum, 1=in prefix, 2=in suffix, 3=after change id
    let mut state: u8 = 0;
    let mut i = 0;
    let mut ansi_pending: Vec<(usize, usize)> = Vec::new(); // byte ranges in content

    while i < content.len() {
        if state == 3 {
            out.extend_from_slice(&content[i..]);
            return;
        }

        if let Some(end) = skip_csi(content, i) {
            match state {
                0 => ansi_pending.push((i, end)),
                1 => {
                    // Any fg color or reset signals end of prefix
                    if end > i + 2 && content[end - 1] == b'm' {
                        let params = std::str::from_utf8(&content[i + 2..end - 1]).unwrap_or("");
                        if params == "0" || params.is_empty() || is_fg_color_sgr(params) {
                            state = 2;
                        }
                    }
                    out.extend_from_slice(&content[i..end]);
                }
                _ => out.extend_from_slice(&content[i..end]),
            }
            i = end;
        } else {
            let b = content[i];
            match state {
                0 => {
                    if b.is_ascii_lowercase() || b.is_ascii_digit() {
                        // Flush buffered ANSI stripping fg colors, then inject stack color
                        for (s, e) in ansi_pending.drain(..) {
                            emit_non_fg_ansi(&content[s..e], out);
                        }
                        out.extend_from_slice(stack_color);
                        state = 1;
                        out.push(b);
                    } else {
                        for (s, e) in ansi_pending.drain(..) {
                            out.extend_from_slice(&content[s..e]);
                        }
                        out.push(b);
                    }
                }
                1 | 2 => {
                    if b == b' ' {
                        state = 3;
                    }
                    out.push(b);
                }
                _ => out.push(b),
            }
            i += 1;
        }
    }

    for (s, e) in ansi_pending {
        out.extend_from_slice(&content[s..e]);
    }
}

// Emit bytes with all visible non-space chars wrapped in dim SGR.
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
            emit_non_fg_ansi(ansi_bytes, out);
            break;
        }

        if bytes[i] == b' ' || bytes[i] == b'\n' {
            emit_non_fg_ansi(ansi_bytes, out);
            out.push(bytes[i]);
            i += 1;
            continue;
        }

        let (_, len) = decode_utf8(bytes, i);
        emit_non_fg_ansi(ansi_bytes, out);
        out.extend_from_slice(EDGE_DIM_ON);
        out.extend_from_slice(&bytes[i..i + len]);
        out.extend_from_slice(EDGE_DIM_OFF);
        i += len;
    }
}

fn is_fg_color_sgr(params: &str) -> bool {
    let parts: Vec<&str> = params.split(';').collect();
    match parts.first().and_then(|p| p.parse::<u16>().ok()).unwrap_or(999) {
        30..=37 | 39 | 90..=97 => true,
        38 => parts.get(1).map(|p| *p == "5" || *p == "2").unwrap_or(false),
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

    let stack_info = query_megamerge_stacks();

    // Per-line node info: (node_vis_col, stack_idx_opt, is_at_node)
    // Only set for commit lines (where parsed[i] is Some).
    let mut commit_info: Vec<Option<(usize, Option<usize>, bool)>> = vec![None; lines.len()];

    if let Some(ref si) = stack_info {
        for (i, (line, p)) in lines.iter().zip(parsed.iter()).enumerate() {
            if let Some(p) = p {
                let body = if line.last() == Some(&b'\n') { &line[..line.len()-1] } else { *line };
                let graph = &body[..p.graph_end];
                let content = &body[p.content_start..];
                if let Some((node_col, node_cp)) = find_node_in_graph(graph) {
                    let is_at = node_cp == 0x40;
                    let stack_idx = extract_change_id(content).and_then(|cid| si.lookup(&cid));
                    commit_info[i] = Some((node_col, stack_idx, is_at));
                }
            }
        }
    }

    // We need two passes to color │ connectors correctly.
    //
    // A │ at (line i, col C) belongs to the branch whose commits appear EITHER:
    //   - ABOVE line i at col C  → forward pass (top-down) propagates the color down
    //   - BELOW line i at col C  → backward pass (bottom-up) propagates the color up
    //
    // Example in a 3-stack megamerge: on the beta commit lines, the │ at col 0
    // (alpha lane) connects to alpha commits BELOW — only the backward pass sees those.
    // The │ at col 4 (gamma lane) connects to gamma commits ABOVE — only the forward pass.
    // We merge both: first non-None color wins (forward takes precedence).
    let mut col_forward: Vec<HashMap<usize, Option<usize>>> = vec![HashMap::new(); lines.len()];
    let mut col_backward: Vec<HashMap<usize, Option<usize>>> = vec![HashMap::new(); lines.len()];

    if stack_info.is_some() {
        // Forward pass (top-down)
        let mut cc: HashMap<usize, Option<usize>> = HashMap::new();
        for i in 0..lines.len() {
            let line = lines[i];
            let body = if line.last() == Some(&b'\n') { &line[..line.len()-1] } else { line };
            if let Some((node_col, stack_idx, _)) = commit_info[i] {
                cc.insert(node_col, stack_idx);
            } else if is_pure_junction_line(body) {
                forward_update_col_from_junction(body, &mut cc);
            }
            col_forward[i] = cc.clone();
        }

        // Backward pass (bottom-up)
        let mut cc: HashMap<usize, Option<usize>> = HashMap::new();
        for i in (0..lines.len()).rev() {
            let line = lines[i];
            let body = if line.last() == Some(&b'\n') { &line[..line.len()-1] } else { line };
            if let Some((node_col, stack_idx, _)) = commit_info[i] {
                cc.insert(node_col, stack_idx);
            } else if is_pure_junction_line(body) {
                reverse_update_col_from_junction(body, &mut cc);
            }
            col_backward[i] = cc.clone();
        }
    }

    // Emit phase (top-down)
    let mut out: Vec<u8> = Vec::with_capacity(input.len() + lines.len() * 8);

    for (i, (line, p)) in lines.iter().zip(parsed.iter()).enumerate() {
        let trailing_nl = line.last() == Some(&b'\n');
        let body = if trailing_nl { &line[..line.len() - 1] } else { *line };

        // Build the effective col_colors for this line by merging forward+backward.
        // For each col: use the first non-None stack index (forward takes precedence,
        // backward fills in gaps). A col with value Some(None) in forward means the
        // col is active but carries no stack — we still fall back to backward.
        let effective_col_colors: HashMap<usize, Option<usize>> = if stack_info.is_some() {
            let mut all_keys: std::collections::HashSet<usize> =
                col_forward[i].keys().copied().collect();
            all_keys.extend(col_backward[i].keys().copied());
            all_keys.iter().map(|&k| {
                let fwd = col_forward[i].get(&k).and_then(|x| *x);
                let bwd = col_backward[i].get(&k).and_then(|x| *x);
                (k, fwd.or(bwd))
            }).collect()
        } else {
            HashMap::new()
        };

        match p {
            Some(p) => {
                let graph = &body[..p.graph_end];
                let content = &body[p.content_start..];

                let (node_col, node_color, _) = commit_info[i].unwrap_or((0, None, false));

                if stack_info.is_some() {
                    emit_graph_colored(
                        graph,
                        Some(node_col),
                        node_color,
                        &effective_col_colors,
                        &mut out,
                    );
                } else {
                    out.write_all(graph)?;
                }

                // Separator: dashes or spaces
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
                    if let Some(cidx) = node_color {
                        out.extend_from_slice(STACK_COLORS[cidx % STACK_COLORS.len()]);
                        for _ in 0..(gap - 2) {
                            out.write_all(DASH.as_bytes())?;
                        }
                        out.extend_from_slice(COLOR_RESET);
                    } else {
                        out.write_all(DIM_ON)?;
                        for _ in 0..(gap - 2) {
                            out.write_all(DASH.as_bytes())?;
                        }
                        out.write_all(DIM_OFF)?;
                    }
                    out.write_all(b" ")?;
                } else {
                    for _ in 0..gap {
                        out.write_all(b" ")?;
                    }
                }

                if let Some(cidx) = node_color {
                    let color = STACK_COLORS[cidx % STACK_COLORS.len()];
                    emit_content_with_id_color(content, color, &mut out);
                } else {
                    out.write_all(content)?;
                }
            }
            None => {
                if stack_info.is_some() {
                    if is_pure_junction_line(body) {
                        emit_dim_graph(body, &mut out);
                    } else {
                        emit_graph_colored(
                            body,
                            None,
                            None,
                            &effective_col_colors,
                            &mut out,
                        );
                    }
                } else {
                    out.write_all(body)?;
                }
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
    let (cmd, args): (String, Vec<String>) = match pager_env {
        Some(s) => {
            let mut parts = s.split_whitespace().map(|s| s.to_string());
            let Some(c) = parts.next() else {
                return Ok(None);
            };
            (c, parts.collect())
        }
        None => ("less".to_string(), vec!["-R".to_string()]),
    };

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
