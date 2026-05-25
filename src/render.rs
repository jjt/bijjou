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

fn write_gap(
    out: &mut Vec<u8>,
    p: &Parsed,
    target_col: usize,
    dashed: bool,
    left_is_node: bool,
) {
    let c = cfg();
    let gap = target_col - p.graph_col;
    let margin = c.dash_margin;
    let min_for_dashes = (2 * margin + 1).max(2);

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
pub fn emit_line(
    line: &[u8],
    parsed: Option<&Parsed>,
    diff_stat: Option<(&DiffStatRow, usize, usize)>,
    target_col: usize,
    max_changeid_w: usize,
    max_author_w: usize,
    out: &mut Vec<u8>,
) -> bool {
    let (body, trailing_nl) = strip_trailing_nl(line);
    let c = cfg();

    if c.hide_vertical_only_lines && parsed.is_none() && is_vertical_only_line(body) {
        return false;
    }

    if let Some((row, max_left, max_right)) = diff_stat {
        emit_diff_stat_line(body, row, target_col, max_left, max_right, out);
        if trailing_nl {
            out.push(b'\n');
        }
        return true;
    }

    match parsed {
        Some(p) => {
            let graph = &body[..p.graph_end];
            emit_dim_graph(graph, out);
            if has_node_char(graph) {
                let tail_is_node = graph_tail_is_node(graph);
                write_gap(out, p, target_col, true, tail_is_node);
                let content = &body[p.content_start..];
                write_padded_content(content, max_changeid_w, max_author_w, out);
            } else if has_elision_char(graph) {
                write_stripping_marker_with(
                    &body[p.content_start..],
                    &[ELIDED_REVISIONS_MARKER],
                    out,
                );
            } else {
                let aligned = target_col + c.details_align_offset;
                let pad = aligned.saturating_sub(p.graph_col);
                for _ in 0..pad {
                    out.push(b' ');
                }
                write_stripping_marker(&body[p.content_start..], out);
            }
        }
        None if has_graph_char(body) => {
            emit_dim_graph(body, out);
        }
        None => out.extend_from_slice(body),
    }

    if trailing_nl {
        out.push(b'\n');
    }
    true
}

fn write_padded_content(content: &[u8], max_cid: usize, max_auth: usize, out: &mut Vec<u8>) {
    let Some(cols) = parse_content_columns(content) else {
        write_stripping_marker(content, out);
        return;
    };
    let cid_pad = max_cid.saturating_sub(cols.changeid_width);
    let auth_pad = max_auth.saturating_sub(cols.author_width);
    out.extend_from_slice(&content[..cols.cid_end]);
    emit_inter_gap(&content[cols.cid_end..cols.author_start], cid_pad, out);
    out.extend_from_slice(&content[cols.author_start..cols.author_end]);
    emit_inter_gap(&content[cols.author_end..cols.rest_start], auth_pad, out);
    write_stripping_marker(&content[cols.rest_start..], out);
}

fn emit_inter_gap(gap_bytes: &[u8], extra_pad: usize, out: &mut Vec<u8>) {
    let c = cfg();
    let visible = count_visible_spaces(gap_bytes);
    let total = visible + extra_pad;
    let margin = c.dash_margin;
    let min_for_dashes = (2 * margin + 1).max(2);
    if total >= min_for_dashes {
        let fill = total - 2 * margin;
        for _ in 0..margin {
            out.push(b' ');
        }
        out.extend_from_slice(&c.dim_on);
        let use_caps = margin == 0;
        let head_cap = use_caps && !c.dash_start.is_empty();
        let tail_cap = use_caps && !c.dash_end.is_empty();
        for idx in 0..fill {
            let is_first = idx == 0;
            let is_last = idx + 1 == fill;
            if tail_cap && is_last && fill > 1 {
                out.extend_from_slice(c.dash_end.as_bytes());
            } else if head_cap && is_first {
                out.extend_from_slice(c.dash_start.as_bytes());
            } else {
                out.extend_from_slice(c.dash.as_bytes());
            }
        }
        out.extend_from_slice(FG_RESET);
        for _ in 0..margin {
            out.push(b' ');
        }
    } else {
        for _ in 0..total {
            out.push(b' ');
        }
    }
    copy_csi_runs(gap_bytes, out);
}

fn count_visible_spaces(seg: &[u8]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < seg.len() {
        if let Some(after) = skip_csi(seg, i) {
            i = after;
            continue;
        }
        if seg[i] == b' ' {
            count += 1;
        }
        i += 1;
    }
    count
}

fn copy_csi_runs(seg: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < seg.len() {
        if let Some(after) = skip_csi(seg, i) {
            out.extend_from_slice(&seg[i..after]);
            i = after;
            continue;
        }
        i += 1;
    }
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

// Visible widths and byte offsets of the first two whitespace-separated
// tokens in commit content (typically changeid + author for
// builtin_log_oneline, or changeid + commitid for builtin_log).
pub struct ContentCols {
    pub changeid_width: usize,
    pub author_width: usize,
    pub cid_end: usize,
    pub author_start: usize,
    pub author_end: usize,
    pub rest_start: usize,
}

pub fn parse_content_columns(content: &[u8]) -> Option<ContentCols> {
    let mut i = 0;
    while let Some(after) = skip_csi(content, i) {
        i = after;
    }
    if i >= content.len() || !content[i].is_ascii_lowercase() {
        return None;
    }

    let mut cid_w = 0;
    while i < content.len() {
        if let Some(after) = skip_csi(content, i) {
            i = after;
            continue;
        }
        if content[i] == b' ' {
            break;
        }
        let (_, len) = decode_utf8(content, i);
        cid_w += 1;
        i += len;
    }
    if i >= content.len() {
        return None;
    }
    let cid_end = i;

    while i < content.len() {
        if let Some(after) = skip_csi(content, i) {
            i = after;
            continue;
        }
        if content[i] == b' ' {
            i += 1;
            continue;
        }
        break;
    }
    if i >= content.len() {
        return None;
    }
    let author_start = i;

    let mut auth_w = 0;
    while i < content.len() {
        if let Some(after) = skip_csi(content, i) {
            i = after;
            continue;
        }
        if content[i] == b' ' {
            break;
        }
        let (_, len) = decode_utf8(content, i);
        auth_w += 1;
        i += len;
    }
    if i >= content.len() {
        return None;
    }
    let author_end = i;

    while i < content.len() {
        if let Some(after) = skip_csi(content, i) {
            i = after;
            continue;
        }
        if content[i] == b' ' {
            i += 1;
            continue;
        }
        break;
    }
    let rest_start = i;

    Some(ContentCols {
        changeid_width: cid_w,
        author_width: auth_w,
        cid_end,
        author_start,
        author_end,
        rest_start,
    })
}

fn is_graph_char(cp: u32) -> bool {
    if matches!(cp, 0x2500..=0x257F | ELISION_CP) {
        return true;
    }
    is_node_char(cp)
}

// A "node" is any codepoint that a jj template might emit at the rightmost
// graph column. Includes jj's built-in node chars (back-compat when no
// template override is applied) plus every configured node icon — so bijjou
// recognizes whatever the user's template emits as part of the graph
// prefix, not as content.
fn is_node_char(cp: u32) -> bool {
    if matches!(
        cp,
        WC_CP | MUTABLE_CP | ALTERNATE_CP | IMMUTABLE_CP | CONFLICT_CP
    ) {
        return true;
    }
    let c = cfg();
    let icons = [
        &c.wc_icon,
        &c.mutable_icon,
        &c.immutable_icon,
        &c.conflict_icon,
        &c.empty_icon,
        &c.wc_empty_icon,
        &c.empty_immutable_icon,
        &c.hidden_icon,
        &c.fallback_icon,
    ];
    icons
        .iter()
        .filter_map(|s| s.chars().next())
        .any(|ch| ch as u32 == cp)
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

pub fn has_elision_char(body: &[u8]) -> bool {
    let mut i = 0;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if cp == ELISION_CP {
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

// A detail-row format `<digits><A|C|D|M|R><digits> <rest>`, after an optional
// graph prefix of vertical edges and spaces. Used for `jj log` templates that
// emit per-file diff stats: e.g. `│  1936515A0 path/to/file`.
pub struct DiffStatRow {
    pub graph_prefix_end: usize,
    pub graph_col: usize,
    pub had_vertical: bool,
    pub left_start: usize,
    pub left_digits_byte_end: usize,
    pub letter_byte_idx: usize,
    pub right_digits_byte_start: usize,
    pub right_digits_byte_end: usize,
    pub right_end: usize,
    pub rest_start: usize,
    pub letter_byte: u8,
    pub left_formatted: String,
    pub right_formatted: String,
    pub left_width: usize,
    pub right_width: usize,
}

// Compact-format a decimal number so it occupies at most 4 visible chars,
// using `k`/`m`/`b`/`t` for thousands/millions/billions/trillions in place of
// the decimal point. Examples: 12345 -> "12k3", 123456 -> "123k", 1234567 ->
// "1m23". Numbers < 10_000 are emitted verbatim. Fractional rounding is
// half-up; if the rounded result overflows the integer-digit budget we
// escalate to the next unit (e.g. 999500 -> "1m00").
pub fn format_compact(n: u64) -> String {
    if n < 10_000 {
        return n.to_string();
    }
    let candidates: [(u64, char); 4] = [
        (1_000, 'k'),
        (1_000_000, 'm'),
        (1_000_000_000, 'b'),
        (1_000_000_000_000, 't'),
    ];
    let (mut divisor, mut letter) = candidates[0];
    for &(d, l) in &candidates {
        divisor = d;
        letter = l;
        if n / d < 1000 {
            break;
        }
    }
    format_compact_with_unit(n, divisor, letter)
}

fn format_compact_with_unit(n: u64, divisor: u64, letter: char) -> String {
    let int_part = n / divisor;
    let int_digits = if int_part < 10 {
        1
    } else if int_part < 100 {
        2
    } else {
        3
    };
    let frac_digits = 3 - int_digits;
    let scale = 10u64.pow(frac_digits as u32);
    let rem = n - int_part * divisor;
    let frac = (rem * scale + divisor / 2) / divisor;

    if frac == scale {
        let new_int = int_part + 1;
        if new_int >= 1000 {
            let (next_d, next_l) = match letter {
                'k' => (1_000_000u64, 'm'),
                'm' => (1_000_000_000u64, 'b'),
                'b' => (1_000_000_000_000u64, 't'),
                _ => return format!("{}{}", new_int, letter),
            };
            return format_compact_with_unit(n, next_d, next_l);
        }
        let new_int_digits = if new_int < 10 {
            1
        } else if new_int < 100 {
            2
        } else {
            3
        };
        let new_frac_digits = 3 - new_int_digits;
        return if new_frac_digits == 0 {
            format!("{}{}", new_int, letter)
        } else {
            format!(
                "{}{}{:0width$}",
                new_int,
                letter,
                0,
                width = new_frac_digits
            )
        };
    }

    if frac_digits == 0 {
        format!("{}{}", int_part, letter)
    } else {
        format!(
            "{}{}{:0width$}",
            int_part,
            letter,
            frac,
            width = frac_digits
        )
    }
}

fn skip_all_csi(body: &[u8], mut i: usize) -> usize {
    while let Some(after) = skip_csi(body, i) {
        i = after;
    }
    i
}

pub fn parse_diff_stat(body: &[u8]) -> Option<DiffStatRow> {
    let mut i = 0;
    let mut vis_col: usize = 0;
    let mut had_vertical = false;
    let graph_prefix_end;
    let graph_col;
    loop {
        let csi_start = i;
        let csi_start_col = vis_col;
        while let Some(after) = skip_csi(body, i) {
            i = after;
        }
        if i >= body.len() {
            return None;
        }
        let b = body[i];
        if b == b' ' {
            i += 1;
            vis_col += 1;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if cp == VERTICAL_CP {
            had_vertical = true;
            i += len;
            vis_col += 1;
            continue;
        }
        if !b.is_ascii_digit() {
            return None;
        }
        graph_prefix_end = csi_start;
        graph_col = csi_start_col;
        break;
    }
    let left_start = i;
    let mut left_val: u64 = 0;
    let mut left_overflow = false;
    let mut left_n = 0usize;
    let mut left_digits_byte_end = i;
    loop {
        let j = skip_all_csi(body, i);
        if j < body.len() && body[j].is_ascii_digit() {
            let d = (body[j] - b'0') as u64;
            left_val = match left_val.checked_mul(10).and_then(|v| v.checked_add(d)) {
                Some(v) => v,
                None => {
                    left_overflow = true;
                    left_val
                }
            };
            i = j + 1;
            left_digits_byte_end = i;
            left_n += 1;
        } else {
            break;
        }
    }
    if left_n == 0 {
        return None;
    }
    let j = skip_all_csi(body, i);
    if j >= body.len() || !matches!(body[j], b'M' | b'A' | b'D' | b'R' | b'C') {
        return None;
    }
    let letter_byte = body[j];
    let letter_byte_idx = j;
    i = j + 1;
    let right_digits_byte_start = skip_all_csi(body, i);
    i = right_digits_byte_start;
    let mut right_val: u64 = 0;
    let mut right_overflow = false;
    let mut right_n = 0usize;
    let mut right_digits_byte_end = i;
    loop {
        let j = skip_all_csi(body, i);
        if j < body.len() && body[j].is_ascii_digit() {
            let d = (body[j] - b'0') as u64;
            right_val = match right_val.checked_mul(10).and_then(|v| v.checked_add(d)) {
                Some(v) => v,
                None => {
                    right_overflow = true;
                    right_val
                }
            };
            i = j + 1;
            right_digits_byte_end = i;
            right_n += 1;
        } else {
            break;
        }
    }
    if right_n == 0 {
        return None;
    }
    let j = skip_all_csi(body, i);
    if j >= body.len() || body[j] != b' ' {
        return None;
    }
    let right_end = j;
    let rest_start = j + 1;
    let left_formatted = if left_overflow {
        "9999".to_string()
    } else {
        format_compact(left_val)
    };
    let right_formatted = if right_overflow {
        "9999".to_string()
    } else {
        format_compact(right_val)
    };
    let left_width = left_formatted.chars().count();
    let right_width = right_formatted.chars().count();
    Some(DiffStatRow {
        graph_prefix_end,
        graph_col,
        had_vertical,
        left_start,
        left_digits_byte_end,
        letter_byte_idx,
        right_digits_byte_start,
        right_digits_byte_end,
        right_end,
        rest_start,
        letter_byte,
        left_formatted,
        right_formatted,
        left_width,
        right_width,
    })
}

// For each contiguous run of `Some(DiffStatRow)` entries, return the group's
// (max left_width, max right_width). Non-diff-stat lines get None. Used to
// align the letter column and the path column within each group independently,
// so one commit's stats don't shift another's.
pub fn compute_diff_stat_groups(
    rows: &[Option<DiffStatRow>],
) -> Vec<Option<(usize, usize)>> {
    let mut widths: Vec<Option<(usize, usize)>> = vec![None; rows.len()];
    let mut i = 0;
    while i < rows.len() {
        if rows[i].is_none() {
            i += 1;
            continue;
        }
        let start = i;
        let mut max_left = 0usize;
        let mut max_right = 0usize;
        while i < rows.len() {
            let Some(r) = rows[i].as_ref() else {
                break;
            };
            max_left = max_left.max(r.left_width);
            max_right = max_right.max(r.right_width);
            i += 1;
        }
        for j in start..i {
            widths[j] = Some((max_left, max_right));
        }
    }
    widths
}

fn emit_diff_stat_line(
    body: &[u8],
    row: &DiffStatRow,
    target_col: usize,
    max_left: usize,
    max_right: usize,
    out: &mut Vec<u8>,
) {
    let mut trimmed_end = row.graph_prefix_end;
    let mut trailing_spaces = 0;
    while trimmed_end > 0 && body[trimmed_end - 1] == b' ' {
        trailing_spaces += 1;
        trimmed_end -= 1;
    }
    let kept_spaces = trailing_spaces.min(1);
    let prefix_end = trimmed_end + kept_spaces;
    emit_dim_graph(&body[..prefix_end], out);

    let collapsed = trailing_spaces.saturating_sub(kept_spaces);

    let letter_target = if row.had_vertical {
        target_col + max_left
    } else {
        max_left
    };
    let pad = (letter_target + collapsed).saturating_sub(row.graph_col + row.left_width);
    for _ in 0..pad {
        out.push(b' ');
    }

    out.extend_from_slice(&body[row.graph_prefix_end..row.left_start]);
    out.extend_from_slice(row.left_formatted.as_bytes());
    out.extend_from_slice(&body[row.left_digits_byte_end..row.letter_byte_idx]);
    out.push(row.letter_byte);
    out.extend_from_slice(&body[row.letter_byte_idx + 1..row.right_digits_byte_start]);
    out.extend_from_slice(row.right_formatted.as_bytes());
    out.extend_from_slice(&body[row.right_digits_byte_end..row.right_end]);
    let right_pad = max_right.saturating_sub(row.right_width);
    for _ in 0..right_pad {
        out.push(b' ');
    }
    out.push(b' ');
    out.extend_from_slice(&body[row.rest_start..]);
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

fn strip_markers() -> Vec<&'static [u8]> {
    let c = cfg();
    let mut v: Vec<&'static [u8]> = Vec::with_capacity(4);
    let em = c.empty_marker.as_bytes();
    if !em.is_empty() {
        v.push(em);
    }
    let dm = c.divergent_marker.as_bytes();
    if !dm.is_empty() {
        v.push(dm);
    }
    let cm = c.conflict_marker.as_bytes();
    if !cm.is_empty() {
        v.push(cm);
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

pub const ELIDED_REVISIONS_MARKER: &[u8] = b"(elided revisions)";

pub fn write_stripping_marker(content: &[u8], out: &mut Vec<u8>) {
    write_stripping_marker_with(content, &[], out);
}

pub fn write_stripping_marker_with(content: &[u8], extra: &[&[u8]], out: &mut Vec<u8>) {
    let mut markers = strip_markers();
    markers.extend_from_slice(extra);
    let mut i = 0;
    let mut last_visible: Option<u8> = out.last().copied();
    while i < content.len() {
        if let Some(after) = skip_csi(content, i) {
            out.extend_from_slice(&content[i..after]);
            i = after;
            continue;
        }
        if let Some(skip) = match_marker(content, i, &markers) {
            let next = i + skip;
            // Look past any CSI runs after the marker for the next visible byte,
            // so a wrapping color sequence (e.g. `\x1b[2m(empty)\x1b[22m`) does
            // not hide the trailing space from the space-collapse check.
            let mut j = next;
            while let Some(after) = skip_csi(content, j) {
                j = after;
            }
            let prev_is_space = last_visible == Some(b' ');
            let next_is_space = content.get(j) == Some(&b' ');
            if prev_is_space && next_is_space {
                out.extend_from_slice(&content[next..j]);
                i = j + 1;
            } else {
                i = next;
            }
            continue;
        }
        out.push(content[i]);
        last_visible = Some(content[i]);
        i += 1;
    }
}

// Node bytes pass through unchanged: jj's template (or upstream emitter) is
// responsible for picking the right icon and label color. Bijjou forwards
// the bytes plus their surrounding ANSI verbatim. See `bijjou jj-config`
// for a template that wires bijjou's icon config into jj.
fn emit_node(raw: &[u8], ansi: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(ansi);
    out.extend_from_slice(raw);
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
pub fn emit_dim_graph(bytes: &[u8], out: &mut Vec<u8>) {
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
            emit_node(raw, ansi, out);
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
    use crate::ansi::{CONFLICT_MARKER_BYTES, DIVERGENT_MARKER_BYTES, EMPTY_MARKER_BYTES};
    use crate::config::{DEFAULT_EDGE_DIM_ON, DEFAULT_GRAPH_VERTICAL};

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
    fn strip_conflict_marker_only() {
        let mut buf = CONFLICT_MARKER_BYTES.to_vec();
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
    fn strip_collapses_surrounding_single_spaces() {
        let mut buf = b"260513 ".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b" no description");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"260513 no description");
    }

    #[test]
    fn strip_keeps_lone_leading_space() {
        let mut buf = b"abc ".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"xyz");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"abc xyz");
    }

    #[test]
    fn strip_keeps_lone_trailing_space() {
        let mut buf = EMPTY_MARKER_BYTES.to_vec();
        buf.extend_from_slice(b" xyz");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b" xyz");
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
    fn strip_collapses_spaces_with_csi_around_marker() {
        let mut buf = b"260513 \x1b[38;5;2m".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"\x1b[39m no description");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"260513 \x1b[38;5;2m\x1b[39mno description");
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

    fn run_emit(graph: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        emit_dim_graph(graph, &mut out);
        out
    }

    #[test]
    fn dim_node_chars_pass_through_verbatim() {
        // Bijjou no longer replaces node glyphs; jj's template owns that.
        // Each of these inputs should emerge unchanged, including the
        // surrounding ANSI on the @ case.
        assert_eq!(run_emit(b"\xe2\x97\x8b"), b"\xe2\x97\x8b"); // ○
        assert_eq!(run_emit(b"\xe2\x97\x86"), b"\xe2\x97\x86"); // ◆
        assert_eq!(run_emit(b"@"), b"@");
        let with_ansi = b"\x1b[1m\x1b[38;5;2m@\x1b[0m";
        assert_eq!(run_emit(with_ansi), with_ansi);
    }

    #[test]
    fn dim_strips_empty_marker() {
        assert_eq!(run_emit(EMPTY_MARKER_BYTES), b"");
    }

    #[test]
    fn dim_strips_divergent_marker() {
        assert_eq!(run_emit(DIVERGENT_MARKER_BYTES), b"");
    }

    #[test]
    fn dim_box_drawing_gets_edge_dim() {
        let out = run_emit(b"\xe2\x94\x82");
        let mut expected = DEFAULT_EDGE_DIM_ON.to_vec();
        expected.extend_from_slice(DEFAULT_GRAPH_VERTICAL.as_bytes());
        expected.extend_from_slice(FG_RESET);
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_spaces_passthrough() {
        assert_eq!(run_emit(b"   "), b"   ");
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
    fn parse_content_columns_basic() {
        let cols = parse_content_columns(b"lvqnlxzv thomasa88 2026 desc").unwrap();
        assert_eq!(cols.changeid_width, 8);
        assert_eq!(cols.author_width, 9);
    }

    #[test]
    fn parse_content_columns_with_slash_suffix() {
        let cols = parse_content_columns(b"qwvkvytr/0 msta 2026 desc").unwrap();
        assert_eq!(cols.changeid_width, 10);
        assert_eq!(cols.author_width, 4);
    }

    #[test]
    fn parse_content_columns_rejects_non_changeid_prefix() {
        assert!(parse_content_columns(b"(elided revisions)").is_none());
        assert!(parse_content_columns(b"123 abc").is_none());
    }

    #[test]
    fn parse_content_columns_skips_csi_around_tokens() {
        let cols = parse_content_columns(b"\x1b[31mabcd\x1b[39m \x1b[34mxy\x1b[39m rest").unwrap();
        assert_eq!(cols.changeid_width, 4);
        assert_eq!(cols.author_width, 2);
    }

    #[test]
    fn parse_content_columns_returns_none_on_single_token() {
        assert!(parse_content_columns(b"onlyone").is_none());
    }

    #[test]
    fn format_compact_user_examples() {
        assert_eq!(format_compact(123), "123");
        assert_eq!(format_compact(1234), "1234");
        assert_eq!(format_compact(9999), "9999");
        assert_eq!(format_compact(12345), "12k3");
        assert_eq!(format_compact(12367), "12k4");
        assert_eq!(format_compact(123456), "123k");
        assert_eq!(format_compact(1234567), "1m23");
        assert_eq!(format_compact(12345678), "12m3");
        assert_eq!(format_compact(12388888), "12m4");
        assert_eq!(format_compact(123456789), "123m");
        assert_eq!(format_compact(1234567890), "1b23");
        assert_eq!(format_compact(12345678901), "12b3");
    }

    #[test]
    fn format_compact_overflow_escalates_unit() {
        assert_eq!(format_compact(999_500), "1m00");
        assert_eq!(format_compact(999_999_999), "1b00");
    }

    #[test]
    fn format_compact_zero_padding() {
        assert_eq!(format_compact(10_000), "10k0");
        assert_eq!(format_compact(1_020_000), "1m02");
    }

    #[test]
    fn diff_stat_formats_large_numbers() {
        let row = parse_diff_stat("│  12345M123456 path".as_bytes()).expect("expected match");
        assert_eq!(row.left_formatted, "12k3");
        assert_eq!(row.right_formatted, "123k");
        assert_eq!(row.left_width, 4);
        assert_eq!(row.right_width, 4);
    }

    #[test]
    fn diff_stat_basic_match() {
        let row = parse_diff_stat("│  1M24523 B".as_bytes()).expect("expected match");
        assert!(row.had_vertical);
        assert_eq!(row.left_formatted, "1");
        assert_eq!(row.right_formatted, "24k5");
        assert_eq!(row.left_width, 1);
        assert_eq!(row.right_width, 4);
        assert_eq!(row.graph_col, 3);
        assert_eq!(row.letter_byte, b'M');
        assert_eq!(row.rest_start, row.right_end + 1);
    }

    #[test]
    fn diff_stat_long_left_digits() {
        let row = parse_diff_stat("│  1936515A0 long".as_bytes()).expect("expected match");
        assert_eq!(row.left_formatted, "1m94");
        assert_eq!(row.left_width, 4);
        assert_eq!(row.right_formatted, "0");
        assert_eq!(row.right_width, 1);
    }

    #[test]
    fn diff_stat_rejects_no_left_digits() {
        assert!(parse_diff_stat("│  M path".as_bytes()).is_none());
    }

    #[test]
    fn diff_stat_rejects_no_right_digits() {
        assert!(parse_diff_stat("│  1M path".as_bytes()).is_none());
    }

    #[test]
    fn diff_stat_rejects_no_trailing_space() {
        assert!(parse_diff_stat("│  1M2".as_bytes()).is_none());
    }

    #[test]
    fn diff_stat_rejects_non_status_letter() {
        assert!(parse_diff_stat("│  1X2 path".as_bytes()).is_none());
    }

    #[test]
    fn diff_stat_matches_without_graph_prefix() {
        let row = parse_diff_stat(b"1M2 path").expect("expected match");
        assert!(!row.had_vertical);
        assert_eq!(row.graph_col, 0);
    }

    #[test]
    fn diff_stat_rejects_node_prefix() {
        assert!(parse_diff_stat("○  1M2 path".as_bytes()).is_none());
    }

    #[test]
    fn diff_stat_multi_vertical_prefix() {
        let row = parse_diff_stat("│ │  1M2 path".as_bytes()).expect("expected match");
        assert!(row.had_vertical);
        assert_eq!(row.graph_col, 5);
    }

    #[test]
    fn diff_stat_csi_before_digits() {
        let row =
            parse_diff_stat(b"\xe2\x94\x82  \x1b[1m1M2 path").expect("expected match");
        assert!(row.had_vertical);
        assert_eq!(row.left_width, 1);
        assert_eq!(row.right_width, 1);
    }

    #[test]
    fn compute_diff_stat_groups_splits_on_gap() {
        let mk = |body: &str| parse_diff_stat(body.as_bytes());
        let rows = vec![
            None,
            mk("│  1M3 a"),
            mk("│  1234A1 b"),
            None,
            mk("│  1D9 c"),
            None,
        ];
        let widths = compute_diff_stat_groups(&rows);
        assert_eq!(widths[0], None);
        assert_eq!(widths[1], Some((4, 1)));
        assert_eq!(widths[2], Some((4, 1)));
        assert_eq!(widths[3], None);
        assert_eq!(widths[4], Some((1, 1)));
        assert_eq!(widths[5], None);
    }

    #[test]
    fn graph_tail_is_node_detects_trailing_node() {
        assert!(graph_tail_is_node("│ │ ○".as_bytes()));
        assert!(!graph_tail_is_node("○ │ │".as_bytes()));
        assert!(!graph_tail_is_node("├─╯".as_bytes()));
    }
}
