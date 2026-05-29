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
mod dsl;
mod output;
mod render;
mod stream;

use std::collections::HashMap;
use std::io::{self, Read, Write};

use crate::ansi::{skip_csi, FG_RESET};
use crate::config::{cfg, Activate, Config, Pager, BIJJOU_TEMPLATE_NAME_FIELD};
use crate::dsl::{collect_widths, parse_json_oneline, parse_nul_oneline, render_row, LeftSide, Node, Template};
use crate::output::write_output;
use crate::render::{contains_bytes, emit_dim_graph, emit_line, find_boundary, strip_trailing_nl};

pub enum RowKind {
    Commit {
        graph_end: usize,
        graph_col: usize,
        last_is_edge: bool,
        template_name: Option<String>,
        fields: HashMap<String, Vec<u8>>,
    },
    Root {
        graph_end: usize,
        value: Vec<u8>,
    },
    Passthrough,
}

// A `templates.<name>` entry compiled at startup. `Empty` carries no template
// body — the row's content is dropped, only the graph prefix is emitted.
pub enum CompiledTemplate {
    Empty,
    Parsed(Template),
}

pub fn compile_templates(
    map: &HashMap<String, String>,
) -> Result<HashMap<String, CompiledTemplate>, String> {
    let mut out = HashMap::with_capacity(map.len());
    for (name, body) in map {
        let entry = if body.is_empty() {
            CompiledTemplate::Empty
        } else {
            let tpl = Template::parse(body).map_err(|e| format!("templates.{}: {}", name, e))?;
            CompiledTemplate::Parsed(tpl)
        };
        out.insert(name.clone(), entry);
    }
    Ok(out)
}

pub fn classify_row(body: &[u8]) -> RowKind {
    let Some(p) = find_boundary(body) else {
        return RowKind::Passthrough;
    };
    let payload = &body[p.content_start..];
    let mut i = 0;
    while i < payload.len() {
        if let Some(after) = skip_csi(payload, i) {
            i = after;
            continue;
        }
        if matches!(payload[i], b' ' | b'\t') {
            i += 1;
            continue;
        }
        break;
    }
    let rest = &payload[i..];
    // NUL/RS-framed record: terminator `\x1e` is the format marker. Try
    // this first; fall back to JSON if not present.
    let mut fields = if rest.contains(&0x1E) {
        let Some(f) = parse_nul_oneline(rest) else {
            return RowKind::Passthrough;
        };
        f
    } else if !rest.is_empty() && rest[0] == b'{' {
        let Some(f) = parse_json_oneline(rest) else {
            return RowKind::Passthrough;
        };
        f
    } else {
        return RowKind::Passthrough;
    };
    if fields.len() == 1 && fields.contains_key("root") {
        let value = fields.get("root").cloned().unwrap_or_default();
        return RowKind::Root {
            graph_end: p.graph_end,
            value,
        };
    }
    let template_name = fields
        .remove(BIJJOU_TEMPLATE_NAME_FIELD)
        .and_then(|v| String::from_utf8(v).ok());
    RowKind::Commit {
        graph_end: p.graph_end,
        graph_col: p.graph_col,
        last_is_edge: p.last_is_edge,
        template_name,
        fields,
    }
}

pub fn emit_classified(
    line: &[u8],
    row: &RowKind,
    templates: &HashMap<String, CompiledTemplate>,
    widths: &HashMap<String, HashMap<String, usize>>,
    max_graph_col: usize,
    out: &mut Vec<u8>,
) {
    let (body, trailing_nl) = strip_trailing_nl(line);
    match row {
        RowKind::Commit {
            graph_end,
            graph_col,
            last_is_edge,
            template_name,
            fields,
        } => {
            emit_dim_graph(&body[..*graph_end], out);
            // Pass the graph→content gap through to `render_row` as a
            // leading ws segment so it participates in rules 1-3
            // (collapse on empty fields, dash-fill across adjacent
            // whitespace) alongside the template's own whitespace.
            let leading_pad = max_graph_col.saturating_sub(*graph_col);
            let leading_left = if *last_is_edge {
                LeftSide::GraphEdge
            } else {
                LeftSide::GraphNode
            };
            let Some(name) = template_name.as_deref() else {
                // Row parsed as fields but carried no `bijjou_template_name` —
                // we have nothing to render with. Pass the rest of the line
                // through verbatim instead of dropping the payload.
                out.extend_from_slice(&body[*graph_end..]);
                if trailing_nl {
                    out.push(b'\n');
                }
                return;
            };
            match templates.get(name) {
                Some(CompiledTemplate::Empty) => {
                    // Configured but empty — render the graph only and drop
                    // the rest of the row.
                }
                Some(CompiledTemplate::Parsed(template)) => {
                    let empty_widths: HashMap<String, usize> = HashMap::new();
                    let w = widths.get(name).unwrap_or(&empty_widths);
                    render_row(template, fields, leading_pad, leading_left, w, out);
                }
                None => {
                    emit_missing_template(name, leading_pad, leading_left, out);
                }
            }
        }
        RowKind::Root { graph_end, value } => {
            emit_dim_graph(&body[..*graph_end], out);
            crate::dsl::emit_pad_public(2, out);
            out.extend_from_slice(value);
        }
        RowKind::Passthrough => {
            let parsed = find_boundary(body);
            // emit_line owns trailing newline handling for the passthrough
            // branch, so return early without our own \n append.
            emit_line(line, parsed.as_ref(), out);
            return;
        }
    }
    if trailing_nl {
        out.push(b'\n');
    }
}

// Render a single-row notice for a row whose `bijjou_template_name` doesn't
// match any configured `templates.<name>`. The message is wrapped in the
// dim SGR pair so it renders in bright black, matching the graph filler.
fn emit_missing_template(name: &str, leading_pad: usize, leading_left: LeftSide, out: &mut Vec<u8>) {
    let c = cfg();
    let mut bytes = Vec::with_capacity(c.dim_on.len() + 32 + name.len() + FG_RESET.len());
    bytes.extend_from_slice(&c.dim_on);
    bytes.extend_from_slice(b"no bijjou template for ");
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(FG_RESET);
    let synth = Template {
        nodes: vec![Node::Literal(b" ".to_vec()), Node::Field("__bijjou_msg".to_string())],
    };
    let mut fields: HashMap<String, Vec<u8>> = HashMap::with_capacity(1);
    fields.insert("__bijjou_msg".to_string(), bytes);
    render_row(&synth, &fields, leading_pad, leading_left, &HashMap::new(), out);
}

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
  log-oneline-json          print a jj log template expression that emits
                            one JSON object per commit, with ANSI color
                            sequences preserved inside the string values.
                            Ready for direct `$(...)` expansion:
                              jj log -T $(bijjou log-oneline-json)
                            All whitespace outside string literals is
                            stripped; intra-string spaces are encoded as
                            jj's `\\x20` byte escape so bash word-splits
                            the output into a single argument.

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

  [layout]
    dash                                    string
    dash-start                              string
    dash-end                                string

  [templates]
    <name>                                  DSL string (see config.default.toml).
                                            Each row's `bijjou_template_name`
                                            field selects `templates.<name>`.

  [stream]
    enabled                                 bool (default true)
    batch-size                              int >= 1 (default 128)

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
            "log-oneline-json" => {
                println!("{}", render_log_oneline_json_inline());
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
        "templates.log_node = '''\n{}\n'''\n",
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

// Single-line `--config KEY=VAL` ready for direct shell substitution:
// `jj log $(bijjou jj-graph-node-config) | bijjou`. The value is a TOML
// basic string whose whitespace is `\u`-escaped so the line word-splits
// into exactly two args regardless of icon contents.
fn render_jj_graph_node_config(cfg: &Config) -> String {
    format!(
        "--config templates.log_node={}",
        toml_basic_string_space_safe(&render_log_node_template_inline(cfg)),
    )
}

// One-line jj template that emits a JSON object per commit. Root commit
// emits `{"root":"..."}`; every other commit emits one field per item
// from the original `separate(...)` block in `log_oneline_json.toml`.
//
// ANSI escape sequences from `format_short_*` helpers are preserved
// verbatim inside the JSON string values (raw ESC bytes, not ``
// escapes), so a terminal that consumes the output gets colored
// rendering. A `replace(...)` call wraps each value, encoding `"`, `\`,
// and `\n` (raw newline) as JSON escape sequences — other bytes,
// including ESC, pass through unchanged.
//
// Output has no whitespace outside string literals so it survives
// unquoted `$(...)` substitution. The one literal space in the source
// — inside `"no description"` — is encoded as jj's `\x20` byte escape.
fn render_log_oneline_json_inline() -> String {
    let jc = |value: &str| -> String {
        let mut s = String::with_capacity(value.len() + 96);
        s.push_str(r#"'"'++replace(regex:"[\"\\\\\\n]","#);
        s.push_str(value);
        s.push_str(r#",|m|if(m.get(0)=="\"","\\\"",if(m.get(0)=="\\","\\\\","\\n")))++'"'"#);
        s
    };
    let fields: &[(&str, &str)] = &[
        (r#"'"change_id":'"#, "format_short_change_id_with_change_offset(self)"),
        (r#"'"commit_id":'"#, "format_short_commit_id(self.commit_id())"),
        (r#"'"author":'"#, "format_short_signature_oneline(self.author())"),
        (r#"'"timestamp":'"#, r#"commit_timestamp(self).format("%y%m%d·%H%M")"#),
        (r#"'"labels":'"#, "format_commit_labels(self)"),
        (r#"'"working_copies":'"#, "self.working_copies()"),
        (r#"'"bookmarks":'"#, "self.bookmarks()"),
        (r#"'"tags":'"#, "self.tags()"),
    ];
    let mut out = String::new();
    out.push_str("if(self.root(),");
    out.push_str(r#"'{"root":'++"#);
    out.push_str(&jc("format_root_commit(self)"));
    out.push_str(r#"++'}'++"\n",'{'++separate(',',"#);
    let mut first = true;
    for (key, val) in fields {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(key);
        out.push_str("++");
        out.push_str(&jc(val));
    }
    out.push(',');
    out.push_str(r#"if(config("ui.show-cryptographic-signatures").as_boolean(),'"signature":'++"#);
    out.push_str(&jc("format_short_cryptographic_signature(self.signature())"));
    out.push_str("),");
    out.push_str(r#"if(self.empty(),'"empty":'++"#);
    out.push_str(&jc("empty_commit_marker"));
    out.push_str("),");
    out.push_str(r#"'"description":'++if(self.description(),"#);
    out.push_str(&jc("self.description().first_line()"));
    out.push(',');
    out.push_str(&jc("label(\"no_desc\",\"no\\x20description\")"));
    out.push_str(r#"))++'}'++"\n")"#);
    out
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

    let templates = compile_templates(&c.templates)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let lines = split_lines(&input);
    let rows: Vec<RowKind> = lines
        .iter()
        .map(|l| classify_row(strip_trailing_nl(l).0))
        .collect();

    let mut widths: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut max_graph_col = 0usize;
    for row in &rows {
        if let RowKind::Commit {
            graph_col,
            template_name,
            fields,
            ..
        } = row
        {
            if let Some(name) = template_name.as_deref() {
                if let Some(CompiledTemplate::Parsed(template)) = templates.get(name) {
                    let entry = widths.entry(name.to_string()).or_default();
                    collect_widths(template, fields, *graph_col, entry);
                }
            }
            if *graph_col > max_graph_col {
                max_graph_col = *graph_col;
            }
        }
    }

    let mut out: Vec<u8> = Vec::with_capacity(input.len() + lines.len() * 16);
    for (line, row) in lines.iter().zip(rows.iter()) {
        emit_classified(line, row, &templates, &widths, max_graph_col, &mut out);
    }
    write_output(&out, lines.len())
}
