# bijjou

A post-processor that takes `NUL` separated fields from custom jj templates (log, etc) and displays them according to bijjou templates.

We leverage jj's template system functionality to write `NUL`-separated key/value strings and pipe the output to bijjou. In general, jj handles the content of the pieces, and bijjou pieces them together.

## Example config

With jj template(s)...

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
    "labels\x00" ++ format_commit_labels(commit),
    "working_copies\x00" ++ commit.working_copies(),
    "bookmarks\x00" ++ commit.bookmarks(),
    "tags\x00" ++ commit.tags(),
    "description\x00" ++ if(commit.description(),
      commit.description().first_line(),
      label("no_desc","no description"),
    ),
  ) ++ "\x1e\n" ++ if(commit.current_working_copy(), diff_numstat(commit)),
)
'''
```

... and bijjou template(s) ...

```
# bijjou config

# each key here matches the value of `bijjou_template_name` from jj
[templates]
# elastic_tab is a bijjou function
# all other values are keys from the jj output
bijjou_log_oneline = '''
  %{elastic_tab(change_id)} %{elastic_tab(commit_id)} %{elastic_tab(author)} %{elastic_tab(timestamp)} %{working_copies} %{bookmarks} %{tags} %{description}'''
```

... we can pipe `jj` into `bijjou`:

```shell
❯ jj log -T bijjou_log_oneline | bijjou

○╶───╴yqpmvt f9f354 ME 260528·2342 bench: compare nom-7 baseline against in-tree parser
@╶───╴qtorvo 3fccae ME 260528·2342 bench: allocation profile for arena vs Box<[u8]>
○╶───╴tlskkw eafc71 ME 260528·1241 HYWC-bench-criterion hydra working copy bench-criterion
𜸩 ○╶─╴zypurq 3b9037 ME 260528·1241 HYWC-cli-subcommands wip(cli): sketch `migrate` subcommand (untested)
𜸨𜸟𜹃
𜸩 ○╶─╴uyuoqm a20d01 ME 260528·1241 HYWC-serde-error-handling hydra working copy serde-error-handling
𜸨𜸟𜹃
𜸩 ○╶─╴pmnrzk b022c8 ME 260528·1241 HYWC-tokio-runtime-upgrade hydra working copy tokio-runtime-upgrade
𜸨𜸟𜹃
○╶───╴utzkux c92053 ME 260528·1241 HYH hydra head
𜸨𜸟𜸠𜸟𜸤
𜸩 𜸩 ○ sxwxzy 53f826 ME 260528·1238 HYS-tokio-runtime-upgrade hydra stack tokio-runtime-upgrade
𜸩 𜸩 ○ tqutxy 041357 ME 260528·1238 feat(runtime): instrument task spawns with tracing spans
𜸩 𜸩 ○ zlykwv 93134c ME 260528·1238 feat(runtime): wire SIGINT/SIGTERM handlers to Shutdown
𜸩 𜸩 ○ mxytsu 66ab8c ME 260528·1238 feat(runtime): add graceful shutdown via broadcast channel
𜸩 𜸩 ○ tvkplq 34e4ad ME 260528·1238 feat(runtime): bootstrap multi-thread tokio runtime
𜸩 ○╶𜸩╴wuspwk 0e2748 ME 260528·1240 HYS-cli-subcommands hydra stack cli-subcommands
𜸩 ○╶𜸩╴uqltss d70c91 ME 260528·1240 docs(cli): flesh out per-subcommand help text and examples
𜸩 ○╶𜸩╴mvnyrx be7296 ME 260528·1240 feat(cli): add `config` get/set subcommands
𜸩 ○╶𜸩╴mnmwkm 4017de ME 260528·1240 feat(cli): add `status` subcommand for task introspection
𜸩 ○╶𜸩╴vtkppq 2c5c59 ME 260528·1240 feat(cli): add `run` subcommand with --watch flag
𜸩 ○╶𜸩╴qpkwzw d62b35 ME 260528·1240 feat(cli): add `init` subcommand to bootstrap config
𜸩 ○╶𜸩╴wrnlyq 60cbd0 ME 260528·1240 feat(cli): scaffold clap parser with global flags
𜸩 𜸨𜸟𜹃
○╶𜸩──╴wymszz 8dc6d7 ME 260528·1241 HYS-bench-criterion hydra stack bench-criterion
○╶𜸩──╴kxqtsw 24f9b7 ME 260528·1241 bench: streaming parser throughput across 1KB..1MB inputs
○╶𜸩──╴vpsrpp 2a9fb8 ME 260528·1241 bench: add criterion harness scaffolding in benches/parser.rs
𜸨𜸟𜹃
◆╶───╴otmzun adb150 ME 260410·1451 HYB main Update A
𜸩
𜹀

❯
```

Color is preserved if enabled from jj and bijjou.

The `elastic_tab()` function aligns the content in a column, and adds a horizontal guide line. You can see this in effect in the change ids: note how they are all aligned.

One output replacement bijjou does is the graph edges which are replaced with something more aesthetic: Large Type Pieces from the [Symbols for Legacy Computing Supplement block](https://en.wikipedia.org/wiki/Symbols_for_Legacy_Computing_Supplement) introduced in Unicode 16.0 ([unicode pdf](https://www.unicode.org/charts/PDF/Unicode-16.0/U160-1CC00.pdf)). You can configure them. More about that below.

Bijjou takes streaming input and by default streams output in batches, either a fixed size (default 128), or in "pager" mode. Pager mode is designed for use with pagers (shocking, I know). It sets the batch size based on screen height to reduce or avoid tears between page down events. Streaming can also be disabled via config. _PS: I recommend [moor](https://github.com/walles/moor). It's great._

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

## Configuration

Precedence (low → high): config file < env vars < CLI flags.

Config file paths (first match wins):

- `$BIJJOU_CONFIG`
- `$XDG_CONFIG_HOME/bijjou/config.toml`
- `$HOME/.config/bijjou/config.toml`

If no file is present, bijjou writes a default one to the XDG path on first run.

Env vars: prefix `BIJJOU__`, replace `.` with `__` and `-` with `_`. Uppercase is canonical, lowercase is accepted too.

```shell
BIJJOU__GRAPH__EDGES__CHARS__HORIZONTAL=X jj log | bijjou
```

CLI flags: `--<key>=<value>`, replace `.` with `__`.

```sh
jj log | bijjou --graph__edges__chars__horizontal=X
```

See [`config.default.toml`](config.default.toml) for every key, default,
and explanatory comment. Quick reference:

| Section               | Keys                                                                                                |
| --------------------- | --------------------------------------------------------------------------------------------------- |
| (top level)           | `activate`, `pager`                                                                                 |
| `[ui]`                | `color` (auto\|always\|never)                                                                       |
| `[layout]`            | `align`, `gap`, `dash`, `dash-arrow`, `dash-margin`                                                 |
| `[filter]`            | `hide-vertical-only-lines`                                                                          |
| `[details]`           | `align-offset`, `diffstat-separator`                                                                |
| `[stream]`            | `enabled`, `batch-size`                                                                             |
| `[commits.markers]`   | `empty`, `divergent`                                                                                |
| `[graph.edges.chars]` | `horizontal`, `vertical`, `top-left`, `top-right`, `bottom-left`, `bottom-right`, `tee-right`, `tee-left`, `tee-down`, `tee-up`, `cross`, `elision` |
| `[colors]`            | `dash-filler`, `edge` (int 0–255 or `"#rrggbb"`)                                    |

Run `bijjou --help` for the same reference inline.

## Development

```sh
mise run build              # release build
mise run test               # all tests
mise run test-unit          # unit tests only
mise run test-insta         # golden snapshot tests
mise run show-golden [name] # render a golden snapshot with ANSI codes live
```

Golden snapshots live under `tests/snapshots/`. After intentional output changes run `cargo insta review` to accept the new versions.
