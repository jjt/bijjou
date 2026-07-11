mod ansi;
mod config;
mod dsl;
mod output;
mod render;
mod stream;

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::OnceLock;

use crate::ansi::{skip_csi, FG_RESET};
use crate::config::{cfg, Activate, Config, Pager, BIJJOU_TEMPLATE_NAME_FIELD};
use crate::dsl::{collect_anchors, parse_nul_oneline, render_row, LeftSide, Node, Template};
use crate::output::write_output;
use crate::render::{
    contains_bytes, emit_dim_graph, emit_line, find_boundary, strip_trailing_nl, Parsed,
};

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
    // Carries the graph boundary already located by `classify_row` (None when
    // the line has no graph prefix at all) so `emit_classified` need not
    // re-run `find_boundary`.
    Passthrough {
        parsed: Option<Parsed>,
    },
}

// A `templates.<name>` entry compiled at startup. `Empty` carries no template
// body — the row's content is dropped, only the graph prefix is emitted.
pub enum CompiledTemplate {
    Empty,
    Parsed(Template),
}

// Per-template alignment state. `anchors[i]` is the row-wide max natural
// column before the i-th elastic_tab in the template (tabs keyed by
// left-to-right order). Grows monotonically as rows are scanned.
#[derive(Default)]
pub struct TemplateMetrics {
    pub anchors: Vec<usize>,
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
        return RowKind::Passthrough { parsed: None };
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
    // NUL/RS-framed record: a `\x1e` terminator is the format marker.
    if !rest.contains(&0x1E) {
        return RowKind::Passthrough { parsed: Some(p) };
    }
    let Some(mut fields) = parse_nul_oneline(rest) else {
        return RowKind::Passthrough { parsed: Some(p) };
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

// Pass-1 accumulation shared by the buffered and streaming paths: fold every
// commit row's elastic-tab anchors into `metrics` and widen `max_graph_col`.
// Anchors only grow (collect_anchors takes maxima), so calling this across
// successive streaming batches widens monotonically and never invalidates
// rows already emitted above.
pub fn accumulate_metrics(
    rows: &[RowKind],
    templates: &HashMap<String, CompiledTemplate>,
    metrics: &mut HashMap<String, TemplateMetrics>,
    max_graph_col: &mut usize,
) {
    for row in rows {
        if let RowKind::Commit {
            graph_col,
            template_name,
            fields,
            ..
        } = row
        {
            if let Some(name) = template_name.as_deref() {
                if let Some(CompiledTemplate::Parsed(template)) = templates.get(name) {
                    let entry = metrics.entry(name.to_string()).or_default();
                    collect_anchors(template, fields, &mut entry.anchors);
                }
            }
            if *graph_col > *max_graph_col {
                *max_graph_col = *graph_col;
            }
        }
    }
}

pub fn emit_classified(
    line: &[u8],
    row: &RowKind,
    templates: &HashMap<String, CompiledTemplate>,
    metrics: &HashMap<String, TemplateMetrics>,
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
                    let m = metrics.get(name).unwrap_or_else(|| empty_metrics());
                    render_row(
                        template,
                        fields,
                        leading_pad,
                        leading_left,
                        &m.anchors,
                        out,
                    );
                }
                None => {
                    emit_missing_template(name, leading_pad, leading_left, out);
                }
            }
        }
        RowKind::Root { graph_end, value } => {
            emit_dim_graph(&body[..*graph_end], out);
            crate::dsl::emit_node_pad(2, out);
            out.extend_from_slice(value);
        }
        RowKind::Passthrough { parsed } => {
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
    let m = empty_metrics();
    render_row(
        &synth,
        &fields,
        leading_pad,
        leading_left,
        &m.anchors,
        out,
    );
}

// A shared, allocation-free empty metrics table for the no-recorded-anchors
// path (a template whose only row is the one being rendered now, or the
// synthetic missing-template notice).
fn empty_metrics() -> &'static TemplateMetrics {
    static EMPTY: OnceLock<TemplateMetrics> = OnceLock::new();
    EMPTY.get_or_init(TemplateMetrics::default)
}

const HELP: &str = "\
bijjou - jj log post-processor

USAGE
  bijjou [OPTIONS] < input

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
    e.g. graph.edges.chars.horizontal -> BIJJOU__GRAPH__EDGES__CHARS__HORIZONTAL=X
         layout.dash-start            -> BIJJOU__LAYOUT__DASH_START=<
         activate                     -> BIJJOU__ACTIVATE=auto

  CLI flags: --<key>=<value>, replace '.' with '__' (hyphens are kept as-is).
    e.g. graph.edges.chars.horizontal -> --graph__edges__chars__horizontal=X
         layout.dash-start            -> --layout__dash-start=<
         templates.log_oneline        -> --templates__log_oneline='...'

  Streaming mode flushes output in batches as input arrives. The first batch
  is pre-scanned so every line in it shares the batch-wide max graph_col.
  Subsequent batches widen monotonically per-line as wider rows arrive, and
  alignment never shifts backwards. In streaming `auto` activation mode the
  scan for the `bijjou_template_name` field is limited to the first batch;
  if it isn't there, the rest of stdin is passed through verbatim.

KEYS
  activate                                  auto|always|never
  pager                                     auto|always|never

  [ui]
    color                                   auto|always|never

  [layout]
    dash                                    string
    dash-start                              string
    dash-end                                string

  [templates]
    <name>                                  DSL string (see bijjou-config.toml).
                                            Each row's `bijjou_template_name`
                                            field selects `templates.<name>`.

  [stream]
    enabled                                 bool (default true)
    batch-size                              int >= 1 (default 128)

  [graph.edges.chars]                       string (each)
    horizontal  vertical
    top-left  top-right  bottom-left  bottom-right
    tee-right  tee-left  tee-down  tee-up
    cross  elision

  [colors]                                  int 0-255 | \"#rrggbb\"
    dash-filler  graph-edge

See bijjou-config.toml for defaults and discussion.
";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", HELP);
        return;
    }
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
    if let Err(e) = cfg_obj.apply_cli(argv) {
        eprintln!("bijjou: {}", e);
        std::process::exit(2);
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

    if c.activate == Activate::Auto
        && !contains_bytes(&input, BIJJOU_TEMPLATE_NAME_FIELD.as_bytes())
    {
        let mut out = io::stdout().lock();
        out.write_all(&input)?;
        out.flush()?;
        return Ok(());
    }

    let templates = compile_templates(&c.templates)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let lines = split_lines(&input);
    let rows: Vec<RowKind> = lines
        .iter()
        .map(|l| classify_row(strip_trailing_nl(l).0))
        .collect();

    let mut metrics: HashMap<String, TemplateMetrics> = HashMap::new();
    let mut max_graph_col = 0usize;
    accumulate_metrics(&rows, &templates, &mut metrics, &mut max_graph_col);

    let mut out: Vec<u8> = Vec::with_capacity(input.len() + lines.len() * 16);
    for (line, row) in lines.iter().zip(rows.iter()) {
        emit_classified(line, row, &templates, &metrics, max_graph_col, &mut out);
    }
    write_output(&out)
}
