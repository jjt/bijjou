// 2026-05-08 Notes on combinations of revision state:
//
// Forbidden pairs:
//   - hidden + working copy — WC always visible (it @, reachable)
//   - hidden + divergent — divergent need ≥2 visible commits same change_id
//
//   All other combos legal. So:
//
//   - conflicted, empty, working copy — orthogonal. Mix any subset freely.
//   - hidden — combine w/ conflicted, empty only.
//   - divergent — combine w/ conflicted, empty, working copy (one of divergent pair can be @).
//
//   Examples valid:
//   - empty working copy (default jj new)
//   - conflicted working copy (rebase conflict in @)
//   - divergent conflicted empty working copy (all four except hidden)
//   - hidden conflicted empty (abandoned dead end)
//
//   Examples invalid:
//   - hidden working copy
//   - hidden divergent (anything)
//
//   Since C,E,WC,I are most common we use the log node:
//   - WC is the hex icon and green
//   - I is a lock icon, takes precedence over WC icon
//   - C is the color red, takes precedence over the WC color
//   - E is a hollow version of the icon

mod ansi;
mod config;
mod output;
mod render;
mod stream;

use std::io::{self, Read, Write};

use crate::config::{cfg, Activate, Config, Pager};
use crate::output::write_output;
use crate::render::{
    compute_diff_stat_groups, contains_bytes, diff_stat_status_rank, emit_line, find_boundary,
    parse_content_columns, parse_diff_stat, strip_trailing_nl, DiffStatRow, Parsed,
};

const HELP: &str = "\
bijjou - jj log post-processor

USAGE
  bijjou [SUBCOMMAND] [OPTIONS] < input

SUBCOMMANDS
  jj-config                 print a TOML snippet for jj's config that wires
                            bijjou's configured node icons into jj's
                            `templates.log_node`. Drop this into your jj
                            config (or a `--config` file).
  jj-graph-node-config      print `--config templates.log_node=...` flags
                            in a form ready for direct `$(...)` expansion:
                              jj log $(bijjou jj-graph-node-config) | bijjou
                            Whitespace inside the TOML values is escaped as
                            `\\u0020` so bash word-splits the line at the
                            argument boundaries only. Pipe through bijjou
                            (or use --no-pager) — jj's builtin pager
                            escapes PUA codepoints as `<U+XXXX>` text.

OPTIONS
  -h, --help                show this help and exit
  --activate[=MODE]         processing mode (auto|always|never); default always; bare flag = always
  --color[=MODE]            color output (auto|always|never); default auto; bare flag = always
  --stream[=BOOL]           streaming mode (default on); bare flag = true; --stream=false disables
  --<key>=<value>           override any config key; replace '.' with '__'

CONFIGURATION
  Precedence (low to high): config file < env vars < CLI flags.

  Config file paths (first match wins):
    $BIJJOU_CONFIG
    $XDG_CONFIG_HOME/bijjou/config.toml
    $HOME/.config/bijjou/config.toml

  Env vars: prefix BIJJOU__, replace '.' with '__' and '-' with '_'.
    Uppercase is the canonical form; lowercase is also accepted.
    e.g. BIJJOU__GRAPH__NODES__CHARS__WORKING_COPY=X

  CLI flags: --<key>=<value>, replace '.' with '__'.
    e.g. --graph__nodes__chars__working-copy=X

  Streaming mode flushes output in batches as input arrives. The first batch
  is pre-scanned so every line in it shares the batch-wide max graph_col.
  Subsequent batches widen monotonically per-line as wider rows arrive, and
  alignment never shifts backwards. In streaming `auto` activation mode the
  marker scan is limited to the first batch; if the marker isn't there, the
  rest of stdin is passed through verbatim.

KEYS
  activate                                  auto|always|never
  activation-marker                         string
  pager                                     auto|always|never

  [ui]
    color                                   auto|always|never

  [filter]
    hide-vertical-only-lines                bool

  [layout]
    align                                   bool (default true)
    gap                                     int >= 0 (default 2)
    dash                                    string
    dash-arrow                              string
    dash-margin                             int >= 0 (default 1)

  [details]
    align-offset                            int >= 0 (default 0)
    diffstat-separator                      string (default \"·\")

  [stream]
    enabled                                 bool (default true)
    batch-size                              int >= 1 (default 128)

  [commits.markers]
    empty                                   string
    divergent                               string

  [graph.nodes.chars]                       string (each)
    working-copy  mutable  immutable  conflict  hidden  fallback
    empty  working-copy-empty  empty-immutable

  [graph.edges.chars]                       string (each)
    horizontal  vertical
    top-left  top-right  bottom-left  bottom-right
    tee-right  tee-left  tee-down  tee-up
    cross  elision

  [colors]                                  int 0-255 | \"#rrggbb\"
    dash-filler  edge
  colors.graph-node                         string (jj color spec, e.g. \"ansi-color-242\" or \"#5f5f5f\");
                                            used in the `jj-config` / `jj-graph-node-config` output

See config.default.toml for defaults and discussion.
";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", HELP);
        return;
    }
    let (subcommand, flag_args) = split_subcommand(argv);
    let mut cfg_obj = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bijjou: {}", e);
            std::process::exit(2);
        }
    };
    if let Err(e) = cfg_obj.apply_env() {
        eprintln!("bijjou: {}", e);
        std::process::exit(2);
    }
    if let Err(e) = cfg_obj.apply_cli(flag_args) {
        eprintln!("bijjou: {}", e);
        std::process::exit(2);
    }
    if let Some(sub) = subcommand {
        match sub.as_str() {
            "jj-config" => {
                print!("{}", render_jj_config(&cfg_obj));
                return;
            }
            "jj-graph-node-config" => {
                println!("{}", render_jj_graph_node_config(&cfg_obj));
                return;
            }
            other => {
                eprintln!("bijjou: unknown subcommand: {}", other);
                std::process::exit(2);
            }
        }
    }
    if cfg_obj.pager == Pager::Always
        && std::env::var("PAGER")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .is_none()
    {
        eprintln!("bijjou: pager = \"always\" but PAGER env var is not set");
        std::process::exit(2);
    }
    config::init(cfg_obj);
    if let Err(e) = run() {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return;
        }
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn split_subcommand(argv: Vec<String>) -> (Option<String>, Vec<String>) {
    let mut sub: Option<String> = None;
    let mut flags = Vec::with_capacity(argv.len());
    for arg in argv {
        if sub.is_none() && !arg.starts_with('-') {
            sub = Some(arg);
        } else {
            flags.push(arg);
        }
    }
    (sub, flags)
}

// Substitute the configured icons into the jj template body the user adds
// to their jj config (or feeds via --config). Icons are wrapped in
// `raw_escape_sequence(...)` so jj's TTY output filter doesn't rewrite
// Private Use Area glyphs (Nerd Font icons) as `<U+XXXX>` text — the
// filter ships with recent jj versions to guard against terminal
// injection from commit text, but trips on legitimate icons we inject
// from config.
//
// Two structural workarounds:
//   - The whole label-and-icon expression sits inside `if(self, …, …)`.
//     For elided revisions self is None, so `self.current_working_copy()`
//     and `self.conflict()` (used inside the label) would raise
//     `<Error: No Commit available>` if evaluated; lifting the !self case
//     to an outer if avoids touching self in that branch.
//   - The icon branches use a nested `if(cond, then, else)` chain rather
//     than `coalesce(...)` because `raw_escape_sequence(...)` reads as
//     null inside coalesce, which would silently fall through every arm.
//
// Keep this body identical to `render_log_node_template_inline` so the
// two subcommands stay in lockstep.
fn render_log_node_template_body(cfg: &Config) -> String {
    format!(
        "if(self,\n  label(\n    separate(\" \",\n      if(self.current_working_copy(), \"working_copy\"),\n      if(self.conflict(), \"conflicted\"),\n      \"graph_node\",\n    ),\n    if(current_working_copy && empty, raw_escape_sequence(\"{wc_empty}\"),\n    if(current_working_copy, raw_escape_sequence(\"{wc}\"),\n    if(immutable && empty, raw_escape_sequence(\"{empty_immutable}\"),\n    if(immutable, raw_escape_sequence(\"{immutable}\"),\n    if(empty, raw_escape_sequence(\"{empty}\"),\n    if(conflict, raw_escape_sequence(\"{conflict}\"),\n       raw_escape_sequence(\"{mutable}\")))))))\n  ),\n  raw_escape_sequence(\"{hidden}\")\n)",
        hidden = cfg.hidden_icon,
        wc_empty = cfg.wc_empty_icon,
        wc = cfg.wc_icon,
        empty_immutable = cfg.empty_immutable_icon,
        immutable = cfg.immutable_icon,
        empty = cfg.empty_icon,
        conflict = cfg.conflict_icon,
        mutable = cfg.mutable_icon,
    )
}

fn render_log_node_template_inline(cfg: &Config) -> String {
    format!(
        "if(self, label(separate(\" \", if(self.current_working_copy(), \"working_copy\"), if(self.conflict(), \"conflicted\"), \"graph_node\"), if(current_working_copy && empty, raw_escape_sequence(\"{wc_empty}\"), if(current_working_copy, raw_escape_sequence(\"{wc}\"), if(immutable && empty, raw_escape_sequence(\"{empty_immutable}\"), if(immutable, raw_escape_sequence(\"{immutable}\"), if(empty, raw_escape_sequence(\"{empty}\"), if(conflict, raw_escape_sequence(\"{conflict}\"), raw_escape_sequence(\"{mutable}\")))))))), raw_escape_sequence(\"{hidden}\"))",
        hidden = cfg.hidden_icon,
        wc_empty = cfg.wc_empty_icon,
        wc = cfg.wc_icon,
        empty_immutable = cfg.empty_immutable_icon,
        immutable = cfg.immutable_icon,
        empty = cfg.empty_icon,
        conflict = cfg.conflict_icon,
        mutable = cfg.mutable_icon,
    )
}

// Multi-line TOML snippet ready to drop into a jj config file.
fn render_jj_config(cfg: &Config) -> String {
    format!(
        "colors.graph_node = \"{}\"\ntemplates.log_node = '''\n{}\n'''\n",
        cfg.graph_node_color,
        render_log_node_template_body(cfg),
    )
}

// Wrap a string as a TOML basic string (double-quoted) with every char
// that would tokenize under bash word-splitting (whitespace) encoded as a
// `\uXXXX` escape. jj's TOML parser turns the escapes back into the
// original chars, so the produced token survives `$(…)` substitution as a
// single argument — no shell quoting and no `eval` needed.
fn toml_basic_string_space_safe(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // Whitespace must be escaped so $(...) word-splitting does not
            // chop a value in half.   (space) is the common case;
            // tabs/newlines/CRs are escaped here too for safety.
            ' ' => out.push_str("\\u0020"),
            '\t' => out.push_str("\\u0009"),
            '\n' => out.push_str("\\u000A"),
            '\r' => out.push_str("\\u000D"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// Single-line `--config KEY=VAL --config KEY=VAL` ready for direct shell
// substitution: `jj log $(bijjou jj-graph-node-config) | bijjou`. Values
// are TOML basic strings whose whitespace is `\u`-escaped so the line
// word-splits into exactly four args regardless of icon contents.
fn render_jj_graph_node_config(cfg: &Config) -> String {
    format!(
        "--config templates.log_node={} --config colors.graph_node={}",
        toml_basic_string_space_safe(&render_log_node_template_inline(cfg)),
        toml_basic_string_space_safe(&cfg.graph_node_color),
    )
}

fn split_lines(input: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, &b) in input.iter().enumerate() {
        if b == b'\n' {
            lines.push(&input[start..=i]);
            start = i + 1;
        }
    }
    if start < input.len() {
        lines.push(&input[start..]);
    }
    lines
}

// Within each contiguous run of diff-stat rows, reorder by status group
// (A, D, M, R, C); stable within each status so file order from the input is
// preserved. Reorders `lines`, `parsed`, and `diff_stat` together so all
// three stay aligned.
fn reorder_diff_stat_groups<'a>(
    lines: &mut [&'a [u8]],
    parsed: &mut [Option<Parsed>],
    diff_stat: &mut [Option<DiffStatRow>],
) {
    let mut i = 0;
    while i < diff_stat.len() {
        if diff_stat[i].is_none() {
            i += 1;
            continue;
        }
        let start = i;
        while i < diff_stat.len() && diff_stat[i].is_some() {
            i += 1;
        }
        let end = i;
        if end - start < 2 {
            continue;
        }
        let mut order: Vec<usize> = (start..end).collect();
        order.sort_by_key(|&j| {
            diff_stat_status_rank(diff_stat[j].as_ref().unwrap().letter_byte)
        });
        let taken_lines: Vec<&[u8]> = (start..end).map(|j| lines[j]).collect();
        let mut taken_parsed: Vec<Option<Parsed>> =
            (start..end).map(|j| parsed[j].take()).collect();
        let mut taken_diff: Vec<Option<DiffStatRow>> =
            (start..end).map(|j| diff_stat[j].take()).collect();
        for (offset, &orig_j) in order.iter().enumerate() {
            let k = orig_j - start;
            lines[start + offset] = taken_lines[k];
            parsed[start + offset] = taken_parsed[k].take();
            diff_stat[start + offset] = taken_diff[k].take();
        }
    }
}

fn run() -> io::Result<()> {
    let c = cfg();
    if c.activate == Activate::Never {
        let mut stdin = io::stdin().lock();
        let mut stdout = io::stdout().lock();
        io::copy(&mut stdin, &mut stdout)?;
        return stdout.flush();
    }
    if c.stream_enabled {
        return stream::run();
    }

    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    if c.activate == Activate::Auto {
        let marker = c.activation_marker.as_bytes();
        if !contains_bytes(&input, marker) {
            let mut out = io::stdout().lock();
            out.write_all(&input)?;
            out.flush()?;
            return Ok(());
        }
    }

    // Non-streaming path: one global max over the whole input, applied to
    // every line. Early narrow rows get dash filler stretching to the widest
    // point of the log. Streaming mode (stream.rs) diverges intentionally:
    // it widens per-line and can't backfill once a row is emitted.
    let mut lines = split_lines(&input);
    let mut parsed: Vec<Option<Parsed>> = lines
        .iter()
        .map(|line| find_boundary(strip_trailing_nl(line).0))
        .collect();
    let mut diff_stat: Vec<Option<DiffStatRow>> = lines
        .iter()
        .map(|line| parse_diff_stat(strip_trailing_nl(line).0))
        .collect();
    reorder_diff_stat_groups(&mut lines, &mut parsed, &mut diff_stat);
    let group_widths = compute_diff_stat_groups(&diff_stat);

    let max_graph = parsed
        .iter()
        .filter_map(|p| p.as_ref().map(|p| p.graph_col))
        .max()
        .unwrap_or(0);

    let (max_cid_w, max_auth_w) = lines
        .iter()
        .zip(parsed.iter())
        .filter_map(|(line, p)| {
            let p = p.as_ref()?;
            let body = strip_trailing_nl(line).0;
            parse_content_columns(&body[p.content_start..])
        })
        .fold((0usize, 0usize), |(c, a), cols| {
            (c.max(cols.changeid_width), a.max(cols.author_width))
        });

    let mut out: Vec<u8> = Vec::with_capacity(input.len() + lines.len() * 8);
    let mut emitted_lines = 0usize;

    for (idx, (line, p)) in lines.iter().zip(parsed.iter()).enumerate() {
        let target_col = if c.align_enabled {
            max_graph + c.align_gap
        } else {
            p.as_ref().map(|p| p.graph_col).unwrap_or(0) + c.align_gap
        };
        let ds_arg = match (diff_stat[idx].as_ref(), group_widths[idx]) {
            (Some(row), Some((ml, mr))) => Some((row, ml, mr)),
            _ => None,
        };
        if emit_line(
            line,
            p.as_ref(),
            ds_arg,
            target_col,
            max_cid_w,
            max_auth_w,
            &mut out,
        ) {
            emitted_lines += 1;
        }
    }

    write_output(&out, emitted_lines)
}
