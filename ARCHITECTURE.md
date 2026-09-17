# Architecture

`bijjou` = stdin/stdout filter. Post-process `jj log` output: rewrite edge
glyphs, dim edges, and re-render per-commit content from a NUL/RS-framed
payload (emitted by a custom jj log template — see `bijjou-jj-config.toml`)
through a small templating DSL. Each payload names a bijjou template via a
`bijjou_template_name` field, which selects one of the configured
`[templates]` entries. Lines that aren't framed commit rows pass through
byte-for-byte. Node glyphs themselves are owned by jj's template — bijjou
recognizes them but never rewrites them.

## Pipeline

```
stdin → activation check → (stream | buffered) → classify → render → output sink → stdout|pager
```

- **Activate gate** (`Activate::Never|Auto|Always`): `Never` = raw copy.
  `Auto` = look for the `bijjou_template_name` field in input, else
  passthrough (in streaming mode the scan is limited to the first batch).
  `Always` (default) = process every line.
- **Stream vs buffered**: `[stream].enabled` switches paths (default on).
  - Stream (`stream.rs`): read in batches, two-pass per batch, flush as
    input arrives. Batch size is `[stream].batch-size` — a fixed line
    count (default 128) or `half-pager` (first batch = `rows-1`, each
    later batch = `(rows-1)/2`).
  - Buffered (`main.rs::run`): slurp stdin, two-pass once, emit.

## Templates

A `[templates]` table maps names → DSL bodies, compiled once at startup
into `CompiledTemplate` (`main.rs::compile_templates`). Each commit row's
`bijjou_template_name` field picks the entry to render with:

- **Parsed** body → render the row through the DSL.
- **Empty** body (`name = ""`) → emit the graph prefix only, drop the
  rest of the row.
- **Missing** (name not in `[templates]`) → emit a dim
  `no bijjou template for <name>` notice where content would go.
- **No name** (row parsed as fields but carried no `bijjou_template_name`)
  → pass the rest of the line through verbatim instead of dropping it.

## Modules

| File         | Job                                                                                      |
| ------------ | ---------------------------------------------------------------------------------------- |
| `main.rs`    | Arg parse, config load chain, dispatch (stream vs buffered). Owns the shared row core: `RowKind`, `classify_row`, `emit_classified`, template compilation, and per-template `TemplateMetrics` (position-keyed anchors). |
| `config.rs`  | `Config` struct, TOML/env/CLI merge, global `cfg()`. Precedence file < env < CLI            |
| `ansi.rs`    | Byte-level ANSI utils: CSI skip, UTF-8 decode, SGR filter/strip                          |
| `render.rs`  | Parse line → `Parsed{graph_col, graph_col_collapsed, graph_end, content_start, last_is_edge, last_is_edge_collapsed}`, recognize edges (box-drawing + elision) by codepoint and nodes structurally (any non-edge glyph in the graph region, incl. custom `log_node` glyphs), emit dimmed edges, and drop inter-column pad cells under `graph.collapse`. Node bytes (and their surrounding ANSI) are forwarded unchanged — node coloring is jj's job, unless `emit_dim_graph` is handed a hydra stack colour. `graph_nodes_to_verticals` rewrites a prefix's node back into a vertical, for the hydra padding row. `node_cell` reports the cell a prefix's node sits in, which is how hydra bounds a stack to its own column. |
| `dsl.rs`     | Templating DSL + NUL/RS-framed record parser (`parse_nul_oneline`). `Template::parse` builds an AST of literal text, `%{field}` lookups, and `%{elastic_tab(field)}` align points. Two-pass render (`collect_anchors` → `render_row`): pass 1 records each elastic-tab's max natural column (anchor), keyed by tab position; pass 2 left-pads to the anchor so the following content's left edge lines up. An arg-ful tab then emits its field inline; an arg-less tab emits nothing (`%{elastic_tab()}%{X}` == `%{elastic_tab(X)}`). Whitespace follows a 4-rule model (see below). |
| `stream.rs`  | Batched reader (`read_batch`), two-pass per batch with monotonic widening (anchors and `graph_col` targets never shrink as new batches arrive), `OutputSink` (stdout or pager spawned via `std::process::Command`/`posix_spawn`). |
| `hydra.rs`   | Hydra awareness. `Topology::from_prefixes` expands `hydra.prefixes` into the bookmark names in force (`HYS-`, `HYWC-`, and the `HYB` / `HYH` / `HYCR` anchors) once, into the `TOPOLOGY` `LazyLock`, together with the stand-in each reads as under `hydra.prefixes-replace`. `Walk` is the per-row state: it classifies each commit row by its `bookmarks` field (stack marker / working copy / anchor / neither), carries a stack's colour down from its marker to the content commits under it in the same graph column (`render::node_cell`), so a commit drawn in another column is nobody's stack, colours a `HYWC-*` row with its stack's colour without carrying it, and draws the top-stack separator row. `Walk::markup` returns both the row's node SGR and, under `hydra.color-bookmarks` or any `hydra.prefixes-replace` key, a rewritten `bookmarks` field whose hydra names carry their stack's colour instead of jj's and read under their stand-ins (`render_row`'s field override). `replace_names` is the same substitution without the colours, for pass 1's anchors. No subprocess, so nothing to wait on. |
| `output.rs`  | Buffered path's terminal write / pager exec (`fork` + `execvp`, replacing bijjou's process)  |

## Render flow per line

1. `find_boundary` → locate end of graph prefix. A position is "graph"
   if its codepoint is an edge (box-drawing range or elision char), or
   is a node — recognized structurally as any non-edge, non-space glyph
   followed by a space or an edge (the column gap jj pads after every
   node). This means custom `log_node` glyphs (□, Nerd-Font PUA, …) are
   handled without enumerating them; content is never misread because its
   first glyph always sits past the gap. `last_is_edge` records whether
   the prefix ended on an edge or a node.
   Under `graph.collapse` the pad cell of every graph column (`is_pad_cell`:
   odd cell index holding a space or a horizontal) is dropped, so column N
   lands at cell N. `Parsed` carries both column counts and both
   `last_is_edge` flags; `classify_row` picks the pair matching the config so
   the graph→content gap matches the prefix actually emitted. Parity is what
   keeps this safe: horizontals that *are* a column's glyph (`├───╯`) and one
   cell of every inactive column survive. Passthrough rows with no boundary
   only collapse when `is_graph_only` holds — prose containing a stray
   box-drawing char must not lose every second character.
2. `classify_row` → after the graph prefix, look for a NUL/RS-framed
   payload (`key\0val\0…\x1e`, emitted by the custom jj log template).
   Lines that parse become `RowKind::Commit` (carrying `graph_col`,
   `graph_end`, `last_is_edge`, `template_name`, and `fields`); a record
   that is just `root\0<value>` becomes `RowKind::Root`; anything else
   stays `RowKind::Passthrough`.
3. Pass 1 over the buffer (or batch): per named template, `collect_anchors`
   records each elastic-tab's max natural column (anchor), keyed by tab
   position; also track the overall max `graph_col` across commit rows.
4. Pass 2 — `emit_classified`:
   - Commit: `hydra::Walk::node_color` for the row's stack colour (and the
     top-stack separator row, if this row opens the second stack),
     `emit_dim_graph` for the graph prefix, right-pad to the
     max graph column (handed to the DSL as a leading pad), then dispatch
     on the row's template (Parsed / Empty / missing / no-name; see
     **Templates**). For a Parsed body, `render_row` walks the template:
     literal text and `%{field}` lookups emit verbatim;
     `%{elastic_tab(...)}` left-pads to its column's anchor (the fill is one
     space for a one-cell gap, otherwise dashes with `layout.dash-start` /
     `layout.dash-end` caps), then an arg-ful tab emits its field value
     while an arg-less tab emits nothing.
   - Root: emit the graph prefix then a 2-cell pad and the `root` value
     verbatim (no template) so root commits don't perturb column widths.
   - Passthrough: `emit_line` from `render.rs` handles the graph-only
     and unframed cases (just the edge-dim rewrite + verbatim tail).

## Hydra markup

A hydra merges linear stacks as siblings off one base, so `jj log` gives each
stack a graph column. The bookmark naming is configurable per repo, so
`hydra.rs` reads it from `hydra.prefixes` and expands it once into
`Topology{stack_prefix, wc_prefix, anchors}` plus the stand-in each of those
reads as under `hydra.prefixes-replace`. The topology is a `LazyLock`, so
both render passes see the same names. A repo with no hydra carries no
bookmark that matches, which is the same thing as no markup. Asking
`hydra status --toml` instead would be authoritative but shells out to jj
several times per log — more than the whole render costs.

`Walk::node_color` runs once per commit row, in log order, from
`emit_classified`:

- A row carrying `HYS-<name>` opens that stack. A row carrying an anchor
  (`HYB` / `HYH` / `HYCR`) leaves hydra territory. A row carrying
  `HYWC-<name>` leaves it too — a working copy sits above the head, outside
  every stack — but the row is that stack's, so it is coloured with the
  stack's colour and carries nothing onward. Anything else — a stack's
  content commits, which name no bookmark — keeps whichever state the walk is
  in, which is how a colour reaches them. Remote refs (`name@remote`) and
  jj's out-of-sync `*` flag are stripped first, since `commit.bookmarks()`
  carries both.
- The column bounds a stack at the bottom. A marker records the cell its node
  sits in (`render::node_cell` over the row's own graph prefix, counting jj's
  two cells per column), and a bookmarkless row only keeps the stack's colour
  while its node stays in that cell. A commit drawn in another column — an
  extra head off the base, below the log's bottom stack and above `HYB` — is
  nobody's stack, so it keeps jj's colours, and the stack does not resume
  under it.
- The row's node colour is the stack's: hashed from its name
  (`hydra.colors = true`), or the palette entry for its index — the order the
  log first named that stack, held for the rest of the run. Working-copy rows
  come above the stacks, so they are what registers the order. The hash walks
  `HUE_SPACE` — the hue circle less `RESERVED_HUES`, 10° either side of
  `#a6e3a1` (115°) and `#f5c2e7` (316°) — and `hue_of` maps its index back
  onto real degrees by skipping the bands, so the hues stay evenly spread. A
  configured palette is passed through as written.
- Under `hydra.color-bookmarks` (default on) the same colour is put on the
  names themselves, and under `hydra.prefixes-replace` the names are printed
  under their stand-ins: `rewrite_bookmarks` splits the `bookmarks` field on
  whitespace, and for each `HYS-*` / `HYWC-*` / anchor token `emit_token`
  drops jj's foreground SGRs (colours only), puts the stack's colour in
  front, and substitutes the token's leader for its stand-in. The rewritten
  field reaches `render_row` as a per-row field override. Every other
  bookmark on the row passes through byte-for-byte.
- A stand-in is not the width of the name it replaces, so pass 1 has to
  measure the anchors on the rewritten field too: `accumulate_metrics` calls
  `hydra::replace_names` — the same substitution, colourless, which needs no
  walk state — and hands it to `collect_anchors` as the same kind of
  override. Recolouring alone changes no width, which is why
  `hydra.color-bookmarks` needs nothing in pass 1.
- Opening the *second* stack draws the separator row jj skipped under the
  first: `graph_nodes_to_verticals` on that row's own graph prefix, back
  through `emit_dim_graph`, so every column lands where it does above and
  below (under `graph.collapse` too).

The walk is single-pass and stateful, so it works identically on the buffered
and streaming paths, and it assumes jj's default top-down order — under
`jj log --reversed` a stack's commits precede its marker and the walk cannot
follow.

## DSL whitespace model

`render_row` classifies output into segments (`Content`, touchable `Ws`,
elastic-tab left-pad `Anchor`, and zero-width `EmptyTag`) and applies four
rules in order:

1. Leading whitespace before the first non-whitespace character is
   preserved verbatim (never collapses, even when the first field is empty).
2. When a `%{}` block emits empty bytes, every whitespace cell between it
   and the nearest non-whitespace character to its **left** collapses to
   zero. `Anchor` cells stop the walk — column-alignment survives empty
   values.
3. After rules 1-2 and elastic-tab alignment, any run of consecutive
   whitespace cells is dash-filled (single cells stay spaces; runs of two
   or more become a capped dash run). The graph→content gap participates
   in this fill alongside the template's own whitespace.
4. Bytes that came out of a `%{}` block are never modified — internal
   whitespace inside a value passes through untouched.

## Dash spec

A "dash run" is the filler placed between a graph node and the rest of
the commit info on the same line. The spec is the single source of
truth for both the intra-graph runs (`render.rs::flush_internal_run`)
and the graph→content / inter-field runs (`dsl.rs::emit_pad`).

- A dash run goes between a graph **node** (not a graph edge) and the
  rest of the commit info on the line.
- Dashes are logically continuous from the node out to the content;
  graph **edges** appearing in the way puncture the run, but the run
  resumes on the other side of the edge.
- Dashes are never emitted on top of a graph edge cell.
- Going left-to-right, the cell immediately right of a node uses
  `layout.dash-start` (default `╶`) — but **only** if that cell is also
  to the left of whitespace OR a graph edge. If the cell right of a node
  sits directly to the left of another node, no dash is emitted at all
  (the space is preserved). If it sits directly to the left of content,
  it's the lone closing cell instead.
- The closing cell — the one immediately left of the content the run
  terminates against — is never a dash. It holds `layout.dash-end`, which
  defaults to `""`, i.e. a plain space, so content always has a space to
  its left (`○╶───── qquxkvlu`, `●╶𜸩 vqyvkx`). Set it to `╴` for the
  half-line cap that mirrors `dash-start`.
- A one-cell run is nothing but its closing cell.
- All other cells in the run use `layout.dash` (default `─`).

Set `layout.dash-start = ""` to drop the opening cap (that cell becomes a
plain dash).

## Config surface

Single global `OnceLock<Config>` via `cfg()`. Three merge layers:

1. `Config::load` → read TOML from `$BIJJOU_CONFIG` | XDG | `~/.config/...`.
   Writes the embedded `bijjou-config.toml` to the XDG path on first run.
2. `apply_env` → `BIJJOU__SECTION__KEY=VAL`.
3. `apply_cli` → `--key__sub=val` (plus shorthands `--activate`,
   `--color`, `--stream[=bool]`).

Keys: top-level (`activate`, `pager`), `[ui].color`,
`[layout]` (`dash`, `dash-start`, `dash-end`), `[templates].<name>`,
`[stream]` (`enabled`, `batch-size` = int | `"half-pager"`),
`[graph]` (`collapse`), `[graph.edges.chars]`, `[colors]`
(`dash-filler`, `graph-edge`), `[hydra]` (`enable`,
`top-stack-padding`, `color-bookmarks`, `colors` = `true` | `false` |
comma/TOML list of `int 0-255 | "#rrggbb"`), `[hydra.prefixes]` (`prefix`, `base`, `head`,
`conflict-resolution`, `stack-head`, `stack-working-copy`),
`[hydra.prefixes-replace]` (the same keys, each optional, each the rendered
stand-in for that name's leader), plus the hidden
`debug.force-screen-height`. Full ref: `bijjou-config.toml`.

## Output

- TTY + `pager=auto|always` + `$PAGER` set → spawn pager subprocess,
  pipe bytes.
- Else → `stdout.write_all`.
- `--color=auto` strips SGR when stdout not a TTY (via `ansi::strip_sgr`;
  gated by `config::color_enabled`).

## Tests

- `tests/golden.rs` + `insta` snapshots under `tests/snapshots/`, rendered
  under `bijjou-config.toml` with `ui.color` forced on and `hydra.enable`
  forced off (the hydra cases opt back in; the fixture's own `HY*` bookmarks
  carry the topology, so no test depends on a live hydra). Run
  `mise run test-insta`. Review with `cargo insta review`.
- Unit tests inline in `render.rs`, `dsl.rs`, `ansi.rs`, `stream.rs`,
  `config.rs`, `hydra.rs`.
