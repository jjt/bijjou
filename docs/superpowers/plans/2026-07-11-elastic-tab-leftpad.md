# elastic_tab Left-Pad-Only Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redefine `elastic_tab` as a left-pad-only tab stop so `%{elastic_tab()}%{X}` and `%{elastic_tab(X)}` produce identical output, and remove the redundant right-pad/`widths` machinery.

**Architecture:** `elastic_tab` becomes a column keyed by tab *position* (Nth tab in the template), not by arg string. Each tab left-pads the current row to that column's max natural column across rows (the "anchor"); an arg-ful tab then emits its field inline, an arg-less tab emits nothing. The `widths` map and all right-pad emission are deleted. Anchors are stored per template as a `Vec<usize>` indexed by tab order.

**Tech Stack:** Rust (edition per `Cargo.toml`), `cargo test`, `insta` snapshots + `assert_cmd` for golden tests.

**Spec:** `docs/superpowers/specs/2026-07-11-elastic-tab-leftpad-design.md`

---

## File structure

- `src/dsl.rs` — rename `collect_widths` → `collect_anchors` (drop `widths`, key anchors by tab index); rewrite the `ElasticTab` arm of `render_row` (drop right-pad, position-keyed left-pad, conditional inline emit); change `render_row` signature (drop `widths`, `anchors` becomes `&[usize]`); update + add unit tests.
- `src/main.rs` — `TemplateMetrics` drops `widths`, `anchors` becomes `Vec<usize>`; update the import, `accumulate_metrics`, `emit_classified`, and `emit_missing_template` call sites.
- `src/stream.rs` — no change (only names the `TemplateMetrics` type and calls `accumulate_metrics`/`emit_classified`, both transparent to the internal change).
- `README.md`, `ARCHITECTURE.md` — update the `elastic_tab` description to the left-pad model.

No config change: the default template body in `src/config.rs` stays arg-ful and is unaffected.

---

## Task 1: Rewrite the elastic_tab engine (left-pad-only, position-keyed)

**Files:**
- Modify: `src/dsl.rs` (`collect_widths`, `render_row`, tests)
- Modify: `src/main.rs` (`TemplateMetrics`, import, `accumulate_metrics`, `emit_classified`, `emit_missing_template`)

- [ ] **Step 1: Write the failing equivalence test**

Add this test inside the `#[cfg(test)] mod tests` block in `src/dsl.rs` (e.g. right after `elastic_tab_pad_combines_with_literal_space`). It uses the *new* `render_row` signature (`anchors: &[usize]`, no `widths`) and the *new* `collect_anchors`, so it will not compile until the implementation lands — that is the intended failing state.

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib argless_tab_equals_argful`
Expected: FAIL — a compile error (`collect_widths` referenced by name change, `render_row`/`collect_anchors` arity/signature mismatch). This confirms the test drives the new API.

- [ ] **Step 3: Rewrite `collect_widths` → `collect_anchors` in `src/dsl.rs`**

Replace the entire `collect_widths` function (currently the block starting at the `pub fn collect_widths(` signature through its closing brace) with:

```rust
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
```

- [ ] **Step 4: Rewrite the `ElasticTab` arm and signature of `render_row` in `src/dsl.rs`**

Change the `render_row` signature — drop the `widths` parameter and change `anchors` to a slice:

```rust
pub fn render_row(
    template: &Template,
    fields: &HashMap<String, Vec<u8>>,
    leading_pad: usize,
    leading_left: LeftSide,
    anchors: &[usize],
    out: &mut Vec<u8>,
) {
```

Add a tab counter next to the existing `let mut col: usize = 0;` line in `render_row`:

```rust
    let mut col: usize = 0;
    let mut tab_i: usize = 0;
```

Replace the entire `Node::ElasticTab(name) => { ... }` arm inside `render_row` with:

```rust
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
```

- [ ] **Step 5: Update `src/main.rs` — `TemplateMetrics`, import, and call sites**

Change the `TemplateMetrics` struct and its doc comment (currently declaring `widths` and `anchors` as `HashMap`s) to:

```rust
// Per-template alignment state. `anchors[i]` is the row-wide max natural
// column before the i-th elastic_tab in the template (tabs keyed by
// left-to-right order). Grows monotonically as rows are scanned.
#[derive(Default)]
pub struct TemplateMetrics {
    pub anchors: Vec<usize>,
}
```

Update the `use crate::dsl::{...}` import (line ~14): replace `collect_widths` with `collect_anchors`:

```rust
use crate::dsl::{collect_anchors, parse_nul_oneline, render_row, LeftSide, Node, Template};
```

In `accumulate_metrics`, replace the `collect_widths(...)` call:

```rust
                    let entry = metrics.entry(name.to_string()).or_default();
                    collect_anchors(template, fields, &mut entry.anchors);
```

In `emit_classified`, update the `render_row` call in the `CompiledTemplate::Parsed` arm — drop `&m.widths`:

```rust
                Some(CompiledTemplate::Parsed(template)) => {
                    let m = metrics.get(name).unwrap_or_else(|| empty_metrics());
                    render_row(
                        template,
                        fields,
                        leading_pad,
                        leading_left,
                        &m.anchors,
                        out,
                    );
                }
```

In `emit_missing_template`, update its `render_row` call the same way — drop `&m.widths`:

```rust
    let m = empty_metrics();
    render_row(
        &synth,
        &fields,
        leading_pad,
        leading_left,
        &m.anchors,
        out,
    );
```

(`empty_metrics()` and its `OnceLock<TemplateMetrics>` need no change — `Default` now yields an empty `anchors` vec.)

- [ ] **Step 6: Update remaining `render_row` call sites in `src/dsl.rs` tests**

Four existing tests call `render_row` with two trailing `&HashMap::new()` args (widths, anchors). Change each to a single `&[]` (empty anchor slice). The affected tests are `empty_field_collapses_preceding_ws`, `leading_template_ws_is_preserved`, `empty_first_field_keeps_leading_ws_and_collapses_right`, and `leading_pad_combines_with_template_leading_ws`. In each, replace:

```rust
            &HashMap::new(),
            &HashMap::new(),
            &mut out,
```

with:

```rust
            &[],
            &mut out,
```

- [ ] **Step 7: Rewrite `elastic_tab_pad_combines_with_literal_space`**

This test asserts the removed `widths` behavior and right-pad. Replace the whole `fn elastic_tab_pad_combines_with_literal_space()` test with a test of the new explicit-trailing-align pattern (`%{elastic_tab(change_id)} %{elastic_tab()}%{description}`):

```rust
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
```

- [ ] **Step 8: Run the full test suite**

Run: `cargo test`
Expected: PASS — all `src/dsl.rs` unit tests (including `argless_tab_equals_argful` and `trailing_tab_aligns_following_field`), plus the golden tests in `tests/golden.rs` (behavior for the default fully-wrapped template is unchanged, so snapshots must not shift). If a golden snapshot changed, STOP and investigate — the default template output must be byte-identical.

- [ ] **Step 9: Verify no stray references to the old API remain**

Run: `rg -n "collect_widths|\.widths|widths:" src/`
Expected: no matches. If any remain, fix them and re-run `cargo test`.

- [ ] **Step 10: Commit**

```bash
jj commit -m "elastic_tab: left-pad-only, position-keyed columns

Drop the right-pad/widths machinery. Columns are now keyed by tab
position, so %{elastic_tab()}%{X} == %{elastic_tab(X)}. Arg-ful tabs
emit their field inline; arg-less tabs are pure alignment markers."
```

---

## Task 2: End-to-end CLI equivalence + golden regression (verification)

**Files:** none modified (verification only).

- [ ] **Step 1: Build the binary**

Run: `cargo build`
Expected: `Finished` with no errors.

- [ ] **Step 2: Generate sample input**

Create `/tmp/claude/gen.py`:

```python
import sys
def rec(node,**kv):
    parts=[]
    for k,v in kv.items():
        parts.append(k); parts.append(v)
    return node+"  "+"\x00".join(parts)+"\x1e\n"
rows=[
 ("@",dict(bijjou_template_name="log_oneline",change_id="qp",commit_id="a1",author="al",timestamp="2601·0900",working_copies="@",bookmarks="main",tags="",description="short one")),
 ("○",dict(bijjou_template_name="log_oneline",change_id="wxyz",commit_id="beef42",author="bob@ex",timestamp="2601·1130",working_copies="",bookmarks="",tags="v1",description="longer description here")),
 ("○",dict(bijjou_template_name="log_oneline",change_id="m",commit_id="c",author="carol",timestamp="2512·2359",working_copies="",bookmarks="feature",tags="",description="mid")),
]
sys.stdout.write("".join(rec(n,**r) for n,r in rows))
```

Run: `mkdir -p /tmp/claude && python3 /tmp/claude/gen.py > /tmp/claude/input.bin`

- [ ] **Step 3: Diff arg-ful vs arg-less template output**

Run:

```bash
BIN=./target/debug/bijjou
A=' %{elastic_tab(change_id)} %{elastic_tab(commit_id)} %{elastic_tab(author)} %{elastic_tab(timestamp)} %{working_copies} %{bookmarks} %{tags} %{description}'
B=' %{elastic_tab()}%{change_id} %{elastic_tab()}%{commit_id} %{elastic_tab()}%{author} %{elastic_tab()}%{timestamp} %{working_copies} %{bookmarks} %{tags} %{description}'
$BIN --color=never --stream=false --activate=always "--templates__log_oneline=$A" < /tmp/claude/input.bin > /tmp/claude/A.txt
$BIN --color=never --stream=false --activate=always "--templates__log_oneline=$B" < /tmp/claude/input.bin > /tmp/claude/B.txt
diff /tmp/claude/A.txt /tmp/claude/B.txt && echo "IDENTICAL"
```

Expected: `IDENTICAL`. (Before this change, B produced a giant dash run and glued fields; now the two must match.)

- [ ] **Step 4: Confirm streaming path matches too**

Run the same two commands without `--stream=false` and diff again:

```bash
$BIN --color=never --activate=always "--templates__log_oneline=$A" < /tmp/claude/input.bin > /tmp/claude/A2.txt
$BIN --color=never --activate=always "--templates__log_oneline=$B" < /tmp/claude/input.bin > /tmp/claude/B2.txt
diff /tmp/claude/A2.txt /tmp/claude/B2.txt && echo "IDENTICAL"
```

Expected: `IDENTICAL`.

No commit (nothing changed). If any diff is non-identical, STOP and return to Task 1.

---

## Task 3: Update documentation

**Files:**
- Modify: `README.md:89`
- Modify: `ARCHITECTURE.md` (lines ~47, ~51, ~52, ~71-84)

- [ ] **Step 1: Update `README.md`**

Replace the paragraph at `README.md:89`:

```
The `elastic_tab()` function aligns the content in a column, and adds a horizontal guide line. You can see this in effect in the change ids: note how they are all aligned.
```

with:

```
The `elastic_tab()` function is a tab stop: it left-pads the current row so the content that follows lines up in a column across rows, adding a horizontal guide line in the gap. You can see this in effect in the change ids: note how they are all aligned. `%{elastic_tab(field)}` is shorthand for a tab immediately followed by `%{field}` — it pads, then emits the field. Columns are keyed by tab position, so each `elastic_tab` in a template is its own column.
```

- [ ] **Step 2: Update `ARCHITECTURE.md` table row for `main.rs` (line ~47)**

Replace `and per-template `TemplateMetrics` (max widths + anchors).` with `and per-template `TemplateMetrics` (position-keyed anchors).`

- [ ] **Step 3: Update `ARCHITECTURE.md` table row for `dsl.rs` (line ~51)**

Replace the `dsl.rs` cell text:

```
Templating DSL + NUL/RS-framed record parser (`parse_nul_oneline`). `Template::parse` builds an AST of literal text, `%{field}` lookups, and `%{elastic_tab(field)}` align points. Two-pass render (`collect_widths` → `render_row`): pass 1 records each elastic-tab field's max visible width **and** max natural column (anchor); pass 2 left-pads to the anchor so the field's left edge lines up, then right-pads the value to the max width. Whitespace follows a 4-rule model (see below).
```

with:

```
Templating DSL + NUL/RS-framed record parser (`parse_nul_oneline`). `Template::parse` builds an AST of literal text, `%{field}` lookups, and `%{elastic_tab(field)}` align points. Two-pass render (`collect_anchors` → `render_row`): pass 1 records each elastic-tab's max natural column (anchor), keyed by tab position; pass 2 left-pads to the anchor so the following content's left edge lines up. An arg-ful tab then emits its field inline; an arg-less tab emits nothing (`%{elastic_tab()}%{X}` == `%{elastic_tab(X)}`). Whitespace follows a 4-rule model (see below).
```

- [ ] **Step 4: Update `ARCHITECTURE.md` `stream.rs` table row (line ~52)**

Replace `monotonic widening (widths, anchors, and `graph_col` targets never shrink...` with `monotonic widening (anchors and `graph_col` targets never shrink...` (drop `widths,`).

- [ ] **Step 5: Update `ARCHITECTURE.md` render-flow steps 3-4 (lines ~71-84)**

Replace step 3:

```
3. Pass 1 over the buffer (or batch): per named template, `collect_widths`
   records each elastic-tab field's max visible width and max natural
   column (anchor); also track the overall max `graph_col` across commit
   rows.
```

with:

```
3. Pass 1 over the buffer (or batch): per named template, `collect_anchors`
   records each elastic-tab's max natural column (anchor), keyed by tab
   position; also track the overall max `graph_col` across commit rows.
```

Replace the `%{elastic_tab(field)}` sentence in step 4 (lines ~80-84):

```
     literal text and `%{field}` lookups emit verbatim;
     `%{elastic_tab(field)}` left-pads to the field's anchor, emits the
     value, then emits `max_width - this_width` right-pad fill cells (one
     space if the gap is one cell; otherwise dashes with
     `layout.dash-start` / `layout.dash-end` caps).
```

with:

```
     literal text and `%{field}` lookups emit verbatim;
     `%{elastic_tab(...)}` left-pads to its column's anchor (the fill is one
     space for a one-cell gap, otherwise dashes with `layout.dash-start` /
     `layout.dash-end` caps), then an arg-ful tab emits its field value
     while an arg-less tab emits nothing.
```

- [ ] **Step 6: Commit**

```bash
jj commit -m "docs: describe elastic_tab left-pad-only model"
```

---

## Self-review notes

- **Spec coverage:** semantics table (Task 1 steps 3-4), position keying (steps 3-4), anchor computation (step 3), removed widths/right-pad (steps 3-5), equivalence (step 1 test + Task 2), trailing-align-now-explicit (step 7 test + README), all-empty column (preserved via `Anchor` surviving rule-2 — unchanged code path), multiple fields (no special handling — `Field` nodes untouched), default template unchanged (golden tests, Task 1 step 8; Task 2 step 3). Docs (Task 3).
- **`stream.rs`:** intentionally untouched — it only references the `TemplateMetrics` type and the `accumulate_metrics`/`emit_classified` fns, all of which keep their outer signatures.
- **Type consistency:** `anchors` is `Vec<usize>` in `TemplateMetrics` and `collect_anchors(&mut Vec<usize>)`, passed to `render_row(anchors: &[usize])` via deref coercion. `collect_widths` fully renamed to `collect_anchors` (verified by Task 1 step 9 grep).
