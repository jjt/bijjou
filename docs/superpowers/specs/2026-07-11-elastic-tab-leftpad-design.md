# elastic_tab: left-pad-only model

Date: 2026-07-11
Status: Approved (design)

## Problem

These two templates should be interchangeable but are not:

```
A: ' %{elastic_tab(change_id)} %{elastic_tab(commit_id)} %{elastic_tab(author)} %{elastic_tab(timestamp)} %{working_copies} %{bookmarks} %{tags} %{description}'
B: ' %{elastic_tab()}%{change_id} %{elastic_tab()}%{commit_id} %{elastic_tab()}%{author} %{elastic_tab()}%{timestamp} %{working_copies} %{bookmarks} %{tags} %{description}'
```

Observed (`--activate=always`, sample input):

```
A: @ qp╶─╴a1╶───╴al╶───╴2601·0900 @ main short one
B: @╶──────────────────╴qpa1al2601·0900 @ main short one
```

### Root cause

`elastic_tab` currently keys its alignment state (anchor + width) on the arg
string. `elastic_tab()` parses to `ElasticTab("")` — arg is the empty string:

1. All four `elastic_tab()` calls collide on key `""` → one shared column, not four.
2. `fields[""]` never exists → value empty, width 0.
3. The following `%{change_id}` is a plain `Field` node → no alignment at all.

So B's four tabs merge into one giant left-pad (col 1 → 20) and the fields glue
together. The arg is load-bearing and required by design; the empty-arg form is
malformed input the parser accepts silently instead of rejecting.

## Two models

**Current — "fixed-width box."** `elastic_tab(field)` pins *both* edges of
`field`: left-pad to a shared anchor column, emit value, right-pad to the
field's max width across rows. Aligning whatever follows is a side effect of
right-padding this field.

**New — "tab stop / left-align."** `elastic_tab()` before content pushes *that
content's* left edge to a shared column. Left-pad only. Alignment is expressed
on the thing being aligned. `elastic_tab(field)` becomes sugar: same left-pad,
plus emit `field` inline.

Right-pad is provably redundant whenever the next column is also an
`elastic_tab` (the next column's left-pad absorbs the identical gap). It is only
load-bearing when an elastic column is followed by non-elastic content you still
want aligned. Verified empirically: with right-pad disabled, the default
template's output is byte-identical.

## Spec

### Semantics

| Form | Behavior |
|------|----------|
| `%{elastic_tab(X)}` | left-pad to this column's anchor, then **emit field X** |
| `%{elastic_tab()}`  | left-pad to this column's anchor, **emit nothing** |

Identity: `%{elastic_tab()}%{X}` ≡ `%{elastic_tab(X)}`. Same tab site → same
anchor; `X` is emitted either by the tab or by the following field — same bytes,
same position.

### Column keying — by position

A "column" is the Nth `elastic_tab` node in the template (0-indexed by order of
appearance), NOT the arg string. Collisions are impossible; the old `""` merge
cannot recur. The arg's sole remaining job is whether a field is emitted inline.

### Anchor computation

Unchanged in spirit, re-keyed by position:

- Pass 1 (`collect_widths` → renamed `collect_anchors`): walk `template.nodes`
  in order with a tab counter `i` incremented on each `ElasticTab`; record
  `anchors[i] = max(anchors[i], natural_col_at_tab)`. `natural_col` is the
  row-relative visible column (leading_pad excluded, as today). Both passes
  number tabs by this same left-to-right counter, so indices line up.
- Pass 2 (`render_row`): at tab `i`, `left_pad = anchors[i].saturating_sub(col)`;
  emit as an `Anchor(left_pad)` segment; then, if arg present, emit `fields[arg]`
  value (Content, or EmptyTag when empty).

### Removed

- The `widths` map and all right-pad emission (`Seg::Ws(pad)` after the value in
  the `ElasticTab` arm of `render_row`).
- `TemplateMetrics.widths`.

## Behavior changes

1. **Trailing non-elastic alignment becomes explicit.** Today
   `%{elastic_tab(author)} %{description}` aligns `description` via author's
   right-pad. New model: `description` runs ragged. Regain it by placing a tab
   before it: `%{elastic_tab(author)} %{elastic_tab()}%{description}`.
2. **Default template output: unchanged.** Its tail (`working_copies bookmarks
   tags description`) is not aligned today either — `timestamp` is fixed-width,
   so its right-pad is already 0. Default config stays arg-ful
   (`%{elastic_tab(change_id)} ...`); arg-less is now equivalent, so the user may
   flip it at will.

## Edge cases

1. **Leading tab / graph edges.** The graph→content gap stays handled by
   `leading_pad` in `emit_classified`, separate from anchors (unchanged). A
   leading `%{elastic_tab()}` sits at natural col 0 and left-pads normally.
2. **All-empty column (whole batch).** Anchor is still recorded at that tab site;
   left-pad still emitted as `Anchor`, which survives rule-2 empty-collapse
   (rule-2 stops at `Anchor`). The column stays in place.
3. **Multiple fields between tabs.** No special handling.
   `%{elastic_tab()}%{a}%{b}` aligns the start of `a`; `b` follows raw.

## Code changes (files)

- `src/dsl.rs`
  - `collect_widths` → `collect_anchors`: signature drops `widths`, keys anchors
    by tab-node index. Requires indexing elastic_tab nodes during the walk.
  - `render_row`: drop right-pad block; key left-pad by tab index; arg-ful still
    emits the field value after the anchor.
  - Tests: rewrite `elastic_tab_pad_combines_with_literal_space` (asserts
    `widths == 6` and a right-pad dash run — both removed). Under the new model
    `%{elastic_tab(change_id)} %{description}` no longer pads the short row; the
    aligned variant is `%{elastic_tab(change_id)} %{elastic_tab()}%{description}`.
    Add a test asserting `%{elastic_tab()}%{X}` ≡ `%{elastic_tab(X)}`.
- `src/main.rs`
  - `TemplateMetrics`: remove `widths`; re-type `anchors` from
    `HashMap<String, usize>` to `Vec<usize>` (index = tab order, grown as tabs
    appear). `render_row`'s `anchors` param changes to match.
  - `accumulate_metrics` / `emit_classified`: pass anchors only.
- `src/config.rs`: no change (default body stays arg-ful).
- `ARCHITECTURE.md`, `README.md`: update the elastic_tab description to the
  left-pad model.

## Out of scope (YAGNI)

- Named/shared columns across tab sites (`elastic_tab(col=x)`). Position keying
  is enough; revisit only if a real cross-site-sharing need appears.
- Rejecting empty-arg as a parse error — obsolete, empty-arg is now a valid,
  meaningful form.
