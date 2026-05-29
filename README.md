# bijjou

A post-processor that takes `NUL` separated fields from custom jj templates
(log, etc) and displays them according to bijjou templates.

We leverage jj's template system functionality to write pieces of 

## What it does

- Replaces jj's box-drawing graph chars (`│ ╭ ╮ ─` …) with Unicode 16 Large
  Type Pieces by default, or any glyph you configure.
- Takes `NUL` separated fields and values from a jj template and provides a
  simple templating language and a handful of layout functions
- Provides `elastic_tab` for fields that vertically aligns the contents
- Adds dashes between the graph nodes and the commit information 
- Accepts streaming input and streams output in batches (see below for details)

## What it doesn't

Bijjou does not replicate functionality of jj's templating system. For instance
logic to colourize the shortest prefix of a change id.

The general pattern is to emit a `NUL` separated hash map of keys with strings
from a jj template and then write a bijjou template that lays them out.

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

## Streaming

Accepts streaming input. Emits streaming output, based on a static `batch-size`
or based on the screen height (# of rows). The latter is intended for use
with a pager, assuming that the pager's status line is a single row.

The batch size for the first page is `<rows> - 1`, and subsequent pages are 
`(<rows> - 1) / 2`, since page down/up seems to move by half screens.

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
| `[details]`           | `align-offset`, `diffstat-separator`                                                                |
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
