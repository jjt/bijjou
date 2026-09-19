# bijjou

`bijjou` is a jujutsu (jj) log post-processor. It makes the graph prettier, and allows rearranging fields with optional elastic tabs for columnar alignment.

`bijjou` takes input from `jj log -T bijjou_log_oneline`, where `bijjou_log_oneline` is a custom jj template that emits `NUL`-separated key/value fields. `bijjou` reads its own template to determine the arrangement of fields.

In general, jj is used to style the values in the commit information. `bijjou` arranges them and styles the graph.

## Example config

With this jj template

```toml
# jj config
[template-aliases]
bijjou_log_oneline' = 'bijjou_log_oneline(self)'
bijjou_log_oneline(commit)' = '''
if(commit.root(),
  "root\x00" ++ format_root_commit(commit) ++ "\x1e\n",
  separate("\x00",
    "bijjou_template_name\x00bijjou_log_oneline",  # required for bijjou
    "change_id\x00" ++ format_short_change_id_with_change_offset(commit),
    "commit_id\x00" ++ format_short_commit_id(commit.commit_id()),
    "author\x00" ++ format_short_signature_oneline(commit.author()),
    "timestamp\x00" ++ commit_timestamp(commit).format("%y%m%d·%H%M"),
    "working_copies\x00" ++ commit.working_copies(),
    "bookmarks\x00" ++ commit.bookmarks(),
    "tags\x00" ++ commit.tags(),
    "description\x00" ++ if(commit.description(),
      commit.description().first_line(),
      label("no_desc","no description"),
    ),
  ) ++ "\x1e\n"
)
'''
```

... and this `bijjou` config (note the matching `bijjou_template_name` of `bijjou_log_oneline` from above)

```toml
# bijjou config
[templates]
bijjou_log_oneline = '''
  %{elastic_tab(change_id)} %{elastic_tab(commit_id)} \ 
  %{elastic_tab(author)} %{elastic_tab(timestamp)} \
  %{working_copies} %{bookmarks} %{tags} %{description}'''
```

... we can pipe `jj` into `bijjou` to get this:

```shell
❯ jj log -T bijjou_log_oneline | bijjou

○╶─── yqpmvt f9f354 ME 260528·2342 bench: compare nom-7 baseline against in-tree parser
@╶─── qtorvo 3fccae ME 260528·2342 bench: allocation profile for arena vs Box<[u8]>
○╶─── tlskkw eafc71 ME 260528·1241 HYWC-bench-criterion hydra working copy bench-criterion
𜸩 ○╶─ zypurq 3b9037 ME 260528·1241 HYWC-cli-subcommands wip(cli): sketch `migrate` subcommand (untested)
𜸨𜸟𜹃
𜸩 ○╶─ uyuoqm a20d01 ME 260528·1241 HYWC-serde-error-handling hydra working copy serde-error-handling
𜸨𜸟𜹃
𜸩 ○╶─ pmnrzk b022c8 ME 260528·1241 HYWC-tokio-runtime-upgrade hydra working copy tokio-runtime-upgrade
𜸨𜸟𜹃
○╶─── utzkux c92053 ME 260528·1241 HYH hydra head
𜸨𜸟𜸠𜸟𜸤
𜸩 𜸩 ○ sxwxzy 53f826 ME 260528·1238 HYS-tokio-runtime-upgrade hydra stack tokio-runtime-upgrade
𜸩 𜸩 ○ tqutxy 041357 ME 260528·1238 feat(runtime): instrument task spawns with tracing spans
𜸩 𜸩 ○ zlykwv 93134c ME 260528·1238 feat(runtime): wire SIGINT/SIGTERM handlers to Shutdown
𜸩 𜸩 ○ mxytsu 66ab8c ME 260528·1238 feat(runtime): add graceful shutdown via broadcast channel
𜸩 𜸩 ○ tvkplq 34e4ad ME 260528·1238 feat(runtime): bootstrap multi-thread tokio runtime
𜸩 ○╶𜸩 wuspwk 0e2748 ME 260528·1240 HYS-cli-subcommands hydra stack cli-subcommands
𜸩 ○╶𜸩 uqltss d70c91 ME 260528·1240 docs(cli): flesh out per-subcommand help text and examples
𜸩 ○╶𜸩 mvnyrx be7296 ME 260528·1240 feat(cli): add `config` get/set subcommands
𜸩 ○╶𜸩 mnmwkm 4017de ME 260528·1240 feat(cli): add `status` subcommand for task introspection
𜸩 ○╶𜸩 vtkppq 2c5c59 ME 260528·1240 feat(cli): add `run` subcommand with --watch flag
𜸩 ○╶𜸩 qpkwzw d62b35 ME 260528·1240 feat(cli): add `init` subcommand to bootstrap config
𜸩 ○╶𜸩 wrnlyq 60cbd0 ME 260528·1240 feat(cli): scaffold clap parser with global flags
𜸩 𜸨𜸟𜹃
○╶𜸩── wymszz 8dc6d7 ME 260528·1241 HYS-bench-criterion hydra stack bench-criterion
○╶𜸩── kxqtsw 24f9b7 ME 260528·1241 bench: streaming parser throughput across 1KB..1MB inputs
○╶𜸩── vpsrpp 2a9fb8 ME 260528·1241 bench: add criterion harness scaffolding in benches/parser.rs
𜸨𜸟𜹃
◆╶─── otmzun adb150 ME 260410·1451 HYB main Update A
𜸩
𜹀

❯
```

## Features

### Elastic tabs

Notice the change ids all line up? That's the `elastic_tab()` function. It left-pads the current row so the content that follows lines up in a column across rows. It also adds a horizontal guide line in the gap. `%{elastic_tab(field)}` is shorthand for a tab that a `%{field}` follows. It pads, then emits the field. `bijjou` keys columns by tab position, so each `elastic_tab` in a template is its own column.

### Color
`bijjou` keeps color when jj and `bijjou` both enable it. By default, jj emits no color when a pipe sends its output to a non-TTY process. To keep color, add a config option or use the `--color=always` CLI flag.

### Graph 
Although jj is typically responsible for styled output of each field, `bijjou` makes one output replacement: it rewrites the graph edges into something more ａｅｓｔｈｅｔｉｃ. These are Large Type Pieces from the [Symbols for Legacy Computing Supplement block](https://en.wikipedia.org/wiki/Symbols_for_Legacy_Computing_Supplement), introduced in Unicode 16.0 ([unicode pdf](https://www.unicode.org/charts/PDF/Unicode-16.0/U160-1CC00.pdf)). If you do not like those, you can set the graph edge characters to anything you want. I am not the dad of you.

### Graph collapse
jj draws the graph in fixed two-cell columns. The glyph sits in the first cell. An inter-column gap sits in the second cell (a space, or a horizontal where a connector passes through). Set `graph.collapse = true` to drop those gap cells. Then the graph becomes half as wide, and the dash run to the content shrinks with it:

```shell
❯ jj log -T bijjou_log_oneline | bijjou --graph__collapse=true

○╶─ rppzwpzx c90ec200 jason 260530·0600 HYH hydra head
𜸨𜸠𜸤
𜸩𜸩○ vxqsrzyn 02d310e8 jason 260530·0600 HYS-foo hydra stack foo
𜸩○𜸩 mzpomyto 30e8e07f jason 260530·0600 HYS-baz hydra stack baz
𜸩𜸨𜹃
○𜸩─ wrvuovwm 9d7ea2ce jason 260530·0600 HYS-bar hydra stack bar
𜸨𜹃
```

Only gap cells go. A glyph that is a horizontal because a connector spans several columns (`├───╯`) keeps its cell. An inactive column keeps one of its two spaces. As a result, nothing slides off the column it belongs to.

### Streaming

`bijjou` takes streaming input. By default, it streams output in batches, either a fixed size (default 128) or `half-pager` mode. This mode works with pagers like `less`, `more`, or [`moor`](https://github. It sets the batch size from the screen height (`height/2 - 1`) to reduce or remove tears between page-down and page-up events. The `-1` accommodates the status bar of pagers.

You can also turn off output streaming in the config.

### Hydra topologies

[`hydra`](https://tangled.org/jjt.io/jj-hydra) is a tool to manage jj megamerges. It comprises a series of linear *stacks* as siblings between a base fork point and a head merge point. As a
result, `jj log` gives each stack its own graph column. With `hydra.enable =
true` (the default), `bijjou` marks those columns up. `bijjou` recognizes them by
bookmark name, from the naming in `[hydra.prefixes]`. A row that carries
`HYS-<name>` opens that stack. An anchor (`HYB` / `HYH` / `HYCR`) or a
`HYWC-*` working copy closes it. The rows between keep the stack they sit in,
for as long as they stay in its graph column. A commit drawn in another
column is nobody's stack and keeps jj's own colors. An extra head off the
base below the bottom stack is one example.

- `hydra.top-stack-padding` draws the separator row that jj skips under the
  log's top stack. jj closes a graph column only when the branch to its
  *left* ends. As a result, every stack gets a `├─╯` row under it, except the
  topmost. The topmost stack runs straight into its neighbor.
- `hydra.colors` assigns a color to each stack's graph nodes. `true` hashes the stack
  name into a hue. Hashing based on the stack name results in a stable coloring regardless of toplogy or number of stacks. `false` leaves jj's nodes alone. A list is a palette,
  indexed by the order the log first names each stack (the index wraps). The
  hash skips two reserved bands corresponding to 10° either side of `#a6e3a1` (115°) and
  `#f5c2e7` (316°), the default colors of jj bookmarks and workspaces in Catpuccin Mocha (TODO: make this configurable). As a result, a hashed stack never reads as one of those.
  `bijjou` uses a palette you wrote as given. A stack's `HYWC-<name>` working
  copy takes its stack's color too, so the two read as one thing.
- `hydra.color-bookmarks` puts that color on the bookmark names themselves.
  As a result, `HYS-<name>` and `HYWC-<name>` read in their column's color
  instead of jj's. Other bookmarks on the row, and the anchors, keep jj's
  colors.
- `[hydra.prefixes-replace]` renames those bookmarks in the output, keyed
  like `[hydra.prefixes]`. A set key stands in for the whole leader built out
  of it and keeps the stack name. For example, `base = "◆"` renders `HYB` as
  `◆`, and `stack-head = "Ψ"` renders `HYS-foo` as `Ψfoo` (the dash goes with
  the leader). `prefix` replaces the shared `HY` leader alone, so `HYS-foo`
  reads `ΨS-foo`. A per-bookmark key wins over `prefix`. A key left unset
  leaves the bookmarks it names as jj printed them. Classification still runs
  on the real names. `bijjou` measures the elastic-tab columns on the rendered
  width, so a stand-in of any width stays aligned.

```shell
❯ jj log -T log_oneline | bijjou --graph__collapse=true

𜸩𜸩𜸩● knktkz 478c07 ME 260915·1336 [LOY-734] docs(home): …
𜸩𜸩𜸩𜸩                                        ← top-stack-padding
𜸩𜸩●𜸩 zolnlk 1a5407 ME 260915·1336 HYS-list-prs hydra stack …
𜸩𜸩●𜸩 kknmqn ef4d55 ME 260915·1336 agent/jjt/loy-832-lis…
𜸩𜸩𜸨𜹃
```

A repo with no `hydra` carries no such bookmark, so it gets no markup. The
output is byte-for-byte what `hydra.enable = false` gives. Classification
runs per row off the `bookmarks` field, so there is no subprocess and nothing
to wait on. If the repo renamed its `hydra` bookmarks, spell the new naming in
`[hydra.prefixes]`. Markup needs jj's default top-down log order. `jj log
--reversed` puts a stack's commits above its marker, which the walk cannot
follow.

## Install

From source:

```sh
go install tangled.org/jjt.io/bijjou@latest
```

Or with [mise](https://mise.jdx.dev), which pins the Go toolchain:

```sh
mise run install               # installs to ~/.local/bin/bijjou
BIJJOU_INSTALL_PATH=... mise run install
```

## Configuration

Precedence (low → high): config file < env vars < CLI flags.

Config file paths (first match wins):

- `$BIJJOU_CONFIG`
- `$XDG_CONFIG_HOME/bijjou/config.toml`
- `$HOME/.config/bijjou/config.toml`

If no file is present, `bijjou` writes a default file to the XDG path on first run.

Env vars: prefix `BIJJOU__`, replace `.` with `__` and `-` with `_`. Uppercase is canonical. `bijjou` also accepts lowercase.

```shell
BIJJOU__GRAPH__EDGES__CHARS__HORIZONTAL=X jj log | bijjou
```

CLI flags: `--<key>=<value>`, replace `.` with `__`.

```sh
jj log | bijjou --graph__edges__chars__horizontal=X
```

See [`bijjou-config.toml`](bijjou-config.toml) for every key, default,
and explanatory comment. Quick reference:

| Section               | Keys                                                                                                |
| --------------------- | --------------------------------------------------------------------------------------------------- |
| (top level)           | `activate`, `pager`                                                                                 |
| `[ui]`                | `color` (auto\|always\|never)                                                                       |
| `[layout]`            | `dash`, `dash-start`, `dash-end`                                                                    |
| `[templates]`         | `<name>` (DSL body — a row's `bijjou_template_name` selects one)                                     |
| `[stream]`            | `enabled`, `batch-size` (int or `"half-pager"`)                                                     |
| `[graph]`             | `collapse` (bool — drop the graph's inter-column pad cells)                                          |
| `[graph.edges.chars]` | `horizontal`, `vertical`, `top-left`, `top-right`, `bottom-left`, `bottom-right`, `tee-right`, `tee-left`, `tee-down`, `tee-up`, `cross`, `elision` |
| `[colors]`            | `dash-filler`, `graph-edge` (int 0–255 or `"#rrggbb"`)                              |
| `[hydra]`             | `enable`, `top-stack-padding`, `color-bookmarks` (bool), `colors` (`true`\|`false`\|list of colors)     |
| `[hydra.prefixes]`    | `prefix`, `base`, `head`, `conflict-resolution`, `stack-head`, `stack-working-copy` |

Run `bijjou --help` for the same reference inline.

## Development

```sh
mise run build              # build bin/bijjou
mise run test               # all tests
mise run test-unit          # package unit tests only
mise run test-golden        # golden output tests
mise run show-golden [name] # render a golden file with ANSI codes live
```

Golden files live under `testdata/golden/`. After intentional output changes,
run `mise run test-golden -- -update` to rewrite them, then read the diff.
