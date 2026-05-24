# Architecture

`bijjou` = stdin/stdout filter. Post-process `jj log` output: rewrite edge
glyphs, dim edges, align content column, optional dash filler. Non-graph
lines pass through byte-for-byte. Node glyphs themselves are owned by jj's
template (see `bijjou jj-config`) — bijjou recognizes them but never
rewrites them.

## Pipeline

```
stdin → activation check → (stream | buffered) → render → output sink → stdout|pager
```

- **Activate gate** (`Activate::Never|Auto|Always`): `Never` = raw copy.
  `Auto` = look for `activation-marker` in input, else passthrough.
- **Stream vs buffered**: `[stream].enabled` switches paths.
  - Stream (`stream.rs`): read in batches, widen graph width monotonically
    per line, flush as input arrives. Cannot backfill.
  - Buffered (`main.rs::run`): slurp stdin, compute global max graph col +
    max changeid/author width, emit one pass. Aligns every line to widest
    column at cost of EOF wait.

## Modules

| File         | Job                                                                                      |
| ------------ | ---------------------------------------------------------------------------------------- |
| `main.rs`    | Arg parse, config load chain, dispatch buffered path                                      |
| `config.rs`  | Config struct, TOML/env/CLI merge, global `cfg()`. Precedence file < env < CLI            |
| `ansi.rs`    | Byte-level ANSI utils: CSI skip, UTF-8 decode, SGR filter/strip                          |
| `render.rs`  | Core. Parse line → `Parsed{graph_col, content_start}`, recognize edges (box-drawing) and nodes (jj defaults + configured icons), emit dimmed edges + dash filler + status icons; nodes pass through verbatim |
| `stream.rs`  | Batched reader, per-line widen, `OutputSink` (stdout or piped pager)                     |
| `output.rs`  | Buffered path's terminal write / pager spawn                                              |

## Render flow per line

1. `find_boundary` → locate end of graph prefix. A position is "graph"
   if its codepoint is in the box-drawing range, is the elision char, is
   one of jj's default node chars (`@ ○ ◆ × ●`), or matches the first
   codepoint of any configured `[graph.nodes.chars]` icon.
2. `parse_content_columns` → extract changeid + author widths for padding.
3. `emit_dim_graph` → rewrite edges via `map_graph_char`, paint them with
   `colors.edge`, and strip jj's edge fg color. Strips `(empty)` /
   `(divergent)` / `(conflict)` markers in passing. Node bytes (and their
   surrounding ANSI) are forwarded unchanged — node coloring is jj's job
   via the `graph_node` label set by the template.
4. `write_gap` → pad spaces (+ optional dash run, optional arrow) up to
   `max_graph + layout.gap`.
5. `write_padded_content` → align changeid/author columns. Status-summary
   lines get Nerd Font M/A/D/R/C icons + optional path color.

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

`[graph.nodes.chars].fallback` is a config-only key: not emitted by the
default template, but recognized as a node icon in input so a custom
template that emits a different glyph still parses correctly.

## Config surface

Single global `OnceLock<Config>` via `cfg()`. Three merge layers:

1. `Config::load` → read TOML from `$BIJJOU_CONFIG` | XDG | `~/.config/...`.
   Writes default on first run.
2. `apply_env` → `BIJJOU__SECTION__KEY=VAL`.
3. `apply_cli` → `--key__sub=val`.

Keys: top-level (`activate`, `pager`, `activation-marker`),
`[ui]`, `[layout]`, `[filter]`, `[details]`, `[stream]`,
`[commits.markers]`, `[graph.nodes.chars]`, `[graph.edges.chars]`,
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
