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
    contains_bytes, emit_line, find_boundary, parse_content_columns, strip_trailing_nl, Parsed,
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
                            ready for shell substitution, e.g.
                            `jj log $(bijjou jj-graph-node-config) | bijjou`.

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
    diffsummary-path-color                  int 0-255 | \"#rrggbb\" | \"original\" (default 15)
    align-offset                            int >= 0 (default 0)

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
// to their jj config (or feeds via --config). The body is identical to the
// one in `render_jj_graph_node_config` so the two subcommands stay in
// lockstep — keep them aligned when editing.
fn render_log_node_template_body(cfg: &Config) -> String {
    format!(
        "label(\n  separate(\" \",\n    if(self.current_working_copy(), \"working_copy\"),\n    if(self.conflict(), \"conflicted\"),\n    \"graph_node\",\n  ),\n  coalesce(\n    if(!self, \"{hidden}\"),\n    if(current_working_copy && empty, \"{wc_empty}\"),\n    if(current_working_copy, \"{wc}\"),\n    if(immutable && empty, \"{empty_immutable}\"),\n    if(immutable, \"{immutable}\"),\n    if(empty, \"{empty}\"),\n    if(conflict, \"{conflict}\"),\n    \"{mutable}\",\n  )\n)",
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
        "label(separate(\" \", if(self.current_working_copy(), \"working_copy\"), if(self.conflict(), \"conflicted\"), \"graph_node\"), coalesce(if(!self, \"{hidden}\"), if(current_working_copy && empty, \"{wc_empty}\"), if(current_working_copy, \"{wc}\"), if(immutable && empty, \"{empty_immutable}\"), if(immutable, \"{immutable}\"), if(empty, \"{empty}\"), if(conflict, \"{conflict}\"), \"{mutable}\"))",
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

// Single-line `--config` arguments suitable for `$(bijjou jj-graph-node-config)`
// in a `jj log` invocation.
fn render_jj_graph_node_config(cfg: &Config) -> String {
    format!(
        "--config templates.log_node='{}' --config colors.graph_node='{}'",
        render_log_node_template_inline(cfg),
        cfg.graph_node_color,
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
    let lines = split_lines(&input);
    let parsed: Vec<Option<Parsed>> = lines
        .iter()
        .map(|line| find_boundary(strip_trailing_nl(line).0))
        .collect();

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

    for (line, p) in lines.iter().zip(parsed.iter()) {
        let target_col = if c.align_enabled {
            max_graph + c.align_gap
        } else {
            p.as_ref().map(|p| p.graph_col).unwrap_or(0) + c.align_gap
        };
        if emit_line(
            line,
            p.as_ref(),
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
