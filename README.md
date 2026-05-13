# bijjou

A post-processor for `jj log` output. Rewrites graph glyphs, dims edges,
aligns content to a uniform column, and optionally fills the gap with a
dash run pointing at each commit for easier alignment of node to commit information.

Bijjou reads stdin, processes it, and writes to stdout. It only touches
graph rows — the rest of the stream is passed through byte-for-byte.

## What it does

- Replaces jj's box-drawing graph chars (`│ ╭ ╮ ─` …) with Unicode 16 Large
  Type Pieces by default, or any glyph you configure.
- Maps commit-node chars (`@ ○ ◆ × ●`) to Nerd Font icons.
- Dims edges with one color and mutable/immutable nodes with another, while
  preserving jj's original color for the working copy and conflict nodes.
- Aligns commit content across rows so every change id starts at the same
  column.
- Streams output as input arrives, or processes the whole input at once.

## Requirements

- Nerd Font for the default node icons.
- A terminal that renders Unicode 16 Large Type Pieces for the default
  edge glyphs (or override them, see config).
- `jj` configured to emit ANSI color (`ui.color = "always"`), since bijjou
  uses the color codes to find the graph column.

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

By default bijjou is in `auto` mode: it processes input only if it sees
the activation marker (`BIJJOU_ACTIVATE`) somewhere in the stream, and
passes input through unchanged otherwise. Force-on with `--activate` or
`--activate=always`; force-off with `--activate=never`.

To wire bijjou into the empty / immutable detection, copy
[`examples/jj-config-snippet.toml`](examples/jj-config-snippet.toml) into
your jj config. It sets `ui.color = "always"` and adds a template that
emits the marker characters bijjou strips back out.

### Streaming

For long logs or live tailing, enable streaming:

```sh
jj log --color=always | bijjou --stream
```

Streaming flushes batches as input arrives. Graph width is tracked across
the whole stream and grows monotonically — alignment never shifts
backwards. See the comment in `examples/bijjou-config.example.toml` for
the trade-off versus the non-streaming path.

## Configuration

Precedence (low → high): config file < env vars < CLI flags.

Config file paths (first match wins):

- `$BIJJOU_CONFIG`
- `$XDG_CONFIG_HOME/bijjou/config.toml`
- `$HOME/.config/bijjou/config.toml`

If no file is present, bijjou writes a default one to the XDG path on
first run.

Env vars: prefix `BIJJOU__`, replace `.` with `__`.

```sh
BIJJOU__graph__nodes__chars__working-copy=X jj log | bijjou
```

CLI flags: `--<key>=<value>`, replace `.` with `__`.

```sh
jj log | bijjou --graph__nodes__chars__working-copy=X
```

See [`examples/bijjou-config.example.toml`](examples/bijjou-config.example.toml)
for every key, default, and explanatory comment. Quick reference:

| Section               | Keys                                                                                                |
| --------------------- | --------------------------------------------------------------------------------------------------- |
| (top level)           | `activate`, `activation-marker`                                                                     |
| `[layout]`            | `align`, `gap`, `dash`, `dash-arrow`, `dash-margin`                                                 |
| `[filter]`            | `hide-vertical-only-lines`                                                                          |
| `[stream]`            | `enabled`, `batch-size`                                                                             |
| `[commits.markers]`   | `empty`, `immutable`                                                                                |
| `[graph.nodes.chars]` | `working-copy`, `mutable`, `immutable`, `conflict`, `alternate`, `empty`, `working-copy-empty`, `empty-immutable` |
| `[graph.edges.chars]` | `horizontal`, `vertical`, `top-left`, `top-right`, `bottom-left`, `bottom-right`, `tee-right`, `tee-left`, `tee-down`, `tee-up`, `cross`, `elision` |
| `[colors]`            | `dash-filler`, `edge`, `mutable-node` (int 0–255 or `"#rrggbb"`)                                    |

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
