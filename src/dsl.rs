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

// Flat NUL/RS-framed parser. Record shape:
//   key1\0val1\0key2\0val2\0...\0keyN\0valN\x1e
// A trailing `\x1e` is required as the record terminator. Values pass
// through verbatim (ANSI ESC bytes etc. survive); no escaping needed
// because neither `\0` nor `\x1e` occur in jj's normal output.
pub fn parse_nul_oneline(bytes: &[u8]) -> Option<HashMap<String, Vec<u8>>> {
    let rs_pos = bytes.iter().position(|&b| b == 0x1E)?;
    let body = &bytes[..rs_pos];
    if body.is_empty() {
        return None;
    }
    let parts: Vec<&[u8]> = body.split(|&b| b == 0).collect();
    if parts.len() < 2 || !parts.len().is_multiple_of(2) {
        return None;
    }
    let mut fields = HashMap::with_capacity(parts.len() / 2);
    for chunk in parts.chunks(2) {
        let key = std::str::from_utf8(chunk[0]).ok()?.to_string();
        if key.is_empty() {
            return None;
        }
        fields.insert(key, chunk[1].to_vec());
    }
    Some(fields)
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

// Pass 1: record, per elastic-tab position, the max natural column across
// rows — the row-relative column the tab would land at if nothing padded.
// Tabs are keyed by their left-to-right order in the template (0-indexed),
// NOT by any arg string, so distinct tabs never collide. Pass 2 left-pads
// each row up to its tab's recorded column so the following content's left
// edge lines up. An arg-ful tab advances the column by its field's width
// (it emits that field); an arg-less tab advances by zero (the following
// %{field} node accounts for the width instead).
pub fn collect_anchors(
    template: &Template,
    fields: &HashMap<String, Vec<u8>>,
    anchors: &mut Vec<usize>,
) {
    let mut col: usize = 0;
    let mut tab_i: usize = 0;
    for node in &template.nodes {
        match node {
            Node::Literal(bytes) => {
                col += visible_width(bytes);
            }
            Node::Field(name) => {
                let vw = fields.get(name).map(|v| visible_width(v)).unwrap_or(0);
                col += vw;
            }
            Node::ElasticTab(name) => {
                if tab_i >= anchors.len() {
                    anchors.resize(tab_i + 1, 0);
                }
                if col > anchors[tab_i] {
                    anchors[tab_i] = col;
                }
                let vw = fields.get(name).map(|v| visible_width(v)).unwrap_or(0);
                col += vw;
                tab_i += 1;
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
    // Left-pad cells emitted ahead of an elastic_tab to align its left
    // edge across rows. Combines with adjacent Ws for dash-fill, but is
    // not removed by Rule 2 when the elastic_tab value is empty — the
    // column-alignment commitment survives empty cells.
    Anchor(usize),
    EmptyTag,
}

// Render one row using the four-rule model documented in
// `bijjou-config.toml`:
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
    anchors: &[usize],
    out: &mut Vec<u8>,
) {
    let mut segs: Vec<Seg> = Vec::new();
    if leading_pad > 0 {
        segs.push(Seg::Ws(leading_pad));
    }
    // Track the per-row "natural" visible column (relative to the start
    // of the template — leading_pad is uniform across rows, so it stays
    // out of this counter). Drives elastic_tab left-pad: if the row's
    // current natural column is behind the tab's recorded anchor, we emit
    // the difference so the following content's left edge lands consistently.
    let mut col: usize = 0;
    let mut tab_i: usize = 0;
    for node in &template.nodes {
        match node {
            Node::Literal(b) => {
                col += visible_width(b);
                push_literal_segs(b, &mut segs);
            }
            Node::Field(name) => {
                let value = fields.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
                col += visible_width(value);
                if value.is_empty() {
                    segs.push(Seg::EmptyTag);
                } else {
                    segs.push(Seg::Content(value.to_vec()));
                }
            }
            Node::ElasticTab(name) => {
                let anchor_target = anchors.get(tab_i).copied().unwrap_or(col);
                let left_pad = anchor_target.saturating_sub(col);
                if left_pad > 0 {
                    segs.push(Seg::Anchor(left_pad));
                    col += left_pad;
                }
                // Arg-ful tab emits its field inline; arg-less tab emits
                // nothing (the following %{field} node emits the value).
                if !name.is_empty() {
                    let value = fields.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
                    let vw = visible_width(value);
                    if value.is_empty() {
                        segs.push(Seg::EmptyTag);
                    } else {
                        segs.push(Seg::Content(value.to_vec()));
                    }
                    col += vw;
                }
                tab_i += 1;
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
            // drop the EmptyTag itself. Anchor segments stop the walk —
            // they encode column-alignment that an empty value must not
            // erase.
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
                    Seg::Content(_) | Seg::Anchor(_) => break,
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
//
// Rows whose graph prefix ends in an edge (`leading_left == GraphEdge`)
// still dash-fill: `emit_pad` drops the left cap so the run abuts the
// edge glyph with a plain dash rather than a `╶`, but the dashes (and the
// closing `╴` against content) are emitted as on any other row.
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
            Seg::Ws(_) | Seg::Anchor(_) => {
                let mut total = 0;
                while i < segs.len() {
                    match &segs[i] {
                        Seg::Ws(n) | Seg::Anchor(n) => {
                            total += n;
                            i += 1;
                        }
                        _ => break,
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

// Emit a fixed-width pad run sitting directly to the right of a graph node
// (the gap between a root commit's graph prefix and its value).
pub fn emit_node_pad(cells: usize, out: &mut Vec<u8>) {
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
    fn nul_oneline_basic() {
        let bytes = b"change_id\x00abc\x00commit_id\x00123\x1e";
        let m = parse_nul_oneline(bytes).unwrap();
        assert_eq!(m.get("change_id").unwrap(), b"abc");
        assert_eq!(m.get("commit_id").unwrap(), b"123");
    }

    #[test]
    fn nul_oneline_preserves_raw_esc_and_newlines() {
        let bytes = b"k\x00\x1b[1mhi\nthere\x1b[0m\x1e";
        let m = parse_nul_oneline(bytes).unwrap();
        assert_eq!(m.get("k").unwrap(), b"\x1b[1mhi\nthere\x1b[0m");
    }

    #[test]
    fn nul_oneline_empty_value() {
        let bytes = b"labels\x00\x00description\x00hi\x1e";
        let m = parse_nul_oneline(bytes).unwrap();
        assert_eq!(m.get("labels").unwrap().as_slice(), b"");
        assert_eq!(m.get("description").unwrap(), b"hi");
    }

    #[test]
    fn nul_oneline_root_record() {
        let bytes = b"root\x00zzzzzz root() 000000\x1e";
        let m = parse_nul_oneline(bytes).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m.get("root").unwrap(), b"zzzzzz root() 000000");
    }

    #[test]
    fn nul_oneline_rejects_no_terminator() {
        assert!(parse_nul_oneline(b"k\x00v").is_none());
    }

    #[test]
    fn nul_oneline_rejects_odd_parts() {
        assert!(parse_nul_oneline(b"k\x00v\x00orphan\x1e").is_none());
    }

    #[test]
    fn nul_oneline_rejects_empty_key() {
        assert!(parse_nul_oneline(b"\x00v\x1e").is_none());
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
    fn trailing_tab_aligns_following_field() {
        // To align non-elastic content after an elastic column, put a tab
        // before it. The short row gets dash fill up to the aligned column;
        // the widest row has no pad.
        let t = Template::parse(
            "%{elastic_tab(change_id)} %{elastic_tab()}%{description}",
        )
        .unwrap();
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
        let mut anchors: Vec<usize> = Vec::new();
        collect_anchors(&t, &r1, &mut anchors);
        collect_anchors(&t, &r2, &mut anchors);
        // tab0 (change_id) at col 0; tab1 (before description) at
        // max(change_id width) + 1 literal space = 6 + 1 = 7.
        assert_eq!(anchors, vec![0, 7]);

        let mut out = Vec::new();
        render_row(&t, &r1, 0, LeftSide::Content, &anchors, &mut out);
        let s = String::from_utf8_lossy(&out);
        assert!(s.starts_with("abc"));
        assert!(s.ends_with("short"));
        assert!(s.contains('╶') || s.contains('─'), "expected dash pad: {}", s);

        let mut out2 = Vec::new();
        render_row(&t, &r2, 0, LeftSide::Content, &anchors, &mut out2);
        assert_eq!(out2, b"abcdef longer");
    }

    #[test]
    fn argless_tab_equals_argful() {
        // `%{elastic_tab()}%{X}` must render byte-identically to
        // `%{elastic_tab(X)}` for every row.
        let ta = Template::parse("%{elastic_tab(change_id)} %{description}").unwrap();
        let tb = Template::parse("%{elastic_tab()}%{change_id} %{description}").unwrap();
        let rows: Vec<HashMap<String, Vec<u8>>> = vec![
            [
                ("change_id".to_string(), b"abc".to_vec()),
                ("description".to_string(), b"short".to_vec()),
            ]
            .into_iter()
            .collect(),
            [
                ("change_id".to_string(), b"abcdef".to_vec()),
                ("description".to_string(), b"longer".to_vec()),
            ]
            .into_iter()
            .collect(),
        ];

        let mut anchors_a: Vec<usize> = Vec::new();
        let mut anchors_b: Vec<usize> = Vec::new();
        for r in &rows {
            collect_anchors(&ta, r, &mut anchors_a);
            collect_anchors(&tb, r, &mut anchors_b);
        }
        for r in &rows {
            let mut oa = Vec::new();
            let mut ob = Vec::new();
            render_row(&ta, r, 0, LeftSide::Content, &anchors_a, &mut oa);
            render_row(&tb, r, 0, LeftSide::Content, &anchors_b, &mut ob);
            assert_eq!(oa, ob, "argless and argful must match");
        }
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
        render_row(
            &t,
            &fields,
            0,
            LeftSide::Content,
            &[],
            &mut out,
        );
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
        render_row(
            &t,
            &fields,
            0,
            LeftSide::Content,
            &[],
            &mut out,
        );
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
        render_row(
            &t,
            &fields,
            0,
            LeftSide::Content,
            &[],
            &mut out,
        );
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
        render_row(
            &t,
            &fields,
            2,
            LeftSide::GraphNode,
            &[],
            &mut out,
        );
        // 2 leading_pad + 1 literal = 3 ws cells → dashes; abuts "abc".
        let s = String::from_utf8_lossy(&out);
        assert!(s.ends_with("abc"));
        assert!(s.contains("╶") || s.contains("─"), "expected dash pad: {}", s);
    }
}
