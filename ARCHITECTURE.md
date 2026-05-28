# Architecture

`bijjou` = stdin/stdout filter. Post-process `jj log` output: rewrite edge
glyphs, dim edges, and re-render per-commit content from a JSON payload
(emitted by `bijjou log-oneline-json`) through a small templating DSL.
Lines that aren't JSON commit rows pass through byte-for-byte. Node
glyphs themselves are owned by jj's template (see `bijjou jj-config`) —
bijjou recognizes them but never rewrites them.

## Pipeline

```
stdin → activation check → (stream | buffered) → render → output sink → stdout|pager
```

- **Activate gate** (`Activate::Never|Auto|Always`): `Never` = raw copy.
  `Auto` = look for `activation-marker` in input, else passthrough.
- **Stream vs buffered**: `[stream].enabled` switches paths.
  - Stream (`stream.rs`): read in batches, emit per line, flush as input
    arrives.
  - Buffered (`main.rs::run`): slurp stdin, emit one pass.

## Modules

| File         | Job                                                                                      |
| ------------ | ---------------------------------------------------------------------------------------- |
| `main.rs`    | Arg parse, config load chain, dispatch buffered path                                      |
| `config.rs`  | Config struct, TOML/env/CLI merge, global `cfg()`. Precedence file < env < CLI            |
| `ansi.rs`    | Byte-level ANSI utils: CSI skip, UTF-8 decode, SGR filter/strip                          |
| `render.rs`  | Parse line → `Parsed{graph_col, graph_end, content_start}`, recognize edges (box-drawing) and nodes (jj defaults + configured icons), emit dimmed edges. Node bytes (and their surrounding ANSI) are forwarded unchanged — node coloring is jj's job via the `graph_node` label set by the template. |
| `dsl.rs`     | Templating DSL + flat JSON parser. `Template::parse` builds an AST of literal text, `%{field}` lookups, and `%{elastic_tab(field)}` align points. Two-pass render: collect max visible widths, then emit each row with right-padded elastic-tab fields. |
| `stream.rs`  | Batched reader, two-pass per batch with monotonic widening (column targets never shrink as new batches arrive), `OutputSink` (stdout or piped pager). |
| `output.rs`  | Buffered path's terminal write / pager spawn                                              |

## Render flow per line

1. `find_boundary` → locate end of graph prefix. A position is "graph"
   if its codepoint is in the box-drawing range, is the elision char, is
   one of jj's default node chars (`@ ○ ◆ × ●`), or matches the first
   codepoint of any configured `[graph.nodes.chars]` icon.
2. `classify_row` → after the graph prefix, look for a `{...}` JSON
   payload (jj's `log-oneline-json` template output). Lines that parse
   become `RowKind::Commit{graph_col, fields}`; the special `{"root":...}`
   shape becomes `RowKind::Root`; anything else stays `RowKind::Passthrough`.
3. Pass 1 over the buffer (or batch): collect per-field max visible
   widths (`collect_widths`) and the overall max `graph_col` across
   commit rows.
4. Pass 2 — `emit_classified`:
   - Commit: `emit_dim_graph` for the graph prefix, right-pad to the
     max graph column, then `render_row` walks the template: literal
     text and `%{field}` lookups emit verbatim; `%{elastic_tab(field)}`
     emits the field value followed by `max_width - this_width` fill
     cells (one space if the gap is one cell; otherwise dashes with
     `layout.dash-start` / `layout.dash-end` caps).
   - Root: emit the graph prefix then the `root` value verbatim (no
     template) so root commits don't perturb column widths.
   - Passthrough: `emit_line` from `render.rs` handles the graph-only
     and non-JSON cases (just the edge-dim rewrite + verbatim tail).

## Subcommands

- `bijjou jj-config` — emit a multi-line TOML snippet for a jj config
  file (`templates.log_node` + `colors.graph_node`). The template body
  substitutes the configured icons for `hidden`, `working-copy[-empty]`,
  `immutable[+empty]`, `empty`, `conflict`, and `mutable` (used for
  non-empty mutable / catch-all).
- `bijjou jj-graph-node-config` — same template body, one-line
  `--config templates.log_node=… --config colors.graph_node=…`. Values
  are TOML basic strings with every whitespace char encoded as `\uXXXX`,
  so the line word-splits at argument boundaries when expanded with
  `$(…)` and jj's TOML parser still sees the original whitespace:
    `jj log $(bijjou jj-graph-node-config) | bijjou`
  No shell quoting, no `eval` — this avoids the failure mode where
  shell quotes embedded in command output reach jj literally.
- `bijjou log-oneline-json` — emit a jj log template expression that
  produces one JSON object per commit. Intended for use with the
  forthcoming content-DSL renderer.

`[graph.nodes.chars].fallback` is a config-only key: not emitted by the
default template, but recognized as a node icon in input so a custom
template that emits a different glyph still parses correctly.

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
- The cell immediately left of the content the run terminates against
  uses `layout.dash-end` (default `╴`).
- All other cells in the run use `layout.dash` (default `─`).

Set `layout.dash-start = ""` to disable caps entirely.

## Config surface

Single global `OnceLock<Config>` via `cfg()`. Three merge layers:

1. `Config::load` → read TOML from `$BIJJOU_CONFIG` | XDG | `~/.config/...`.
   Writes default on first run.
2. `apply_env` → `BIJJOU__SECTION__KEY=VAL`.
3. `apply_cli` → `--key__sub=val`.

Keys: top-level (`activate`, `pager`, `activation-marker`),
`[ui]`, `[layout]`, `[template]`, `[stream]`,
`[graph.nodes.chars]`, `[graph.edges.chars]`,
`[colors]`. Full ref: `config.default.toml`.

## Output

- TTY + `pager=auto|always` + `$PAGER` set → spawn pager subprocess,
  pipe bytes.
- Else → `stdout.write_all`.
- `--color=auto` strips SGR when stdout not a TTY (via `ansi::strip_sgr`).

## Tests

- `tests/golden.rs` + `insta` snapshots under `tests/snapshots/`. Run
  `mise run test-insta`. Review with `cargo insta review`.
- Unit tests inline in `render.rs`, `ansi.rs`, `stream.rs`, `config.rs`.
