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

// A render segment is one chunk of output. `Content` is opaque bytes (a
// tag value, or non-space literal text from the template) that must pass
// through unchanged. `Ws` is touchable whitespace (literal spaces from
// the template, elastic-tab pad cells, or the leading graph→content gap)
// that the rules in `render_row` may strip or fill with dashes.
#[derive(Debug, Clone)]
enum Seg {
    Content(Vec<u8>),
    Ws(usize),
    EmptyTag,
}

// Render one row using the four-rule model documented in
// `config.default.toml`:
//   1. Leading whitespace before the first non-whitespace character is
//      preserved verbatim.
//   2. When a %{} block emits empty bytes, every whitespace cell between
//      that block and the nearest non-whitespace character to its left
//      collapses to zero.
//   3. After steps 1-2 and the elastic-tab column alignment have been
//      applied, any run of consecutive whitespace cells is filled with
//      dashes (single cells stay as spaces; runs of two or more become a
//      capped dash run).
//   4. Bytes that came out of a %{} block (a Field or ElasticTab value)
//      are never modified — internal whitespace inside a value passes
//      through untouched.
// `leading_pad` is prepended as a Ws segment so the graph→content gap
// emitted by `emit_classified` participates in steps 2-3 alongside the
// template's own whitespace.
pub fn render_row(
    template: &Template,
    fields: &HashMap<String, Vec<u8>>,
    leading_pad: usize,
    leading_left: LeftSide,
    widths: &HashMap<String, usize>,
    out: &mut Vec<u8>,
) {
    let mut segs: Vec<Seg> = Vec::new();
    if leading_pad > 0 {
        segs.push(Seg::Ws(leading_pad));
    }
    for node in &template.nodes {
        match node {
            Node::Literal(b) => push_literal_segs(b, &mut segs),
            Node::Field(name) => {
                let value = fields.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
                if value.is_empty() {
                    segs.push(Seg::EmptyTag);
                } else {
                    segs.push(Seg::Content(value.to_vec()));
                }
            }
            Node::ElasticTab(name) => {
                let value = fields.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
                let vw = visible_width(value);
                let target = widths.get(name).copied().unwrap_or(vw);
                let pad = target.saturating_sub(vw);
                if value.is_empty() {
                    segs.push(Seg::EmptyTag);
                } else {
                    segs.push(Seg::Content(value.to_vec()));
                }
                if pad > 0 {
                    segs.push(Seg::Ws(pad));
                }
            }
        }
    }
    apply_rule_2(&mut segs);
    emit_segs(&segs, leading_left, out);
}

// Split a Literal node into alternating Ws / Content segments based on
// runs of ASCII space. Multi-byte UTF-8 sequences are not space chars, so
// they go into Content runs.
fn push_literal_segs(bytes: &[u8], segs: &mut Vec<Seg>) {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            let start = i;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            segs.push(Seg::Ws(i - start));
        } else {
            let start = i;
            while i < bytes.len() && bytes[i] != b' ' {
                i += 1;
            }
            segs.push(Seg::Content(bytes[start..i].to_vec()));
        }
    }
}

// Rule 2: for each `EmptyTag`, walk left and drop every preceding `Ws`
// segment until reaching the first `Content`. EmptyTag segments are
// transparent for the walk (they represent zero-width tags). If no
// Content lies to the left, rule 1 wins and nothing is stripped.
fn apply_rule_2(segs: &mut Vec<Seg>) {
    let mut i = 0;
    while i < segs.len() {
        if !matches!(segs[i], Seg::EmptyTag) {
            i += 1;
            continue;
        }
        let has_anchor_left = segs[..i].iter().any(|s| matches!(s, Seg::Content(_)));
        if has_anchor_left {
            // Pop trailing Ws and EmptyTag entries leftward from i, then
            // drop the EmptyTag itself.
            let mut j = i;
            while j > 0 {
                match &segs[j - 1] {
                    Seg::Ws(_) => {
                        segs.remove(j - 1);
                        j -= 1;
                        i -= 1;
                    }
                    Seg::EmptyTag => {
                        j -= 1;
                    }
                    Seg::Content(_) => break,
                }
            }
        }
        // Drop the EmptyTag marker — it carried no bytes anyway.
        segs.remove(i);
    }
}

// What sits immediately to the left of a Ws run. Drives whether the run's
// left end emits a `╶` cap (next to a node or to interior content),
// a plain `─` (next to a graph edge — caps never face edges), or simply
// a space (the run is only one cell wide with no graph context).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftSide {
    GraphNode,
    GraphEdge,
    Content,
}

// Walk segments, combining adjacent Ws into single dash-fill calls so
// rule 3 (consecutive whitespace becomes dashes) applies across literal,
// pad, and graph-gap cells uniformly.
//
// `leading_left` describes the prefix sitting to the left of the first
// Ws (before any Content has been emitted). Once Content appears, every
// subsequent Ws sees Content on its left.
fn emit_segs(segs: &[Seg], leading_left: LeftSide, out: &mut Vec<u8>) {
    let mut i = 0;
    let mut content_emitted = false;
    while i < segs.len() {
        match &segs[i] {
            Seg::Content(bytes) => {
                out.extend_from_slice(bytes);
                content_emitted = true;
                i += 1;
            }
            Seg::Ws(_) => {
                let mut total = 0;
                while i < segs.len() {
                    if let Seg::Ws(n) = &segs[i] {
                        total += n;
                        i += 1;
                    } else {
                        break;
                    }
                }
                let left = if content_emitted {
                    LeftSide::Content
                } else {
                    leading_left
                };
                emit_pad(total, left, out);
            }
            Seg::EmptyTag => {
                // Rule 2 should have removed these; treat any survivor as
                // zero-width and skip.
                i += 1;
            }
        }
    }
}

pub fn emit_pad_public(cells: usize, out: &mut Vec<u8>) {
    emit_pad(cells, LeftSide::GraphNode, out);
}

// Decides which glyphs fill the run based on what sits to the left:
//   - GraphNode: run starts with `dash_start` (cell right of the node).
//     Single cells stay as a literal space — a lone `dash_start` next to
//     a node reads as visual noise.
//   - GraphEdge: the left end is suppressed — dashes never overwrite or
//     abut directly onto a graph edge glyph. Single cells emit `dash_end`
//     so the run still terminates against the content on the right.
//   - Content: behaves like GraphNode — opening with `dash_start`, single
//     cells stay as a literal space.
// The right end of every multi-cell run is `dash_end` (cell left of the
// content the run terminates against).
fn emit_pad(cells: usize, left: LeftSide, out: &mut Vec<u8>) {
    if cells == 0 {
        return;
    }
    let c = cfg();
    let caps_enabled = !c.dash_start.is_empty();
    if cells == 1 {
        if !caps_enabled {
            out.push(b' ');
            return;
        }
        match left {
            LeftSide::GraphNode => out.push(b' '),
            LeftSide::GraphEdge => {
                out.extend_from_slice(&c.dim_on);
                out.extend_from_slice(c.dash_end.as_bytes());
                out.extend_from_slice(FG_RESET);
            }
            LeftSide::Content => out.push(b' '),
        }
        return;
    }
    out.extend_from_slice(&c.dim_on);
    let suppress_left_cap = matches!(left, LeftSide::GraphEdge);
    let opening_cap = caps_enabled && !suppress_left_cap;
    for idx in 0..cells {
        if opening_cap && idx == 0 {
            out.extend_from_slice(c.dash_start.as_bytes());
        } else if caps_enabled && idx + 1 == cells {
            out.extend_from_slice(c.dash_end.as_bytes());
        } else {
            out.extend_from_slice(c.dash.as_bytes());
        }
    }
    out.extend_from_slice(FG_RESET);
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
    fn elastic_tab_pad_combines_with_literal_space() {
        // Rule 3: pad (3 cells) + literal " " (1 cell) = 4 ws cells, all
        // dashed together. r1 (narrow change_id) gets dashes; r2 (widest)
        // has no pad, so only the literal space contributes a single cell
        // and stays as a space.
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

        let mut out = Vec::new();
        render_row(&t, &r1, 0, LeftSide::Content, &widths, &mut out);
        let s = String::from_utf8_lossy(&out);
        assert!(s.starts_with("abc"));
        assert!(s.ends_with("short"));
        assert!(s.contains("╶") || s.contains("─"), "expected dash pad: {}", s);

        let mut out2 = Vec::new();
        render_row(&t, &r2, 0, LeftSide::Content, &widths, &mut out2);
        assert_eq!(out2, b"abcdef longer");
    }

    #[test]
    fn empty_field_collapses_preceding_ws() {
        // Rule 2: when %{labels} is empty, the literal " " between
        // %{change_id} and %{labels} is stripped, and %{description}
        // ends up sitting directly after its own preceding literal space.
        let t = Template::parse("%{change_id} %{labels} %{description}").unwrap();
        let fields: HashMap<String, Vec<u8>> = [
            ("change_id".to_string(), b"abc".to_vec()),
            ("labels".to_string(), b"".to_vec()),
            ("description".to_string(), b"hi".to_vec()),
        ]
        .into_iter()
        .collect();
        let mut out = Vec::new();
        render_row(&t, &fields, 0, LeftSide::Content, &HashMap::new(), &mut out);
        assert_eq!(out, b"abc hi");
    }

    #[test]
    fn leading_template_ws_is_preserved() {
        // Rule 1: leading whitespace before the first non-ws content is
        // preserved verbatim.
        let t = Template::parse(" %{description}").unwrap();
        let fields: HashMap<String, Vec<u8>> = [("description".to_string(), b"hi".to_vec())]
            .into_iter()
            .collect();
        let mut out = Vec::new();
        render_row(&t, &fields, 0, LeftSide::Content, &HashMap::new(), &mut out);
        assert_eq!(out, b" hi");
    }

    #[test]
    fn empty_first_field_keeps_leading_ws_and_collapses_right() {
        // Rule 1 protects the leading " " (no non-ws content to its
        // left). Rule 2 is strictly left-only, so the " " after the
        // empty field also survives; the two cells combine under rule 3
        // into a dash fill before the next non-ws content.
        let t = Template::parse(" %{labels} %{description}").unwrap();
        let fields: HashMap<String, Vec<u8>> = [
            ("labels".to_string(), b"".to_vec()),
            ("description".to_string(), b"hi".to_vec()),
        ]
        .into_iter()
        .collect();
        let mut out = Vec::new();
        render_row(&t, &fields, 0, LeftSide::Content, &HashMap::new(), &mut out);
        let s = String::from_utf8_lossy(&out);
        assert!(s.ends_with("hi"));
        assert!(s.contains("╶") || s.contains("─"), "expected dash fill: {}", s);
    }

    #[test]
    fn leading_pad_combines_with_template_leading_ws() {
        // graph_pad passed via `leading_pad` joins the template's own
        // leading " " into a single dash run.
        let t = Template::parse(" %{change_id}").unwrap();
        let fields: HashMap<String, Vec<u8>> = [("change_id".to_string(), b"abc".to_vec())]
            .into_iter()
            .collect();
        let mut out = Vec::new();
        render_row(&t, &fields, 2, LeftSide::GraphNode, &HashMap::new(), &mut out);
        // 2 leading_pad + 1 literal = 3 ws cells → dashes; abuts "abc".
        let s = String::from_utf8_lossy(&out);
        assert!(s.ends_with("abc"));
        assert!(s.contains("╶") || s.contains("─"), "expected dash pad: {}", s);
    }
}
