use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const DEFAULT_DASH: &str = "─";
pub const DEFAULT_DASH_START: &str = "╶";
// Closing cell of a dash run — the cell abutting the content. Empty by
// default so the content always has a plain space to its left. Set it to a
// glyph (`╴` U+2574, the half-line matching `dash-start`) to cap the run
// against the content instead.
pub const DEFAULT_DASH_END: &str = "";
pub const DEFAULT_DIM_ON: &[u8] = b"\x1b[38;5;8m";
pub const DEFAULT_EDGE_DIM_ON: &[u8] = b"\x1b[38;5;8m";
pub const DEFAULT_GRAPH_HORIZONTAL: &str = "𜸟";
pub const DEFAULT_GRAPH_VERTICAL: &str = "𜸩";
pub const DEFAULT_GRAPH_TOP_LEFT: &str = "𜸚";
pub const DEFAULT_GRAPH_TOP_RIGHT: &str = "𜸤";
pub const DEFAULT_GRAPH_BOTTOM_LEFT: &str = "𜸾";
pub const DEFAULT_GRAPH_BOTTOM_RIGHT: &str = "𜹃";
pub const DEFAULT_GRAPH_TEE_RIGHT: &str = "𜸨";
pub const DEFAULT_GRAPH_TEE_LEFT: &str = "𜸶";
pub const DEFAULT_GRAPH_TEE_DOWN: &str = "𜸠";
pub const DEFAULT_GRAPH_TEE_UP: &str = "𜹀";
pub const DEFAULT_GRAPH_CROSS: &str = "𜸺";
pub const DEFAULT_GRAPH_ELISION: &str = "𜹀";
pub const DEFAULT_STREAM_BATCH_SIZE: usize = 128;
pub const DEFAULT_LOG_ONELINE_NAME: &str = "log_oneline";
pub const DEFAULT_LOG_ONELINE_BODY: &str = " %{elastic_tab(change_id)} %{elastic_tab(commit_id)} %{elastic_tab(author)} %{elastic_tab(timestamp)} %{working_copies} %{bookmarks} %{tags} %{description}";
pub const BIJJOU_TEMPLATE_NAME_FIELD: &str = "bijjou_template_name";
pub const DEFAULT_HYDRA_PREFIX: &str = "HY";
pub const DEFAULT_HYDRA_BASE: &str = "B";
pub const DEFAULT_HYDRA_HEAD: &str = "H";
pub const DEFAULT_HYDRA_CONFLICT_RESOLUTION: &str = "CR";
pub const DEFAULT_HYDRA_STACK_HEAD: &str = "S";
pub const DEFAULT_HYDRA_STACK_WORKING_COPY: &str = "WC";

// `hydra.colors`: leave the nodes alone, hash each stack's name into a
// colour, or index an explicit palette by the stack's position in the graph
// (top of the log first, wrapping when there are more stacks than colours).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HydraColors {
    Off,
    Hash,
    Palette(Vec<Vec<u8>>),
}

// `hydra.prefixes`: the bookmark naming this repo's hydra uses, so a row is
// classified from its `bookmarks` field alone. `hydra status --toml` reports
// the same naming, but it shells out to jj several times per log, which costs
// more than every other thing bijjou does put together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HydraPrefixes {
    // Shared leader of every hydra bookmark: `HY`.
    pub prefix: String,
    // Anchor suffixes: `HYB`, `HYH`, `HYCR`.
    pub base: String,
    pub head: String,
    pub conflict_resolution: String,
    // Per-stack suffixes, each followed by `-<stack name>`: `HYS-foo`,
    // `HYWC-foo`.
    pub stack_head: String,
    pub stack_working_copy: String,
}

impl Default for HydraPrefixes {
    fn default() -> Self {
        Self {
            prefix: DEFAULT_HYDRA_PREFIX.to_string(),
            base: DEFAULT_HYDRA_BASE.to_string(),
            head: DEFAULT_HYDRA_HEAD.to_string(),
            conflict_resolution: DEFAULT_HYDRA_CONFLICT_RESOLUTION.to_string(),
            stack_head: DEFAULT_HYDRA_STACK_HEAD.to_string(),
            stack_working_copy: DEFAULT_HYDRA_STACK_WORKING_COPY.to_string(),
        }
    }
}

impl HydraPrefixes {
    // `HYS-` — the marker bookmark that opens a stack.
    pub fn stack_marker(&self) -> String {
        format!("{}{}-", self.prefix, self.stack_head)
    }

    // `HYWC-` — a stack's working copy, which sits outside the stack.
    pub fn working_copy(&self) -> String {
        format!("{}{}-", self.prefix, self.stack_working_copy)
    }

    // `HYB` / `HYH` / `HYCR` — whole bookmark names, matched exactly.
    pub fn anchors(&self) -> Vec<String> {
        [&self.base, &self.head, &self.conflict_resolution]
            .iter()
            .map(|suffix| format!("{}{}", self.prefix, suffix))
            .collect()
    }

    // What `HYS-` reads as: the per-bookmark key if set, else the `HY`
    // leader replaced and the rest of the marker kept, else nothing.
    pub fn stack_marker_replace(&self, r: &HydraPrefixReplace) -> Option<String> {
        replace_leader(&r.stack_head, &r.prefix, &format!("{}-", self.stack_head))
    }

    // What `HYWC-` reads as, under the same rule.
    pub fn working_copy_replace(&self, r: &HydraPrefixReplace) -> Option<String> {
        replace_leader(
            &r.stack_working_copy,
            &r.prefix,
            &format!("{}-", self.stack_working_copy),
        )
    }

    // What `HYB` / `HYH` / `HYCR` read as, in the order `anchors` returns
    // them.
    pub fn anchors_replace(&self, r: &HydraPrefixReplace) -> Vec<Option<String>> {
        [
            (&r.base, &self.base),
            (&r.head, &self.head),
            (&r.conflict_resolution, &self.conflict_resolution),
        ]
        .into_iter()
        .map(|(whole, suffix)| replace_leader(whole, &r.prefix, suffix))
        .collect()
    }
}

// `hydra.prefixes-replace`: what a hydra bookmark reads as once rendered,
// keyed the same way as `hydra.prefixes`. A set key stands in for the leader
// `hydra.prefixes` builds out of it, and the name behind that leader is kept:
// `base = "◆"` renders `HYB` as `◆`, `stack-head = "Ψ"` renders `HYS-foo` as
// `Ψfoo` — the dash belongs to the leader, so it goes with it. `prefix`
// replaces the shared `HY` leader only, so `prefix = "Ψ"` renders `HYS-foo`
// as `ΨS-foo`; a per-bookmark key wins over it. A key left unset leaves the
// bookmarks it names exactly as jj printed them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HydraPrefixReplace {
    pub prefix: Option<String>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub conflict_resolution: Option<String>,
    pub stack_head: Option<String>,
    pub stack_working_copy: Option<String>,
}

// One bookmark's rendered leader: the whole-leader replacement if that key
// is set, else `prefix`'s replacement with the rest of the leader kept
// behind it, else `None` — that bookmark is left alone.
fn replace_leader(whole: &Option<String>, prefix: &Option<String>, rest: &str) -> Option<String> {
    match (whole, prefix) {
        (Some(whole), _) => Some(whole.clone()),
        (None, Some(lead)) => Some(format!("{}{}", lead, rest)),
        (None, None) => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchSize {
    Fixed(usize),
    HalfPager,
}

impl Default for BatchSize {
    fn default() -> Self {
        BatchSize::Fixed(DEFAULT_STREAM_BATCH_SIZE)
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Activate {
    Auto,
    #[default]
    Always,
    Never,
}

pub fn parse_activate(s: &str) -> Result<Activate, String> {
    match s {
        "auto" => Ok(Activate::Auto),
        "always" => Ok(Activate::Always),
        "never" => Ok(Activate::Never),
        other => Err(format!("expected auto|always|never, got {:?}", other)),
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Pager {
    #[default]
    Auto,
    Always,
    Never,
}

pub fn parse_pager(s: &str) -> Result<Pager, String> {
    match s {
        "auto" => Ok(Pager::Auto),
        "always" => Ok(Pager::Always),
        "never" => Ok(Pager::Never),
        other => Err(format!("expected auto|always|never, got {:?}", other)),
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Auto,
    Always,
    Never,
}

pub fn parse_color(s: &str) -> Result<Color, String> {
    match s {
        "auto" => Ok(Color::Auto),
        "always" => Ok(Color::Always),
        "never" => Ok(Color::Never),
        other => Err(format!("expected auto|always|never, got {:?}", other)),
    }
}

pub struct Config {
    pub dash: String,
    pub dash_start: String,
    pub dash_end: String,
    pub graph_horizontal: String,
    pub graph_vertical: String,
    pub graph_top_left: String,
    pub graph_top_right: String,
    pub graph_bottom_left: String,
    pub graph_bottom_right: String,
    pub graph_tee_right: String,
    pub graph_tee_left: String,
    pub graph_tee_down: String,
    pub graph_tee_up: String,
    pub graph_cross: String,
    pub graph_elision: String,
    pub graph_collapse: bool,
    pub dim_on: Vec<u8>,
    pub edge_dim_on: Vec<u8>,
    pub activate: Activate,
    pub pager: Pager,
    pub color: Color,
    pub stream_enabled: bool,
    pub stream_batch_size: BatchSize,
    pub debug_force_screen_height: Option<usize>,
    pub templates: HashMap<String, String>,
    pub hydra_enable: bool,
    pub hydra_top_stack_padding: bool,
    pub hydra_colors: HydraColors,
    pub hydra_color_bookmarks: bool,
    pub hydra_prefixes: HydraPrefixes,
    pub hydra_prefixes_replace: HydraPrefixReplace,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dash: DEFAULT_DASH.to_string(),
            dash_start: DEFAULT_DASH_START.to_string(),
            dash_end: DEFAULT_DASH_END.to_string(),
            graph_horizontal: DEFAULT_GRAPH_HORIZONTAL.to_string(),
            graph_vertical: DEFAULT_GRAPH_VERTICAL.to_string(),
            graph_top_left: DEFAULT_GRAPH_TOP_LEFT.to_string(),
            graph_top_right: DEFAULT_GRAPH_TOP_RIGHT.to_string(),
            graph_bottom_left: DEFAULT_GRAPH_BOTTOM_LEFT.to_string(),
            graph_bottom_right: DEFAULT_GRAPH_BOTTOM_RIGHT.to_string(),
            graph_tee_right: DEFAULT_GRAPH_TEE_RIGHT.to_string(),
            graph_tee_left: DEFAULT_GRAPH_TEE_LEFT.to_string(),
            graph_tee_down: DEFAULT_GRAPH_TEE_DOWN.to_string(),
            graph_tee_up: DEFAULT_GRAPH_TEE_UP.to_string(),
            graph_cross: DEFAULT_GRAPH_CROSS.to_string(),
            graph_elision: DEFAULT_GRAPH_ELISION.to_string(),
            graph_collapse: false,
            dim_on: DEFAULT_DIM_ON.to_vec(),
            edge_dim_on: DEFAULT_EDGE_DIM_ON.to_vec(),
            activate: Activate::default(),
            pager: Pager::default(),
            color: Color::default(),
            stream_enabled: true,
            stream_batch_size: BatchSize::default(),
            debug_force_screen_height: None,
            templates: {
                let mut m = HashMap::new();
                m.insert(
                    DEFAULT_LOG_ONELINE_NAME.to_string(),
                    DEFAULT_LOG_ONELINE_BODY.to_string(),
                );
                m
            },
            hydra_enable: true,
            hydra_top_stack_padding: true,
            hydra_colors: HydraColors::Hash,
            hydra_color_bookmarks: true,
            hydra_prefixes: HydraPrefixes::default(),
            hydra_prefixes_replace: HydraPrefixReplace::default(),
        }
    }
}

fn flatten_toml(
    prefix: &str,
    table: &toml::Table,
    out: &mut Vec<(String, String)>,
) -> Result<(), String> {
    for (k, v) in table {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{}.{}", prefix, k)
        };
        match v {
            toml::Value::Table(t) => flatten_toml(&key, t, out)?,
            toml::Value::String(s) => out.push((key, s.clone())),
            toml::Value::Integer(i) => out.push((key, i.to_string())),
            toml::Value::Boolean(b) => out.push((key, b.to_string())),
            toml::Value::Float(f) => out.push((key, f.to_string())),
            // A list of scalars flattens to the comma-joined form the env and
            // CLI layers have to spell it in anyway, so one syntax covers
            // every layer.
            toml::Value::Array(items) => out.push((key.clone(), join_scalars(&key, items)?)),
            toml::Value::Datetime(_) => return Err(format!("{}: datetimes not supported", key)),
        }
    }
    Ok(())
}

fn join_scalars(key: &str, items: &[toml::Value]) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        match item {
            toml::Value::String(s) => parts.push(s.clone()),
            toml::Value::Integer(i) => parts.push(i.to_string()),
            toml::Value::Boolean(b) => parts.push(b.to_string()),
            toml::Value::Float(f) => parts.push(f.to_string()),
            _ => return Err(format!("{}: array items must be scalars", key)),
        }
    }
    Ok(parts.join(","))
}

fn parse_bool_str(s: &str) -> Result<bool, String> {
    match s {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("expected true|false, got {:?}", other)),
    }
}

// Env-var convention: uppercase, `__` separates config-path segments
// (becomes `.`), and single `_` becomes `-`. The lowercase / hyphenated
// form is accepted unchanged.
fn env_key_to_config_key(name: &str) -> String {
    name.replace("__", ".").replace('_', "-").to_lowercase()
}

fn parse_batch_size(s: &str) -> Result<BatchSize, String> {
    if s == "half-pager" {
        return Ok(BatchSize::HalfPager);
    }
    let n: i64 = s
        .parse()
        .map_err(|_| format!("expected integer >= 1 or \"half-pager\", got {:?}", s))?;
    if n < 1 {
        return Err(format!("expected integer >= 1, got {}", n));
    }
    Ok(BatchSize::Fixed(n as usize))
}

fn parse_color_str(s: &str) -> Result<Vec<u8>, String> {
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return Err(format!("expected #rrggbb, got {:?}", s));
        }
        let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| format!("bad hex: {:?}", s))?;
        let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| format!("bad hex: {:?}", s))?;
        let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| format!("bad hex: {:?}", s))?;
        return Ok(format!("\x1b[38;2;{};{};{}m", r, g, b).into_bytes());
    }
    match s.parse::<i64>() {
        Ok(n) if (0..=255).contains(&n) => Ok(format!("\x1b[38;5;{}m", n).into_bytes()),
        Ok(n) => Err(format!("expected 0-255, got {}", n)),
        Err(_) => Err(format!("expected integer or \"#rrggbb\", got {:?}", s)),
    }
}

fn parse_hydra_colors(s: &str) -> Result<HydraColors, String> {
    match s {
        "true" => return Ok(HydraColors::Hash),
        "false" => return Ok(HydraColors::Off),
        _ => {}
    }
    let mut palette = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("empty color in palette {:?}", s));
        }
        palette.push(parse_color_str(part)?);
    }
    Ok(HydraColors::Palette(palette))
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, String> {
        let mut cfg = Self::default();
        let table: toml::Table = s.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let mut kvs: Vec<(String, String)> = Vec::new();
        flatten_toml("", &table, &mut kvs)?;
        for (k, v) in &kvs {
            cfg.apply_kv(k, v, "")?;
        }
        Ok(cfg)
    }

    pub fn apply_env(&mut self) -> Result<(), String> {
        let mut vars: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k.starts_with("BIJJOU__"))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in vars {
            let key = env_key_to_config_key(&k["BIJJOU__".len()..]);
            self.apply_kv(&key, &v, "env")?;
        }
        Ok(())
    }

    pub fn apply_cli<I: IntoIterator<Item = String>>(&mut self, args: I) -> Result<(), String> {
        for arg in args {
            if arg == "--activate" {
                self.apply_kv("activate", "always", "cli")?;
                continue;
            }
            if arg == "--stream" {
                self.apply_kv("stream.enabled", "true", "cli")?;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--stream=") {
                self.apply_kv("stream.enabled", value, "cli")?;
                continue;
            }
            if arg == "--color" {
                self.apply_kv("ui.color", "always", "cli")?;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--color=") {
                self.apply_kv("ui.color", value, "cli")?;
                continue;
            }
            let rest = arg
                .strip_prefix("--")
                .ok_or_else(|| format!("cli: unknown argument: {}", arg))?;
            let (raw_key, value) = rest
                .split_once('=')
                .ok_or_else(|| format!("cli: missing value for --{}", rest))?;
            let key = raw_key.replace("__", ".");
            self.apply_kv(&key, value, "cli")?;
        }
        Ok(())
    }

    fn apply_kv(&mut self, key: &str, value: &str, src: &str) -> Result<(), String> {
        let prefix = if src.is_empty() {
            String::new()
        } else {
            format!("{}: ", src)
        };
        let mkerr = |e: String| format!("{}{}: {}", prefix, key, e);
        match key {
            "activate" => self.activate = parse_activate(value).map_err(mkerr)?,
            "pager" => self.pager = parse_pager(value).map_err(mkerr)?,
            "ui.color" => self.color = parse_color(value).map_err(mkerr)?,
            "graph.edges.chars.horizontal" => self.graph_horizontal = value.to_string(),
            "graph.edges.chars.vertical" => self.graph_vertical = value.to_string(),
            "graph.edges.chars.top-left" => self.graph_top_left = value.to_string(),
            "graph.edges.chars.top-right" => self.graph_top_right = value.to_string(),
            "graph.edges.chars.bottom-left" => self.graph_bottom_left = value.to_string(),
            "graph.edges.chars.bottom-right" => self.graph_bottom_right = value.to_string(),
            "graph.edges.chars.tee-right" => self.graph_tee_right = value.to_string(),
            "graph.edges.chars.tee-left" => self.graph_tee_left = value.to_string(),
            "graph.edges.chars.tee-down" => self.graph_tee_down = value.to_string(),
            "graph.edges.chars.tee-up" => self.graph_tee_up = value.to_string(),
            "graph.edges.chars.cross" => self.graph_cross = value.to_string(),
            "graph.edges.chars.elision" => self.graph_elision = value.to_string(),
            "graph.collapse" => self.graph_collapse = parse_bool_str(value).map_err(mkerr)?,
            "layout.dash" => self.dash = value.to_string(),
            "layout.dash-start" => self.dash_start = value.to_string(),
            "layout.dash-end" => self.dash_end = value.to_string(),
            "stream.enabled" => self.stream_enabled = parse_bool_str(value).map_err(mkerr)?,
            "stream.batch-size" => {
                self.stream_batch_size = parse_batch_size(value).map_err(mkerr)?;
            }
            k if k.starts_with("templates.") => {
                let name = &k["templates.".len()..];
                if name.is_empty() || name.contains('.') {
                    return Err(mkerr(format!(
                        "expected `templates.<name>` with a flat name, got {:?}",
                        k
                    )));
                }
                self.templates.insert(name.to_string(), value.to_string());
            }
            "colors.dash-filler" => self.dim_on = parse_color_str(value).map_err(mkerr)?,
            "colors.graph-edge" => self.edge_dim_on = parse_color_str(value).map_err(mkerr)?,
            "hydra.enable" => self.hydra_enable = parse_bool_str(value).map_err(mkerr)?,
            "hydra.top-stack-padding" => {
                self.hydra_top_stack_padding = parse_bool_str(value).map_err(mkerr)?;
            }
            "hydra.colors" => self.hydra_colors = parse_hydra_colors(value).map_err(mkerr)?,
            "hydra.color-bookmarks" => {
                self.hydra_color_bookmarks = parse_bool_str(value).map_err(mkerr)?;
            }
            "hydra.prefixes.prefix" => self.hydra_prefixes.prefix = value.to_string(),
            "hydra.prefixes.base" => self.hydra_prefixes.base = value.to_string(),
            "hydra.prefixes.head" => self.hydra_prefixes.head = value.to_string(),
            "hydra.prefixes.conflict-resolution" => {
                self.hydra_prefixes.conflict_resolution = value.to_string();
            }
            "hydra.prefixes.stack-head" => self.hydra_prefixes.stack_head = value.to_string(),
            "hydra.prefixes.stack-working-copy" => {
                self.hydra_prefixes.stack_working_copy = value.to_string();
            }
            "hydra.prefixes-replace.prefix" => {
                self.hydra_prefixes_replace.prefix = Some(value.to_string());
            }
            "hydra.prefixes-replace.base" => {
                self.hydra_prefixes_replace.base = Some(value.to_string());
            }
            "hydra.prefixes-replace.head" => {
                self.hydra_prefixes_replace.head = Some(value.to_string());
            }
            "hydra.prefixes-replace.conflict-resolution" => {
                self.hydra_prefixes_replace.conflict_resolution = Some(value.to_string());
            }
            "hydra.prefixes-replace.stack-head" => {
                self.hydra_prefixes_replace.stack_head = Some(value.to_string());
            }
            "hydra.prefixes-replace.stack-working-copy" => {
                self.hydra_prefixes_replace.stack_working_copy = Some(value.to_string());
            }
            "debug.force-screen-height" => {
                let n: i64 = value
                    .parse()
                    .map_err(|_| mkerr(format!("expected integer >= 1, got {:?}", value)))?;
                if n < 1 {
                    return Err(mkerr(format!("expected integer >= 1, got {}", n)));
                }
                self.debug_force_screen_height = Some(n as usize);
            }
            other => return Err(format!("{}unknown key: {}", prefix, other)),
        }
        Ok(())
    }

    pub fn load() -> Result<Self, String> {
        let Some(path) = config_path() else {
            return Ok(Self::default());
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => {
                if std::env::var("BIJJOU_CONFIG").is_ok() {
                    return Ok(Self::default());
                }
                match create_default_config() {
                    Some(p) => std::fs::read_to_string(&p).unwrap_or_default(),
                    None => return Ok(Self::default()),
                }
            }
        };
        Self::from_toml(&content).map_err(|e| format!("{}: {}", path.display(), e))
    }
}

const EXAMPLE_CONFIG: &str = include_str!("../bijjou-config.toml");

fn default_config_target() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("XDG_CONFIG_HOME") {
        if !v.is_empty() {
            return Some(PathBuf::from(v).join("bijjou").join("config.toml"));
        }
    }
    let h = std::env::var("HOME").ok().filter(|s| !s.is_empty())?;
    Some(
        PathBuf::from(h)
            .join(".config")
            .join("bijjou")
            .join("config.toml"),
    )
}

fn create_default_config() -> Option<PathBuf> {
    let target = default_config_target()?;
    if let Some(parent) = target.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("bijjou: failed to create {}: {}", parent.display(), e);
            return None;
        }
    }
    if let Err(e) = std::fs::write(&target, EXAMPLE_CONFIG) {
        eprintln!("bijjou: failed to write {}: {}", target.display(), e);
        return None;
    }
    eprintln!("bijjou: created default config at {}", target.display());
    Some(target)
}

fn config_path() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("BIJJOU_CONFIG") {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var("HOME").ok().filter(|s| !s.is_empty())?;
            PathBuf::from(home).join(".config")
        }
    };
    Some(base.join("bijjou").join("config.toml"))
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn cfg() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

pub fn color_enabled() -> bool {
    use std::io::IsTerminal;
    match cfg().color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => std::io::stdout().is_terminal(),
    }
}

pub fn init(c: Config) {
    let _ = CONFIG.set(c);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_parses_section_string_int() {
        let s = r#"
[graph.edges.chars]
horizontal = "X"

[colors]
graph-edge = 200
"#;
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.graph_horizontal, "X");
        assert_eq!(cfg.edge_dim_on, b"\x1b[38;5;200m".to_vec());
    }

    #[test]
    fn toml_color_hex_string() {
        let s = "[colors]\ngraph-edge = \"#aabbcc\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.edge_dim_on, b"\x1b[38;2;170;187;204m".to_vec());
    }

    #[test]
    fn toml_unknown_key_errors() {
        let s = "[graph.edges.chars]\nbogus = \"x\"\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn toml_color_out_of_range_errors() {
        let s = "[colors]\ngraph-edge = 999\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn toml_empty_input_yields_default() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.dash, DEFAULT_DASH);
        assert_eq!(cfg.dim_on, DEFAULT_DIM_ON);
    }

    #[test]
    fn toml_activation_marker_key_is_now_unknown() {
        // The configurable marker was removed; auto mode keys off the
        // `bijjou_template_name` field in the input instead.
        assert!(Config::from_toml("activation-marker = \"XX\"\n").is_err());
    }

    #[test]
    fn toml_activate_default_is_always() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.activate, Activate::Always);
    }

    #[test]
    fn toml_activate_accepts_each_variant() {
        for (s, want) in [
            ("auto", Activate::Auto),
            ("always", Activate::Always),
            ("never", Activate::Never),
        ] {
            let cfg = Config::from_toml(&format!("activate = \"{}\"\n", s)).unwrap();
            assert_eq!(cfg.activate, want);
        }
    }

    #[test]
    fn toml_activate_rejects_bad_value() {
        assert!(Config::from_toml("activate = \"sometimes\"\n").is_err());
    }

    #[test]
    fn toml_pager_default_is_auto() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.pager, Pager::Auto);
    }

    #[test]
    fn toml_pager_accepts_each_variant() {
        for (s, want) in [
            ("auto", Pager::Auto),
            ("always", Pager::Always),
            ("never", Pager::Never),
        ] {
            let cfg = Config::from_toml(&format!("pager = \"{}\"\n", s)).unwrap();
            assert_eq!(cfg.pager, want);
        }
    }

    #[test]
    fn toml_pager_rejects_bad_value() {
        assert!(Config::from_toml("pager = \"sometimes\"\n").is_err());
    }

    #[test]
    fn cli_pager_value() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--pager=never"])).unwrap();
        assert_eq!(cfg.pager, Pager::Never);
    }

    #[test]
    fn toml_color_default_is_auto() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.color, Color::Auto);
    }

    #[test]
    fn toml_color_accepts_each_variant() {
        for (s, want) in [
            ("auto", Color::Auto),
            ("always", Color::Always),
            ("never", Color::Never),
        ] {
            let cfg = Config::from_toml(&format!("[ui]\ncolor = \"{}\"\n", s)).unwrap();
            assert_eq!(cfg.color, want);
        }
    }

    #[test]
    fn toml_color_rejects_bad_value() {
        assert!(Config::from_toml("[ui]\ncolor = \"sometimes\"\n").is_err());
    }

    #[test]
    fn cli_bare_color_sets_always() {
        let mut cfg = Config {
            color: Color::Never,
            ..Default::default()
        };
        cfg.apply_cli(args(&["--color"])).unwrap();
        assert_eq!(cfg.color, Color::Always);
    }

    #[test]
    fn cli_color_value() {
        for (s, want) in [
            ("auto", Color::Auto),
            ("always", Color::Always),
            ("never", Color::Never),
        ] {
            let mut cfg = Config::default();
            cfg.apply_cli(args(&[&format!("--color={}", s)])).unwrap();
            assert_eq!(cfg.color, want);
        }
    }

    #[test]
    fn cli_color_invalid_value_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--color=sometimes"])).is_err());
    }

    #[test]
    fn cli_color_nested_form() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--ui__color=never"])).unwrap();
        assert_eq!(cfg.color, Color::Never);
    }

    #[test]
    fn toml_comments_and_blanks_ignored() {
        let s = "# top comment\n\n[layout]\ndash = \"-\"\n# trailing\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.dash, "-");
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cli_bare_activate_sets_always() {
        let mut cfg = Config {
            activate: Activate::Never,
            ..Default::default()
        };
        cfg.apply_cli(args(&["--activate"])).unwrap();
        assert_eq!(cfg.activate, Activate::Always);
    }

    #[test]
    fn cli_activate_value() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--activate=always"])).unwrap();
        assert_eq!(cfg.activate, Activate::Always);
    }

    #[test]
    fn cli_nested_key_dots_via_double_underscore() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--graph__edges__chars__horizontal=X"]))
            .unwrap();
        assert_eq!(cfg.graph_horizontal, "X");
    }

    #[test]
    fn cli_color_int_and_hex() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&[
            "--colors__graph-edge=200",
            "--colors__dash-filler=#aabbcc",
        ]))
        .unwrap();
        assert_eq!(cfg.edge_dim_on, b"\x1b[38;5;200m".to_vec());
        assert_eq!(cfg.dim_on, b"\x1b[38;2;170;187;204m".to_vec());
    }

    #[test]
    fn cli_unknown_key_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--bogus=x"])).is_err());
    }

    #[test]
    fn cli_missing_value_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--layout__dash"])).is_err());
    }

    #[test]
    fn cli_overrides_existing_value() {
        let mut cfg = Config::default();
        cfg.apply_kv("layout.dash", "-", "").unwrap();
        cfg.apply_cli(args(&["--layout__dash=="])).unwrap();
        assert_eq!(cfg.dash, "=");
    }

    #[test]
    fn apply_kv_unknown_key_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_kv("nope.x", "v", "env").is_err());
    }

    #[test]
    fn env_key_uppercase_lowercases() {
        assert_eq!(env_key_to_config_key("ACTIVATE"), "activate");
    }

    #[test]
    fn env_key_double_underscore_becomes_dot() {
        assert_eq!(
            env_key_to_config_key("GRAPH__EDGES__CHARS__TOP_LEFT"),
            "graph.edges.chars.top-left"
        );
    }

    #[test]
    fn env_key_lowercase_hyphen_form_unchanged() {
        assert_eq!(
            env_key_to_config_key("graph__edges__chars__top-left"),
            "graph.edges.chars.top-left"
        );
    }

    #[test]
    fn env_key_single_underscore_becomes_hyphen() {
        assert_eq!(
            env_key_to_config_key("LAYOUT__DASH_START"),
            "layout.dash-start"
        );
    }

    #[test]
    fn stream_defaults() {
        let cfg = Config::from_toml("").unwrap();
        assert!(cfg.stream_enabled);
        assert_eq!(
            cfg.stream_batch_size,
            BatchSize::Fixed(DEFAULT_STREAM_BATCH_SIZE)
        );
    }

    #[test]
    fn stream_toml_section() {
        let s = "[stream]\nenabled = true\nbatch-size = 64\n";
        let cfg = Config::from_toml(s).unwrap();
        assert!(cfg.stream_enabled);
        assert_eq!(cfg.stream_batch_size, BatchSize::Fixed(64));
    }

    #[test]
    fn stream_toml_batch_size_zero_errors() {
        let s = "[stream]\nbatch-size = 0\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn stream_toml_batch_size_negative_errors() {
        let s = "[stream]\nbatch-size = -1\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn cli_bare_stream_sets_enabled() {
        let mut cfg = Config {
            stream_enabled: false,
            ..Default::default()
        };
        cfg.apply_cli(args(&["--stream"])).unwrap();
        assert!(cfg.stream_enabled);
    }

    #[test]
    fn cli_stream_true() {
        let mut cfg = Config {
            stream_enabled: false,
            ..Default::default()
        };
        cfg.apply_cli(args(&["--stream=true"])).unwrap();
        assert!(cfg.stream_enabled);
    }

    #[test]
    fn cli_stream_false() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--stream=false"])).unwrap();
        assert!(!cfg.stream_enabled);
    }

    #[test]
    fn cli_stream_invalid_value_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--stream=sometimes"])).is_err());
    }

    #[test]
    fn cli_stream_batch_size() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--stream", "--stream__batch-size=32"]))
            .unwrap();
        assert!(cfg.stream_enabled);
        assert_eq!(cfg.stream_batch_size, BatchSize::Fixed(32));
    }

    #[test]
    fn cli_stream_batch_size_zero_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--stream__batch-size=0"])).is_err());
    }

    #[test]
    fn cli_stream_batch_size_non_integer_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--stream__batch-size=abc"])).is_err());
    }

    #[test]
    fn stream_toml_batch_size_half_pager() {
        let s = "[stream]\nbatch-size = \"half-pager\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.stream_batch_size, BatchSize::HalfPager);
    }

    #[test]
    fn cli_stream_batch_size_half_pager() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--stream__batch-size=half-pager"]))
            .unwrap();
        assert_eq!(cfg.stream_batch_size, BatchSize::HalfPager);
    }

    #[test]
    fn layout_dash_defaults() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.dash, DEFAULT_DASH);
        assert_eq!(cfg.dash_start, DEFAULT_DASH_START);
        assert_eq!(cfg.dash_end, DEFAULT_DASH_END);
    }

    #[test]
    fn layout_toml_dash_override() {
        let s = "[layout]\ndash = \".\"\ndash-start = \"<\"\ndash-end = \">\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.dash, ".");
        assert_eq!(cfg.dash_start, "<");
        assert_eq!(cfg.dash_end, ">");
    }

    #[test]
    fn cli_layout_dash_start() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--layout__dash-start=<"])).unwrap();
        assert_eq!(cfg.dash_start, "<");
    }

    #[test]
    fn cli_layout_dash_end() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--layout__dash-end=>"])).unwrap();
        assert_eq!(cfg.dash_end, ">");
    }

    #[test]
    fn graph_collapse_defaults_off() {
        assert!(!Config::from_toml("").unwrap().graph_collapse);
    }

    #[test]
    fn graph_collapse_toml_and_cli() {
        let cfg = Config::from_toml("[graph]\ncollapse = true\n").unwrap();
        assert!(cfg.graph_collapse);

        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--graph__collapse=true"])).unwrap();
        assert!(cfg.graph_collapse);
    }

    #[test]
    fn graph_collapse_invalid_value_errors() {
        let mut cfg = Config::default();
        assert!(cfg
            .apply_cli(args(&["--graph__collapse=sometimes"]))
            .is_err());
    }

    #[test]
    fn hydra_defaults_are_on_with_hashed_colors() {
        let cfg = Config::from_toml("").unwrap();
        assert!(cfg.hydra_enable);
        assert!(cfg.hydra_top_stack_padding);
        assert_eq!(cfg.hydra_colors, HydraColors::Hash);
        assert!(cfg.hydra_color_bookmarks);
        assert_eq!(cfg.hydra_prefixes, HydraPrefixes::default());
    }

    #[test]
    fn hydra_color_bookmarks_parses_from_toml_and_cli() {
        let cfg = Config::from_toml("[hydra]\ncolor-bookmarks = false\n").unwrap();
        assert!(!cfg.hydra_color_bookmarks);

        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--hydra__color-bookmarks=false"]))
            .unwrap();
        assert!(!cfg.hydra_color_bookmarks);
    }

    #[test]
    fn hydra_prefixes_come_from_toml_and_cli() {
        let cfg = Config::from_toml(
            "[hydra.prefixes]\nprefix = \"ZZ\"\nconflict-resolution = \"RES\"\n",
        )
        .unwrap();
        assert_eq!(cfg.hydra_prefixes.prefix, "ZZ");
        assert_eq!(cfg.hydra_prefixes.conflict_resolution, "RES");
        assert_eq!(cfg.hydra_prefixes.stack_head, DEFAULT_HYDRA_STACK_HEAD);
        assert_eq!(cfg.hydra_prefixes.anchors(), vec!["ZZB", "ZZH", "ZZRES"]);

        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--hydra__prefixes__stack-working-copy=W"]))
            .unwrap();
        assert_eq!(cfg.hydra_prefixes.working_copy(), "HYW-");
    }

    #[test]
    fn hydra_prefix_replacements_come_from_toml_and_cli() {
        let cfg = Config::from_toml("[hydra.prefixes-replace]\nstack-head = \"Ψ\"\n").unwrap();
        let r = &cfg.hydra_prefixes_replace;
        // The key that was set stands in; every other bookmark is untouched.
        assert_eq!(
            cfg.hydra_prefixes.stack_marker_replace(r).as_deref(),
            Some("Ψ")
        );
        assert_eq!(cfg.hydra_prefixes.working_copy_replace(r), None);
        assert_eq!(
            cfg.hydra_prefixes.anchors_replace(r),
            vec![None, None, None]
        );

        // `prefix` replaces the shared leader only, and a per-bookmark key
        // wins over it.
        let cfg =
            Config::from_toml("[hydra.prefixes-replace]\nprefix = \"⋔\"\nbase = \"◆\"\n").unwrap();
        let r = &cfg.hydra_prefixes_replace;
        assert_eq!(
            cfg.hydra_prefixes.stack_marker_replace(r).as_deref(),
            Some("⋔S-")
        );
        assert_eq!(
            cfg.hydra_prefixes.working_copy_replace(r).as_deref(),
            Some("⋔WC-")
        );
        assert_eq!(
            cfg.hydra_prefixes.anchors_replace(r),
            vec![
                Some("◆".to_string()),
                Some("⋔H".to_string()),
                Some("⋔CR".to_string())
            ]
        );

        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--hydra__prefixes-replace__stack-working-copy=ψ"]))
            .unwrap();
        assert_eq!(
            cfg.hydra_prefixes
                .working_copy_replace(&cfg.hydra_prefixes_replace)
                .as_deref(),
            Some("ψ")
        );
    }

    #[test]
    fn hydra_colors_bool_forms() {
        let cfg = Config::from_toml("[hydra]\ncolors = false\n").unwrap();
        assert_eq!(cfg.hydra_colors, HydraColors::Off);

        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--hydra__colors=true"])).unwrap();
        assert_eq!(cfg.hydra_colors, HydraColors::Hash);
    }

    #[test]
    fn hydra_colors_toml_list_flattens_to_a_palette() {
        let cfg = Config::from_toml("[hydra]\ncolors = [1, \"#aabbcc\"]\n").unwrap();
        assert_eq!(
            cfg.hydra_colors,
            HydraColors::Palette(vec![
                b"\x1b[38;5;1m".to_vec(),
                b"\x1b[38;2;170;187;204m".to_vec(),
            ])
        );
    }

    #[test]
    fn hydra_colors_cli_list_is_comma_separated() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--hydra__colors=1,#aabbcc"])).unwrap();
        assert_eq!(
            cfg.hydra_colors,
            HydraColors::Palette(vec![
                b"\x1b[38;5;1m".to_vec(),
                b"\x1b[38;2;170;187;204m".to_vec(),
            ])
        );
    }

    #[test]
    fn hydra_colors_rejects_bad_entries() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--hydra__colors=1,999"])).is_err());
        assert!(cfg.apply_cli(args(&["--hydra__colors=1,,2"])).is_err());
        assert!(cfg.apply_cli(args(&["--hydra__colors=sometimes"])).is_err());
    }

    #[test]
    fn hydra_toggles_reject_non_bools() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--hydra__enable=sometimes"])).is_err());
        assert!(cfg
            .apply_cli(args(&["--hydra__top-stack-padding=sometimes"]))
            .is_err());
    }

    #[test]
    fn toml_array_of_non_scalars_errors() {
        assert!(Config::from_toml("[hydra]\ncolors = [[1]]\n").is_err());
    }
}
