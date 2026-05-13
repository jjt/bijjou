use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const DEFAULT_DASH: &str = "·";
pub const DEFAULT_DASH_ARROW: &str = "";
pub const DEFAULT_DASH_MARGIN: usize = 1;
pub const DEFAULT_DIM_ON: &[u8] = b"\x1b[38;5;8m";
pub const DEFAULT_EDGE_DIM_ON: &[u8] = b"\x1b[38;5;240m";
pub const DEFAULT_MUTABLE_NODE_COLOR: &[u8] = b"\x1b[38;5;245m";
pub const DEFAULT_EMPTY_ICON: &str = "";
pub const DEFAULT_WC_EMPTY_ICON: &str = "";
pub const DEFAULT_EMPTY_IMMUTABLE_ICON: &str = "";
pub const DEFAULT_WC_ICON: &str = "󰋘";
pub const DEFAULT_MUTABLE_ICON: &str = "";
pub const DEFAULT_IMMUTABLE_ICON: &str = "";
pub const DEFAULT_CONFLICT_ICON: &str = "";
pub const DEFAULT_ALTERNATE_ICON: &str = "";
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
pub const DEFAULT_EMPTY_MARKER: &str = "𝙴";
pub const DEFAULT_IMMUTABLE_MARKER: &str = "𝙸";
pub const DEFAULT_STREAM_BATCH_SIZE: usize = 128;
pub const DEFAULT_ALIGN_ENABLED: bool = true;
pub const DEFAULT_ALIGN_GAP: usize = 2;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Activate {
    #[default]
    Auto,
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
    pub alternate_icon: String,
    pub empty_icon: String,
    pub wc_empty_icon: String,
    pub empty_immutable_icon: String,
    pub dash: String,
    pub dash_arrow: String,
    pub dash_margin: usize,
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
    pub mutable_node_color: Vec<u8>,
    pub activation_marker: String,
    pub empty_marker: String,
    pub immutable_marker: String,
    pub activate: Activate,
    pub pager: Pager,
    pub hide_vertical_only_lines: bool,
    pub stream_enabled: bool,
    pub stream_batch_size: usize,
    pub align_enabled: bool,
    pub align_gap: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wc_icon: DEFAULT_WC_ICON.to_string(),
            mutable_icon: DEFAULT_MUTABLE_ICON.to_string(),
            immutable_icon: DEFAULT_IMMUTABLE_ICON.to_string(),
            conflict_icon: DEFAULT_CONFLICT_ICON.to_string(),
            alternate_icon: DEFAULT_ALTERNATE_ICON.to_string(),
            empty_icon: DEFAULT_EMPTY_ICON.to_string(),
            wc_empty_icon: DEFAULT_WC_EMPTY_ICON.to_string(),
            empty_immutable_icon: DEFAULT_EMPTY_IMMUTABLE_ICON.to_string(),
            dash: DEFAULT_DASH.to_string(),
            dash_arrow: DEFAULT_DASH_ARROW.to_string(),
            dash_margin: DEFAULT_DASH_MARGIN,
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
            mutable_node_color: DEFAULT_MUTABLE_NODE_COLOR.to_vec(),
            activation_marker: DEFAULT_ACTIVATION_MARKER.to_string(),
            empty_marker: DEFAULT_EMPTY_MARKER.to_string(),
            immutable_marker: DEFAULT_IMMUTABLE_MARKER.to_string(),
            activate: Activate::default(),
            pager: Pager::default(),
            hide_vertical_only_lines: false,
            stream_enabled: false,
            stream_batch_size: DEFAULT_STREAM_BATCH_SIZE,
            align_enabled: DEFAULT_ALIGN_ENABLED,
            align_gap: DEFAULT_ALIGN_GAP,
        }
    }
}

#[derive(Debug)]
enum TomlValue {
    String(String),
    Int(i64),
    Bool(bool),
}

type TomlSections = HashMap<String, HashMap<String, TomlValue>>;

fn parse_toml(s: &str) -> Result<TomlSections, String> {
    let mut sections: TomlSections = HashMap::new();
    let mut current = String::new();

    for (idx, raw) in s.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let inner = rest
                .strip_suffix(']')
                .ok_or_else(|| format!("line {}: unclosed section header", idx + 1))?;
            current = inner.trim().to_string();
            sections.entry(current.clone()).or_default();
            continue;
        }
        let eq = line
            .find('=')
            .ok_or_else(|| format!("line {}: missing '='", idx + 1))?;
        let key = line[..eq].trim().to_string();
        let val_str = line[eq + 1..].trim();
        let val = parse_toml_value(val_str).map_err(|e| format!("line {}: {}", idx + 1, e))?;
        sections
            .entry(current.clone())
            .or_default()
            .insert(key, val);
    }
    Ok(sections)
}

fn parse_toml_value(s: &str) -> Result<TomlValue, String> {
    if let Some(rest) = s.strip_prefix('"') {
        let inner = rest
            .strip_suffix('"')
            .ok_or_else(|| format!("unterminated string: {:?}", s))?;
        return Ok(TomlValue::String(unescape(inner)?));
    }
    if s == "true" {
        return Ok(TomlValue::Bool(true));
    }
    if s == "false" {
        return Ok(TomlValue::Bool(false));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(TomlValue::Int(n));
    }
    Err(format!(
        "expected quoted string, bool, or integer, got {:?}",
        s
    ))
}

fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => return Err(format!("bad escape: \\{}", other)),
            None => return Err("trailing backslash".into()),
        }
    }
    Ok(out)
}

fn stringify(v: &TomlValue) -> String {
    match v {
        TomlValue::String(s) => s.clone(),
        TomlValue::Int(n) => n.to_string(),
        TomlValue::Bool(b) => b.to_string(),
    }
}

fn parse_bool_str(s: &str) -> Result<bool, String> {
    match s {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("expected true|false, got {:?}", other)),
    }
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
        let sections = parse_toml(s)?;
        for (section, kvs) in &sections {
            for (k, v) in kvs {
                let dotted = if section.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", section, k)
                };
                cfg.apply_kv(&dotted, &stringify(v), "")?;
            }
        }
        Ok(cfg)
    }

    pub fn apply_env(&mut self) -> Result<(), String> {
        let mut vars: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k.starts_with("BIJJOU__"))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in vars {
            let key = k["BIJJOU__".len()..].replace("__", ".");
            self.apply_kv(&key, &v, "env")?;
        }
        Ok(())
    }

    pub fn apply_cli<I: IntoIterator<Item = String>>(&mut self, args: I) -> Result<(), String> {
        for arg in args {
            if arg == "--activate" {
                self.apply_kv("activate", "auto", "cli")?;
                continue;
            }
            if arg == "--stream" {
                self.apply_kv("stream.enabled", "true", "cli")?;
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
            "activation-marker" => {
                validate_activation_marker(value).map_err(mkerr)?;
                self.activation_marker = value.to_string();
            }
            "graph.nodes.chars.working-copy" => self.wc_icon = value.to_string(),
            "graph.nodes.chars.mutable" => self.mutable_icon = value.to_string(),
            "graph.nodes.chars.immutable" => self.immutable_icon = value.to_string(),
            "graph.nodes.chars.conflict" => self.conflict_icon = value.to_string(),
            "graph.nodes.chars.alternate" => self.alternate_icon = value.to_string(),
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
            "layout.dash-arrow" => self.dash_arrow = value.to_string(),
            "layout.dash-margin" => {
                let n: i64 = value
                    .parse()
                    .map_err(|_| mkerr(format!("expected integer >= 0, got {:?}", value)))?;
                if n < 0 {
                    return Err(mkerr(format!("expected integer >= 0, got {}", n)));
                }
                self.dash_margin = n as usize;
            }
            "commits.markers.empty" => self.empty_marker = value.to_string(),
            "commits.markers.immutable" => self.immutable_marker = value.to_string(),
            "filter.hide-vertical-only-lines" => {
                self.hide_vertical_only_lines = parse_bool_str(value).map_err(mkerr)?;
            }
            "stream.enabled" => self.stream_enabled = parse_bool_str(value).map_err(mkerr)?,
            "stream.batch-size" => {
                let n: i64 = value
                    .parse()
                    .map_err(|_| mkerr(format!("expected integer >= 1, got {:?}", value)))?;
                if n < 1 {
                    return Err(mkerr(format!("expected integer >= 1, got {}", n)));
                }
                self.stream_batch_size = n as usize;
            }
            "layout.align" => self.align_enabled = parse_bool_str(value).map_err(mkerr)?,
            "layout.gap" => {
                let n: i64 = value
                    .parse()
                    .map_err(|_| mkerr(format!("expected integer >= 0, got {:?}", value)))?;
                if n < 0 {
                    return Err(mkerr(format!("expected integer >= 0, got {}", n)));
                }
                self.align_gap = n as usize;
            }
            "colors.dash-filler" => self.dim_on = parse_color_str(value).map_err(mkerr)?,
            "colors.edge" => self.edge_dim_on = parse_color_str(value).map_err(mkerr)?,
            "colors.mutable-node" => self.mutable_node_color = parse_color_str(value).map_err(mkerr)?,
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

const EXAMPLE_CONFIG: &str = include_str!("../examples/bijjou-config.example.toml");

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
        let s = "[colors]\nmutable-node = \"#aabbcc\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.mutable_node_color, b"\x1b[38;2;170;187;204m".to_vec());
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
    fn toml_commits_markers_override() {
        let s = "[commits.markers]\nempty = \"E\"\nimmutable = \"I\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.empty_marker, "E");
        assert_eq!(cfg.immutable_marker, "I");
    }

    #[test]
    fn toml_commits_markers_empty_disables() {
        let s = "[commits.markers]\nempty = \"\"\nimmutable = \"\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.empty_marker, "");
        assert_eq!(cfg.immutable_marker, "");
    }

    #[test]
    fn toml_default_commits_markers_match_legacy() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.empty_marker, DEFAULT_EMPTY_MARKER);
        assert_eq!(cfg.immutable_marker, DEFAULT_IMMUTABLE_MARKER);
    }

    #[test]
    fn toml_activate_default_is_auto() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.activate, Activate::Auto);
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
    fn toml_comments_and_blanks_ignored() {
        let s = "# top comment\n\n[layout]\ndash = \"-\"\n# trailing\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.dash, "-");
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cli_bare_activate_sets_auto() {
        let mut cfg = Config::default();
        cfg.activate = Activate::Never;
        cfg.apply_cli(args(&["--activate"])).unwrap();
        assert_eq!(cfg.activate, Activate::Auto);
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
        cfg.apply_cli(args(&["--colors__edge=200", "--colors__mutable-node=#aabbcc"]))
            .unwrap();
        assert_eq!(cfg.edge_dim_on, b"\x1b[38;5;200m".to_vec());
        assert_eq!(cfg.mutable_node_color, b"\x1b[38;2;170;187;204m".to_vec());
    }

    #[test]
    fn cli_bool_filter() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--filter__hide-vertical-only-lines=true"]))
            .unwrap();
        assert!(cfg.hide_vertical_only_lines);
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
    fn stream_defaults() {
        let cfg = Config::from_toml("").unwrap();
        assert!(!cfg.stream_enabled);
        assert_eq!(cfg.stream_batch_size, DEFAULT_STREAM_BATCH_SIZE);
    }

    #[test]
    fn stream_toml_section() {
        let s = "[stream]\nenabled = true\nbatch-size = 64\n";
        let cfg = Config::from_toml(s).unwrap();
        assert!(cfg.stream_enabled);
        assert_eq!(cfg.stream_batch_size, 64);
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
        cfg.apply_cli(args(&["--stream"])).unwrap();
        assert!(cfg.stream_enabled);
    }

    #[test]
    fn cli_stream_batch_size() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--stream", "--stream__batch-size=32"])).unwrap();
        assert!(cfg.stream_enabled);
        assert_eq!(cfg.stream_batch_size, 32);
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
    fn layout_defaults() {
        let cfg = Config::from_toml("").unwrap();
        assert!(cfg.align_enabled);
        assert_eq!(cfg.align_gap, DEFAULT_ALIGN_GAP);
    }

    #[test]
    fn layout_toml_section() {
        let s = "[layout]\nalign = false\ngap = 4\ndash = \".\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert!(!cfg.align_enabled);
        assert_eq!(cfg.align_gap, 4);
        assert_eq!(cfg.dash, ".");
    }

    #[test]
    fn layout_toml_gap_zero_ok() {
        let s = "[layout]\ngap = 0\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.align_gap, 0);
    }

    #[test]
    fn layout_toml_gap_negative_errors() {
        let s = "[layout]\ngap = -1\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn cli_layout_align_false() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--layout__align=false"])).unwrap();
        assert!(!cfg.align_enabled);
    }

    #[test]
    fn cli_layout_gap() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--layout__gap=5"])).unwrap();
        assert_eq!(cfg.align_gap, 5);
    }

    #[test]
    fn cli_layout_gap_zero_ok() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--layout__gap=0"])).unwrap();
        assert_eq!(cfg.align_gap, 0);
    }

    #[test]
    fn cli_layout_gap_negative_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_cli(args(&["--layout__gap=-1"])).is_err());
    }

    #[test]
    fn cli_layout_dash_arrow() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--layout__dash-arrow=>"])).unwrap();
        assert_eq!(cfg.dash_arrow, ">");
    }

    #[test]
    fn layout_dash_margin_default() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.dash_margin, DEFAULT_DASH_MARGIN);
        assert_eq!(cfg.dash, DEFAULT_DASH);
    }

    #[test]
    fn layout_dash_margin_toml() {
        let s = "[layout]\ndash-margin = 0\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.dash_margin, 0);
    }

    #[test]
    fn layout_dash_margin_negative_errors() {
        let s = "[layout]\ndash-margin = -1\n";
        assert!(Config::from_toml(s).is_err());
    }

    #[test]
    fn cli_layout_dash_margin() {
        let mut cfg = Config::default();
        cfg.apply_cli(args(&["--layout__dash-margin=3"])).unwrap();
        assert_eq!(cfg.dash_margin, 3);
    }
}
