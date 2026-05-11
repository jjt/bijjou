use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const DEFAULT_DASH: &str = "━";
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
pub const DEFAULT_GRAPH_ELISION: &str = "⌇";
pub const DEFAULT_ACTIVATION_MARKER: &str = "𝘽";

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
        }
    }
}

#[derive(Debug)]
enum TomlValue {
    String(String),
    Int(i64),
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
    if let Ok(n) = s.parse::<i64>() {
        return Ok(TomlValue::Int(n));
    }
    Err(format!("expected quoted string or integer, got {:?}", s))
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

fn parse_color(v: &TomlValue) -> Result<Vec<u8>, String> {
    match v {
        TomlValue::Int(n) => {
            if !(0..=255).contains(n) {
                return Err(format!("expected 0-255, got {}", n));
            }
            Ok(format!("\x1b[38;5;{}m", n).into_bytes())
        }
        TomlValue::String(s) => {
            let hex = s
                .strip_prefix('#')
                .ok_or_else(|| format!("expected integer or \"#rrggbb\", got {:?}", s))?;
            if hex.len() != 6 {
                return Err(format!("expected #rrggbb, got {:?}", s));
            }
            let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| format!("bad hex: {:?}", s))?;
            let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| format!("bad hex: {:?}", s))?;
            let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| format!("bad hex: {:?}", s))?;
            Ok(format!("\x1b[38;2;{};{};{}m", r, g, b).into_bytes())
        }
    }
}

fn take_string(sec: &str, k: &str, v: &TomlValue) -> Result<String, String> {
    match v {
        TomlValue::String(s) => Ok(s.clone()),
        _ => Err(format!("{}.{}: expected string", sec, k)),
    }
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, String> {
        let mut cfg = Self::default();
        let sections = parse_toml(s)?;

        if let Some(sec) = sections.get("icons") {
            for (k, v) in sec {
                let s = take_string("icons", k, v)?;
                match k.as_str() {
                    "working_copy" => cfg.wc_icon = s,
                    "mutable" => cfg.mutable_icon = s,
                    "immutable" => cfg.immutable_icon = s,
                    "conflict" => cfg.conflict_icon = s,
                    "alternate" => cfg.alternate_icon = s,
                    "empty" => cfg.empty_icon = s,
                    "working_copy_empty" => cfg.wc_empty_icon = s,
                    "empty_immutable" => cfg.empty_immutable_icon = s,
                    other => return Err(format!("unknown key: icons.{}", other)),
                }
            }
        }

        if let Some(sec) = sections.get("graph") {
            for (k, v) in sec {
                let s = take_string("graph", k, v)?;
                match k.as_str() {
                    "horizontal" => cfg.graph_horizontal = s,
                    "vertical" => cfg.graph_vertical = s,
                    "top_left" => cfg.graph_top_left = s,
                    "top_right" => cfg.graph_top_right = s,
                    "bottom_left" => cfg.graph_bottom_left = s,
                    "bottom_right" => cfg.graph_bottom_right = s,
                    "tee_right" => cfg.graph_tee_right = s,
                    "tee_left" => cfg.graph_tee_left = s,
                    "tee_down" => cfg.graph_tee_down = s,
                    "tee_up" => cfg.graph_tee_up = s,
                    "cross" => cfg.graph_cross = s,
                    "elision" => cfg.graph_elision = s,
                    other => return Err(format!("unknown key: graph.{}", other)),
                }
            }
        }

        if let Some(sec) = sections.get("separator") {
            for (k, v) in sec {
                let s = take_string("separator", k, v)?;
                match k.as_str() {
                    "dash" => cfg.dash = s,
                    other => return Err(format!("unknown key: separator.{}", other)),
                }
            }
        }

        if let Some(sec) = sections.get("activation") {
            for (k, v) in sec {
                let s = take_string("activation", k, v)?;
                match k.as_str() {
                    "marker" => cfg.activation_marker = s,
                    other => return Err(format!("unknown key: activation.{}", other)),
                }
            }
        }

        if let Some(sec) = sections.get("colors") {
            for (k, v) in sec {
                let bytes = parse_color(v).map_err(|e| format!("colors.{}: {}", k, e))?;
                match k.as_str() {
                    "dash_filler" => cfg.dim_on = bytes,
                    "edge" => cfg.edge_dim_on = bytes,
                    "mutable_node" => cfg.mutable_node_color = bytes,
                    other => return Err(format!("unknown key: colors.{}", other)),
                }
            }
        }

        Ok(cfg)
    }

    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match Self::from_toml(&content) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("bijjou: {}: {}", path.display(), e);
                Self::default()
            }
        }
    }
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
[icons]
working_copy = "X"

[colors]
edge = 200
"#;
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.wc_icon, "X");
        assert_eq!(cfg.edge_dim_on, b"\x1b[38;5;200m".to_vec());
    }

    #[test]
    fn toml_color_hex_string() {
        let s = "[colors]\nmutable_node = \"#aabbcc\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.mutable_node_color, b"\x1b[38;2;170;187;204m".to_vec());
    }

    #[test]
    fn toml_unknown_key_errors() {
        let s = "[icons]\nbogus = \"x\"\n";
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
        let s = "[activation]\nmarker = \"XX\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.activation_marker, "XX");
    }

    #[test]
    fn toml_activation_marker_empty_disables_gate() {
        let s = "[activation]\nmarker = \"\"\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.activation_marker, "");
    }

    #[test]
    fn toml_default_activation_marker_is_b() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.activation_marker, DEFAULT_ACTIVATION_MARKER);
    }

    #[test]
    fn toml_comments_and_blanks_ignored() {
        let s = "# top comment\n\n[separator]\ndash = \"-\"\n# trailing\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.dash, "-");
    }
}
