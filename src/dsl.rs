use std::collections::HashMap;

use crate::ansi::{decode_utf8, skip_csi, FG_RESET};
use crate::config::cfg;

// Template AST. Authored as `%{field}` or `%{func(field)}` tokens with
// arbitrary literal text between them. The render path walks `nodes` left
// to right per commit; elastic-tab nodes pad to a column shared across
// commits so that the field's left edge lines up vertically.
#[derive(Debug, Clone)]
pub enum Node {
    Literal(Vec<u8>),
    Field(String),
    ElasticTab(String),
}

#[derive(Debug, Clone)]
pub struct Template {
    pub nodes: Vec<Node>,
}

impl Template {
    pub fn parse(src: &str) -> Result<Template, String> {
        // Pre-pass: collapse real newlines to spaces; treat the two-char
        // sequence `\n` as a real newline. Other backslash sequences pass
        // through.
        let mut prepped = String::with_capacity(src.len());
        let bytes = src.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'n' {
                prepped.push('\n');
                i += 2;
                continue;
            }
            if b == b'\n' {
                prepped.push(' ');
                i += 1;
                continue;
            }
            prepped.push(b as char);
            i += 1;
        }

        let mut nodes: Vec<Node> = Vec::new();
        let mut buf: Vec<u8> = Vec::new();
        let pb = prepped.as_bytes();
        let mut i = 0;
        while i < pb.len() {
            if pb[i] == b'%' && i + 1 < pb.len() && pb[i + 1] == b'{' {
                if !buf.is_empty() {
                    nodes.push(Node::Literal(std::mem::take(&mut buf)));
                }
                let end = pb[i + 2..].iter().position(|&b| b == b'}').ok_or_else(|| {
                    format!("template: unterminated %{{ at byte {}", i)
                })?;
                let inner = std::str::from_utf8(&pb[i + 2..i + 2 + end])
                    .map_err(|_| "template: invalid utf-8 inside %{...}".to_string())?
                    .trim();
                nodes.push(parse_expr(inner)?);
                i += 2 + end + 1;
                continue;
            }
            buf.push(pb[i]);
            i += 1;
        }
        if !buf.is_empty() {
            nodes.push(Node::Literal(buf));
        }
        Ok(Template { nodes })
    }
}

fn parse_expr(s: &str) -> Result<Node, String> {
    if let Some(open) = s.find('(') {
        if !s.ends_with(')') {
            return Err(format!("template: missing `)` in `{}`", s));
        }
        let name = s[..open].trim();
        let arg = s[open + 1..s.len() - 1].trim();
        match name {
            "elastic_tab" => Ok(Node::ElasticTab(arg.to_string())),
            other => Err(format!("template: unknown function `{}`", other)),
        }
    } else {
        Ok(Node::Field(s.to_string()))
    }
}

// Flat JSON object parser. Keys and values are JSON strings; values may
// contain raw ESC bytes (the jj template emits ANSI escapes inside the
// string literal) and the standard `\"`, `\\`, `\n` escapes for the three
// chars that would otherwise break JSON tokenization.
pub fn parse_json_oneline(bytes: &[u8]) -> Option<HashMap<String, Vec<u8>>> {
    let mut i = skip_ws(bytes, 0);
    if i >= bytes.len() || bytes[i] != b'{' {
        return None;
    }
    i += 1;
    let mut fields: HashMap<String, Vec<u8>> = HashMap::new();
    loop {
        i = skip_ws(bytes, i);
        if i < bytes.len() && bytes[i] == b'}' {
            return Some(fields);
        }
        let (key, after_key) = read_json_string(bytes, i)?;
        i = skip_ws(bytes, after_key);
        if i >= bytes.len() || bytes[i] != b':' {
            return None;
        }
        i = skip_ws(bytes, i + 1);
        let (val, after_val) = read_json_string(bytes, i)?;
        let key_str = String::from_utf8(key).ok()?;
        fields.insert(key_str, val);
        i = skip_ws(bytes, after_val);
        if i >= bytes.len() {
            return None;
        }
        match bytes[i] {
            b',' => i += 1,
            b'}' => return Some(fields),
            _ => return None,
        }
    }
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    i
}

fn read_json_string(bytes: &[u8], mut i: usize) -> Option<(Vec<u8>, usize)> {
    if i >= bytes.len() || bytes[i] != b'"' {
        return None;
    }
    i += 1;
    let mut out: Vec<u8> = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            return Some((out, i + 1));
        }
        if b == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'n' => out.push(b'\n'),
                _ => return None,
            }
            i += 2;
            continue;
        }
        out.push(b);
        i += 1;
    }
    None
}

// Count visible cells in a byte slice. CSI escapes are skipped; each
// remaining codepoint counts as one cell.
pub fn visible_width(bytes: &[u8]) -> usize {
    let mut i = 0;
    let mut w = 0;
    while i < bytes.len() {
        if let Some(after) = skip_csi(bytes, i) {
            i = after;
            continue;
        }
        let (_cp, len) = decode_utf8(bytes, i);
        w += 1;
        i += len;
    }
    w
}

// Pass-1: record the max visible width seen for each elastic-tab field
// across rows. Pass-2 will right-pad each field's value to that width so
// the column following the field (a literal space, another elastic-tab,
// etc.) lands at a fixed offset on every row.
pub fn collect_widths(
    template: &Template,
    fields: &HashMap<String, Vec<u8>>,
    _start_col: usize,
    widths: &mut HashMap<String, usize>,
) {
    for node in &template.nodes {
        if let Node::ElasticTab(name) = node {
            let vw = fields
                .get(name)
                .map(|v| visible_width(v))
                .unwrap_or(0);
            let entry = widths.entry(name.clone()).or_insert(vw);
            if vw > *entry {
                *entry = vw;
            }
        }
    }
}

// Pass-2: render the template into `out`. Each elastic-tab emits its
// value followed by enough fill cells to reach the field's max width
// from pass-1. Fill is one space when only one cell short; runs of two
// or more cells are emitted as dashes with cap glyphs at the ends.
pub fn render_row(
    template: &Template,
    fields: &HashMap<String, Vec<u8>>,
    _start_col: usize,
    widths: &HashMap<String, usize>,
    out: &mut Vec<u8>,
) {
    for node in &template.nodes {
        match node {
            Node::Literal(b) => out.extend_from_slice(b),
            Node::Field(name) => {
                if let Some(v) = fields.get(name) {
                    out.extend_from_slice(v);
                }
            }
            Node::ElasticTab(name) => {
                let value: &[u8] = fields.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
                let vw = visible_width(value);
                let target = widths.get(name).copied().unwrap_or(vw);
                out.extend_from_slice(value);
                emit_pad(target.saturating_sub(vw), out);
            }
        }
    }
}

pub fn emit_pad_public(cells: usize, out: &mut Vec<u8>) {
    emit_pad(cells, out);
}

fn emit_pad(cells: usize, out: &mut Vec<u8>) {
    if cells == 0 {
        return;
    }
    if cells == 1 {
        out.push(b' ');
        return;
    }
    let c = cfg();
    out.extend_from_slice(&c.dim_on);
    let head_cap = !c.dash_start.is_empty();
    for idx in 0..cells {
        if head_cap && idx == 0 {
            out.extend_from_slice(c.dash_start.as_bytes());
        } else if head_cap && idx + 1 == cells && !c.dash_start.is_empty() {
            // Reuse dash_start as both opening and closing cap; the
            // existing config models a single cap glyph used at both
            // ends of a run (e.g. `╶─╴`). When dash_start is set we
            // mirror it for the closing cell.
            out.extend_from_slice(closing_cap(&c.dash_start).as_bytes());
        } else {
            out.extend_from_slice(c.dash.as_bytes());
        }
    }
    out.extend_from_slice(FG_RESET);
}

// Map the opening cap glyph (default `╶`, U+2576) to the matching
// closing cap (default `╴`, U+2574). Both are half-line glyphs from the
// box-drawing block. Falls back to the input when no known pair matches,
// so a custom dash_start still gives a usable (if uncapped) closing.
fn closing_cap(open: &str) -> std::borrow::Cow<'_, str> {
    match open {
        "╶" => std::borrow::Cow::Borrowed("╴"),
        "╷" => std::borrow::Cow::Borrowed("╵"),
        other => std::borrow::Cow::Borrowed(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_template() {
        let src = " %{elastic_tab(change_id)}\n%{elastic_tab(commit_id)}\n%{elastic_tab(author)}\n%{elastic_tab(timestamp)}\n%{working_copies}\n%{bookmarks}\n%{tags}\n%{description}";
        let t = Template::parse(src).unwrap();
        let elastic_count = t.nodes.iter().filter(|n| matches!(n, Node::ElasticTab(_))).count();
        let field_count = t.nodes.iter().filter(|n| matches!(n, Node::Field(_))).count();
        assert_eq!(elastic_count, 4);
        assert_eq!(field_count, 4);
    }

    #[test]
    fn parse_literal_newline_escape() {
        // `\n` literal in source becomes a real newline in output.
        let t = Template::parse("a\\nb").unwrap();
        match &t.nodes[0] {
            Node::Literal(b) => assert_eq!(b, b"a\nb"),
            _ => panic!("expected literal"),
        }
    }

    #[test]
    fn parse_unterminated_brace_errors() {
        assert!(Template::parse("%{foo").is_err());
    }

    #[test]
    fn parse_unknown_function_errors() {
        assert!(Template::parse("%{wat(foo)}").is_err());
    }

    #[test]
    fn json_oneline_basic() {
        let bytes = br#"{"change_id":"abc","commit_id":"123"}"#;
        let m = parse_json_oneline(bytes).unwrap();
        assert_eq!(m.get("change_id").unwrap(), b"abc");
        assert_eq!(m.get("commit_id").unwrap(), b"123");
    }

    #[test]
    fn json_oneline_preserves_raw_esc() {
        let bytes = b"{\"k\":\"\x1b[1mhi\x1b[0m\"}";
        let m = parse_json_oneline(bytes).unwrap();
        assert_eq!(m.get("k").unwrap(), b"\x1b[1mhi\x1b[0m");
    }

    #[test]
    fn json_oneline_decodes_quote_and_backslash() {
        let bytes = br#"{"k":"a\"b\\c\nd"}"#;
        let m = parse_json_oneline(bytes).unwrap();
        assert_eq!(m.get("k").unwrap(), b"a\"b\\c\nd");
    }

    #[test]
    fn json_oneline_rejects_non_object() {
        assert!(parse_json_oneline(b"[]").is_none());
        assert!(parse_json_oneline(b"plain").is_none());
    }

    #[test]
    fn visible_width_skips_csi() {
        assert_eq!(visible_width(b"\x1b[1mhi\x1b[0m"), 2);
    }

    #[test]
    fn visible_width_multibyte() {
        assert_eq!(visible_width("○".as_bytes()), 1);
    }

    #[test]
    fn elastic_tab_right_pads_to_max_width() {
        let t = Template::parse("%{elastic_tab(change_id)} %{description}").unwrap();
        let r1: HashMap<String, Vec<u8>> = [
            ("change_id".to_string(), b"abc".to_vec()),
            ("description".to_string(), b"short".to_vec()),
        ]
        .into_iter()
        .collect();
        let r2: HashMap<String, Vec<u8>> = [
            ("change_id".to_string(), b"abcdef".to_vec()),
            ("description".to_string(), b"longer".to_vec()),
        ]
        .into_iter()
        .collect();
        let mut widths = HashMap::new();
        collect_widths(&t, &r1, 0, &mut widths);
        collect_widths(&t, &r2, 0, &mut widths);
        assert_eq!(widths.get("change_id").copied(), Some(6));

        // r1's change_id is 3 wide; pad to width 6 with 3 dash cells.
        let mut out = Vec::new();
        render_row(&t, &r1, 0, &widths, &mut out);
        let s = String::from_utf8_lossy(&out);
        assert!(s.starts_with("abc"));
        assert!(s.contains(" short"));
        assert!(s.contains("╶") || s.contains("─"), "expected dash pad: {}", s);

        // r2 is the widest row, so no pad after change_id.
        let mut out2 = Vec::new();
        render_row(&t, &r2, 0, &widths, &mut out2);
        assert_eq!(out2, b"abcdef longer");
    }

    #[test]
    fn elastic_tab_single_cell_pad_is_space() {
        let t = Template::parse("%{elastic_tab(a)} %{description}").unwrap();
        let r1: HashMap<String, Vec<u8>> = [
            ("a".to_string(), b"ab".to_vec()),
            ("description".to_string(), b"x".to_vec()),
        ]
        .into_iter()
        .collect();
        let r2: HashMap<String, Vec<u8>> = [
            ("a".to_string(), b"abc".to_vec()),
            ("description".to_string(), b"x".to_vec()),
        ]
        .into_iter()
        .collect();
        let mut widths = HashMap::new();
        collect_widths(&t, &r1, 0, &mut widths);
        collect_widths(&t, &r2, 0, &mut widths);
        assert_eq!(widths.get("a").copied(), Some(3));

        // 1-cell gap (3 - 2 = 1) → single literal space, not dashes.
        let mut out = Vec::new();
        render_row(&t, &r1, 0, &widths, &mut out);
        assert_eq!(out, b"ab  x");
    }
}
