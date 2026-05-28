use std::path::PathBuf;
use std::sync::OnceLock;

pub const DEFAULT_DASH: &str = "─";
pub const DEFAULT_DASH_START: &str = "╶";
pub const DEFAULT_DASH_END: &str = "╴";
pub const DEFAULT_DIM_ON: &[u8] = b"\x1b[38;5;8m";
pub const DEFAULT_EDGE_DIM_ON: &[u8] = b"\x1b[38;5;8m";
pub const DEFAULT_EMPTY_ICON: &str = "";
pub const DEFAULT_WC_EMPTY_ICON: &str = "□";
pub const DEFAULT_EMPTY_IMMUTABLE_ICON: &str = "";
pub const DEFAULT_WC_ICON: &str = "■";
pub const DEFAULT_MUTABLE_ICON: &str = "●";
pub const DEFAULT_IMMUTABLE_ICON: &str = "";
pub const DEFAULT_CONFLICT_ICON: &str = "";
pub const DEFAULT_HIDDEN_ICON: &str = "🮀";
pub const DEFAULT_FALLBACK_ICON: &str = "●";
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
pub const DEFAULT_ACTIVATION_MARKER: &str = "BIJJOU_ACTIVATE";
pub const DEFAULT_STREAM_BATCH_SIZE: usize = 128;
pub const DEFAULT_TEMPLATE_ONELINE: &str = " %{elastic_tab(change_id)} %{elastic_tab(commit_id)} %{elastic_tab(author)} %{elastic_tab(timestamp)} %{working_copies} %{bookmarks} %{tags} %{description}";

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

pub fn validate_activation_marker(m: &str) -> Result<(), String> {
    if m.is_empty() {
        return Err("activation-marker: must not be empty".into());
    }
    if let Some(c) = m.chars().find(|c| c.is_control()) {
        return Err(format!(
            "activation-marker: contains non-printable character {:?}",
            c
        ));
    }
    Ok(())
}

pub struct Config {
    pub wc_icon: String,
    pub mutable_icon: String,
    pub immutable_icon: String,
    pub conflict_icon: String,
    pub hidden_icon: String,
    pub fallback_icon: String,
    pub empty_icon: String,
    pub wc_empty_icon: String,
    pub empty_immutable_icon: String,
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
    pub dim_on: Vec<u8>,
    pub edge_dim_on: Vec<u8>,
    pub activation_marker: String,
    pub activate: Activate,
    pub pager: Pager,
    pub color: Color,
    pub stream_enabled: bool,
    pub stream_batch_size: BatchSize,
    pub debug_force_screen_height: Option<usize>,
    pub template_oneline: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wc_icon: DEFAULT_WC_ICON.to_string(),
            mutable_icon: DEFAULT_MUTABLE_ICON.to_string(),
            immutable_icon: DEFAULT_IMMUTABLE_ICON.to_string(),
            conflict_icon: DEFAULT_CONFLICT_ICON.to_string(),
            hidden_icon: DEFAULT_HIDDEN_ICON.to_string(),
            fallback_icon: DEFAULT_FALLBACK_ICON.to_string(),
            empty_icon: DEFAULT_EMPTY_ICON.to_string(),
            wc_empty_icon: DEFAULT_WC_EMPTY_ICON.to_string(),
            empty_immutable_icon: DEFAULT_EMPTY_IMMUTABLE_ICON.to_string(),
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
            dim_on: DEFAULT_DIM_ON.to_vec(),
            edge_dim_on: DEFAULT_EDGE_DIM_ON.to_vec(),
            activation_marker: DEFAULT_ACTIVATION_MARKER.to_string(),
            activate: Activate::default(),
            pager: Pager::default(),
            color: Color::default(),
            stream_enabled: true,
            stream_batch_size: BatchSize::default(),
            debug_force_screen_height: None,
            template_oneline: DEFAULT_TEMPLATE_ONELINE.to_string(),
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
            toml::Value::Array(_) => return Err(format!("{}: arrays not supported", key)),
            toml::Value::Datetime(_) => return Err(format!("{}: datetimes not supported", key)),
        }
    }
    Ok(())
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
            "activation-marker" => {
                validate_activation_marker(value).map_err(mkerr)?;
                self.activation_marker = value.to_string();
            }
            "graph.nodes.chars.working-copy" => self.wc_icon = value.to_string(),
            "graph.nodes.chars.mutable" => self.mutable_icon = value.to_string(),
            "graph.nodes.chars.immutable" => self.immutable_icon = value.to_string(),
            "graph.nodes.chars.conflict" => self.conflict_icon = value.to_string(),
            "graph.nodes.chars.hidden" => self.hidden_icon = value.to_string(),
            "graph.nodes.chars.fallback" => self.fallback_icon = value.to_string(),
            "graph.nodes.chars.empty" => self.empty_icon = value.to_string(),
            "graph.nodes.chars.working-copy-empty" => self.wc_empty_icon = value.to_string(),
            "graph.nodes.chars.empty-immutable" => self.empty_immutable_icon = value.to_string(),
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
            "layout.dash" => self.dash = value.to_string(),
            "layout.dash-start" => self.dash_start = value.to_string(),
            "layout.dash-end" => self.dash_end = value.to_string(),
            "stream.enabled" => self.stream_enabled = parse_bool_str(value).map_err(mkerr)?,
            "stream.batch-size" => {
                self.stream_batch_size = parse_batch_size(value).map_err(mkerr)?;
            }
            "template.oneline" => self.template_oneline = value.to_string(),
            "colors.dash-filler" => self.dim_on = parse_color_str(value).map_err(mkerr)?,
            "colors.edge" => self.edge_dim_on = parse_color_str(value).map_err(mkerr)?,
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

const EXAMPLE_CONFIG: &str = include_str!("../config.default.toml");

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
[graph.nodes.chars]
working-copy = "X"

[colors]
edge = 200
"#;
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.wc_icon, "X");
        assert_eq!(cfg.edge_dim_on, b"\x1b[38;5;200m".to_vec());
    }

    #[test]
    fn toml_color_hex_string() {
        let s = "[colors]\nedge = \"#aabbcc\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.edge_dim_on, b"\x1b[38;2;170;187;204m".to_vec());
    }

    #[test]
    fn toml_unknown_key_errors() {
        let s = "[graph.nodes.chars]\nbogus = \"x\"\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn toml_color_out_of_range_errors() {
        let s = "[colors]\nedge = 999\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn toml_empty_input_yields_default() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.wc_icon, DEFAULT_WC_ICON);
        assert_eq!(cfg.dim_on, DEFAULT_DIM_ON);
    }

    #[test]
    fn toml_activation_marker_override() {
        let s = "activation-marker = \"XX\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.activation_marker, "XX");
    }

    #[test]
    fn toml_activation_marker_empty_is_error() {
        let s = "activation-marker = \"\"\n";
        let err = match Config::from_toml(s) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(err.contains("activation-marker"), "got: {}", err);
    }

    #[test]
    fn toml_activation_marker_control_char_is_error() {
        let s = "activation-marker = \"AB\\nCD\"\n";
        let err = match Config::from_toml(s) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(err.contains("non-printable"), "got: {}", err);
    }

    #[test]
    fn toml_default_activation_marker_matches_const() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.activation_marker, DEFAULT_ACTIVATION_MARKER);
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
        let mut cfg = Config::default();
        cfg.color = Color::Never;
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
        let mut cfg = Config::default();
        cfg.activate = Activate::Never;
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
        cfg.apply_cli(args(&["--graph__nodes__chars__working-copy=X"]))
            .unwrap();
        assert_eq!(cfg.wc_icon, "X");
    }

    #[test]
    fn cli_color_int_and_hex() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&[
            "--colors__edge=200",
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
            env_key_to_config_key("GRAPH__NODES__CHARS__WORKING_COPY"),
            "graph.nodes.chars.working-copy"
        );
    }

    #[test]
    fn env_key_lowercase_hyphen_form_unchanged() {
        assert_eq!(
            env_key_to_config_key("graph__nodes__chars__working-copy"),
            "graph.nodes.chars.working-copy"
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
        let mut cfg = Config::default();
        cfg.stream_enabled = false;
        cfg.apply_cli(args(&["--stream"])).unwrap();
        assert!(cfg.stream_enabled);
    }

    #[test]
    fn cli_stream_true() {
        let mut cfg = Config::default();
        cfg.stream_enabled = false;
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
}
