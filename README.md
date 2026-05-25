# bijjou

A post-processor for `jj log` output. Rewrites graph glyphs, dims edges,
aligns content to a uniform column, and optionally fills the gap with a
dash run pointing at each commit for easier alignment of node to commit information.

Bijjou reads stdin, processes it, and writes to stdout. It only touches
graph rows — the rest of the stream is passed through byte-for-byte.

## What it does

- Replaces jj's box-drawing graph chars (`│ ╭ ╮ ─` …) with Unicode 16 Large
  Type Pieces by default, or any glyph you configure.
- Aligns commit content across rows so every change id starts at the same
  column. Dims edges and fills the gap between the graph and content with
  a dash run (and optional arrow) for easier eye-tracking.
- Replaces `jj diff --summary` status letters (`M A D R C`) with Nerd Font
  icons, optionally colorizing the path to match. Strips `{}` braces from
  rename/copy lines.
- Streams output as input arrives, or processes the whole input at once.

### Node icons

Bijjou does not rewrite jj's node glyphs (`@ ○ ◆ × ●`) at render time —
jj's own template owns that. Run `bijjou jj-config` to emit a TOML
snippet for your jj config that wires bijjou's `[graph.nodes.chars]`
into `templates.log_node`:

```sh
bijjou jj-config >> ~/.config/jj/config.toml
```

Or splice the same template inline via shell substitution:

```sh
jj log $(bijjou jj-graph-node-config) --color=always | bijjou
```

(whitespace inside the TOML values is `\u`-escaped so `$(…)` word-splits
the line at the argument boundaries only — no `eval` needed.)

The pipe to `bijjou` matters even if you don't want the other
post-processing. Without it, jj launches its builtin pager
(`sapling-streampager`), which treats every codepoint whose
`unicode-width` is 0 (most Private Use Area glyphs, including Nerd
Font icons) as "unprintable" and renders it as the literal text
`<U+XXXX>`. Piping to bijjou makes jj's stdout a pipe, which suppresses
the pager and lets the raw bytes reach the terminal. If you really
need jj's output directly, pass `--no-pager` (or set
`ui.paginate = "never"`).

Any icon configured in `[graph.nodes.chars]` is also recognized as a
node when bijjou parses input, so the alignment math stays correct when
you swap defaults.

## Requirements

- Nerd Font for the default node icons.
- A terminal that renders Unicode 16 Large Type Pieces for the default
  edge glyphs (or override them, see config).

## Install

From source:

```sh
cargo install --path .
```

Or with [mise](https://mise.jdx.dev):

```sh
mise run install               # installs to ~/.local/bin/bijjou
BIJJOU_INSTALL_PATH=... mise run install
```

## Use

Pipe `jj log` through bijjou:

```sh
jj log --color=always | bijjou
```

By default bijjou is in `always` mode: it processes every line of input.
Set `--activate=auto` to gate processing on the presence of the activation
marker (`BIJJOU_ACTIVATE`) in stdin, or `--activate=never` to force
byte-for-byte passthrough.

Color output defaults to `auto` (emit when stdout is a terminal, strip
otherwise). Override with `--color=always` or `--color=never`, or set
`[ui] color = "..."` in the config file.

bijjou detects jj's native `(empty)` and `(divergent)` log annotations out
of the box — no jj config required. Setting `ui.color = "always"` is
optional and only useful if you want jj's color choices preserved when
output is piped (bijjou itself locates the graph column by codepoint, not
by ANSI). See
[`examples/jj-config-snippet.toml`](examples/jj-config-snippet.toml) for a
minimal snippet.

### Streaming

Streaming is on by default: bijjou flushes batches as input arrives. Graph
width is tracked across the whole stream and grows monotonically —
alignment never shifts backwards. Disable with `--stream=false` to fall
back to the buffered path, which aligns every line to the widest graph
column in the input at the cost of waiting for EOF. See the comment in
[`config.default.toml`](config.default.toml) for the trade-off.

## Configuration

Precedence (low → high): config file < env vars < CLI flags.

Config file paths (first match wins):

- `$BIJJOU_CONFIG`
- `$XDG_CONFIG_HOME/bijjou/config.toml`
- `$HOME/.config/bijjou/config.toml`

If no file is present, bijjou writes a default one to the XDG path on
first run.

Env vars: prefix `BIJJOU__`, replace `.` with `__` and `-` with `_`.
Uppercase is canonical; lowercase is accepted too.

```sh
BIJJOU__GRAPH__NODES__CHARS__WORKING_COPY=X jj log | bijjou
```

CLI flags: `--<key>=<value>`, replace `.` with `__`.

```sh
jj log | bijjou --graph__nodes__chars__working-copy=X
```

See [`config.default.toml`](config.default.toml) for every key, default,
and explanatory comment. Quick reference:

| Section               | Keys                                                                                                |
| --------------------- | --------------------------------------------------------------------------------------------------- |
| (top level)           | `activate`, `activation-marker`, `pager`                                                            |
| `[ui]`                | `color` (auto\|always\|never)                                                                       |
| `[layout]`            | `align`, `gap`, `dash`, `dash-arrow`, `dash-margin`                                                 |
| `[filter]`            | `hide-vertical-only-lines`                                                                          |
| `[details]`           | `align-offset`                                                                                      |
| `[stream]`            | `enabled`, `batch-size`                                                                             |
| `[commits.markers]`   | `empty`, `divergent`                                                                                |
| `[graph.nodes.chars]` | `working-copy`, `mutable`, `immutable`, `conflict`, `hidden`, `fallback`, `empty`, `working-copy-empty`, `empty-immutable` |
| `[graph.edges.chars]` | `horizontal`, `vertical`, `top-left`, `top-right`, `bottom-left`, `bottom-right`, `tee-right`, `tee-left`, `tee-down`, `tee-up`, `cross`, `elision` |
| `[colors]`            | `dash-filler`, `edge` (int 0–255 or `"#rrggbb"`); `graph-node` (jj color spec, e.g. `"ansi-color-242"`)                                    |

Run `bijjou --help` for the same reference inline.

## Development

```sh
mise run build              # release build
mise run test               # all tests
mise run test-unit          # unit tests only
mise run test-insta         # golden snapshot tests
mise run show-golden [name] # render a golden snapshot with ANSI codes live
```

Golden snapshots live under `tests/snapshots/`. After intentional output
changes run `cargo insta review` to accept the new versions.
