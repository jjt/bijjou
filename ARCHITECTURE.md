# Architecture

`bijjou` = stdin/stdout filter. Post-process `jj log` output: rewrite edge
glyphs and dim edges. Everything past the graph prefix passes through
byte-for-byte. Non-graph lines pass through byte-for-byte. Node glyphs
themselves are owned by jj's template (see `bijjou jj-config`) — bijjou
recognizes them but never rewrites them.

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
| `render.rs`  | Core. Parse line → `Parsed{graph_col, graph_end, content_start}`, recognize edges (box-drawing) and nodes (jj defaults + configured icons), emit dimmed edges. Node bytes (and their surrounding ANSI) are forwarded unchanged — node coloring is jj's job via the `graph_node` label set by the template. Bytes past the graph prefix are copied verbatim. |
| `stream.rs`  | Batched reader, per-line emit, `OutputSink` (stdout or piped pager)                     |
| `output.rs`  | Buffered path's terminal write / pager spawn                                              |

## Render flow per line

1. `find_boundary` → locate end of graph prefix. A position is "graph"
   if its codepoint is in the box-drawing range, is the elision char, is
   one of jj's default node chars (`@ ○ ◆ × ●`), or matches the first
   codepoint of any configured `[graph.nodes.chars]` icon.
2. `emit_dim_graph` → rewrite edges via `map_graph_char`, paint them with
   `colors.edge`, and strip jj's edge fg color. Space runs between graph
   chars (once a node has appeared on the line) are filled with the
   configured `layout.dash`, with `layout.dash-start` capping the run
   when it abuts a node. Node bytes (and their surrounding ANSI) are
   forwarded unchanged.
3. Bytes from `graph_end` to end of line are copied byte-for-byte.

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

## Config surface

Single global `OnceLock<Config>` via `cfg()`. Three merge layers:

1. `Config::load` → read TOML from `$BIJJOU_CONFIG` | XDG | `~/.config/...`.
   Writes default on first run.
2. `apply_env` → `BIJJOU__SECTION__KEY=VAL`.
3. `apply_cli` → `--key__sub=val`.

Keys: top-level (`activate`, `pager`, `activation-marker`),
`[ui]`, `[layout]`, `[stream]`,
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
