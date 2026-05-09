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

use std::collections::HashMap;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

const DEFAULT_DASH: &str = "━";
const DEFAULT_DIM_ON: &[u8] = b"\x1b[38;5;8m";
const DEFAULT_EDGE_DIM_ON: &[u8] = b"\x1b[38;5;240m";
const DEFAULT_MUTABLE_NODE_COLOR: &[u8] = b"\x1b[38;5;245m";
const FG_RESET: &[u8] = b"\x1b[39m";
const DEFAULT_EMPTY_ICON: &str = "\u{f28d}";
const DEFAULT_WC_EMPTY_ICON: &str = "\u{e667}";
const DEFAULT_EMPTY_IMMUTABLE_ICON: &str = "\u{f456}";
const DEFAULT_WC_ICON: &str = "\u{f02d8}";
const DEFAULT_MUTABLE_ICON: &str = "\u{f111}";
const DEFAULT_IMMUTABLE_ICON: &str = "\u{f023}";
const DEFAULT_CONFLICT_ICON: &str = "\u{f071}";
const DEFAULT_ALTERNATE_ICON: &str = "\u{f059}";
const DEFAULT_GRAPH_HORIZONTAL: &str = "𜸟";
const DEFAULT_GRAPH_VERTICAL: &str = "𜸩";
const DEFAULT_GRAPH_TOP_LEFT: &str = "𜸚";
const DEFAULT_GRAPH_TOP_RIGHT: &str = "𜸤";
const DEFAULT_GRAPH_BOTTOM_LEFT: &str = "𜸾";
const DEFAULT_GRAPH_BOTTOM_RIGHT: &str = "𜹃";
const DEFAULT_GRAPH_TEE_RIGHT: &str = "𜸨";
const DEFAULT_GRAPH_TEE_LEFT: &str = "𜸶";
const DEFAULT_GRAPH_TEE_DOWN: &str = "𜸠";
const DEFAULT_GRAPH_TEE_UP: &str = "𜹀";
const DEFAULT_GRAPH_CROSS: &str = "𜸺";
const DEFAULT_GRAPH_ELISION: &str = "⌇";

const EMPTY_MARKER: u32 = 0x1D640; // 𝙀
const EMPTY_MARKER_BYTES: &[u8] = b"\xf0\x9d\x99\x80";
const IMMUTABLE_MARKER: u32 = 0x1D644; // 𝙄
const IMMUTABLE_MARKER_BYTES: &[u8] = b"\xf0\x9d\x99\x84";

struct Config {
    wc_icon: String,
    mutable_icon: String,
    immutable_icon: String,
    conflict_icon: String,
    alternate_icon: String,
    empty_icon: String,
    wc_empty_icon: String,
    empty_immutable_icon: String,
    dash: String,
    graph_horizontal: String,
    graph_vertical: String,
    graph_top_left: String,
    graph_top_right: String,
    graph_bottom_left: String,
    graph_bottom_right: String,
    graph_tee_right: String,
    graph_tee_left: String,
    graph_tee_down: String,
    graph_tee_up: String,
    graph_cross: String,
    graph_elision: String,
    dim_on: Vec<u8>,
    edge_dim_on: Vec<u8>,
    mutable_node_color: Vec<u8>,
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
    sections.insert(String::new(), HashMap::new());

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
            sections.entry(current.clone()).or_insert_with(HashMap::new);
            continue;
        }
        let eq = line
            .find('=')
            .ok_or_else(|| format!("line {}: missing '='", idx + 1))?;
        let key = line[..eq].trim().to_string();
        let val_str = line[eq + 1..].trim();
        let val = parse_toml_value(val_str)
            .map_err(|e| format!("line {}: {}", idx + 1, e))?;
        sections
            .entry(current.clone())
            .or_insert_with(HashMap::new)
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
            let hex = s.strip_prefix('#').ok_or_else(|| {
                format!("expected integer or \"#rrggbb\", got {:?}", s)
            })?;
            if hex.len() != 6 {
                return Err(format!("expected #rrggbb, got {:?}", s));
            }
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| format!("bad hex: {:?}", s))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| format!("bad hex: {:?}", s))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| format!("bad hex: {:?}", s))?;
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
    fn from_toml(s: &str) -> Result<Self, String> {
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

        if let Some(sec) = sections.get("colors") {
            for (k, v) in sec {
                let bytes = parse_color(v)
                    .map_err(|e| format!("colors.{}: {}", k, e))?;
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

    fn load() -> Self {
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

fn cfg() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

struct Parsed {
    graph_end: usize,
    content_start: usize,
    graph_col: usize,
}

fn skip_csi(bytes: &[u8], i: usize) -> Option<usize> {
    if i + 1 >= bytes.len() || bytes[i] != 0x1b || bytes[i + 1] != b'[' {
        return None;
    }
    let mut j = i + 2;
    while j < bytes.len() {
        let b = bytes[j];
        j += 1;
        if (0x40..=0x7e).contains(&b) {
            return Some(j);
        }
    }
    Some(j)
}

fn decode_utf8(bytes: &[u8], i: usize) -> (u32, usize) {
    let b = bytes[i];
    if b < 0x80 {
        return (b as u32, 1);
    }
    if b < 0xc0 {
        return (b as u32, 1);
    }
    if b < 0xe0 && i + 1 < bytes.len() {
        let cp = ((b & 0x1f) as u32) << 6 | (bytes[i + 1] & 0x3f) as u32;
        return (cp, 2);
    }
    if b < 0xf0 && i + 2 < bytes.len() {
        let cp = ((b & 0x0f) as u32) << 12
            | ((bytes[i + 1] & 0x3f) as u32) << 6
            | (bytes[i + 2] & 0x3f) as u32;
        return (cp, 3);
    }
    if i + 3 < bytes.len() {
        let cp = ((b & 0x07) as u32) << 18
            | ((bytes[i + 1] & 0x3f) as u32) << 12
            | ((bytes[i + 2] & 0x3f) as u32) << 6
            | (bytes[i + 3] & 0x3f) as u32;
        return (cp, 4);
    }
    (b as u32, 1)
}

fn is_graph_char(cp: u32) -> bool {
    matches!(
        cp,
        0x2500
            ..=0x257F      // box drawing block
        | 0x40               // @
        | 0x7E               // ~
        | 0xD7               // ×
        | 0x25CB             // ○
        | 0x25CF             // ●
        | 0x25C6 // ◆
    )
}

fn find_boundary(line: &[u8]) -> Option<Parsed> {
    let mut i = 0;
    let mut vis_col: usize = 0;
    let mut had_graph = false;

    while i < line.len() {
        if let Some(after) = skip_csi(line, i) {
            i = after;
            continue;
        }

        if line[i] == b' ' {
            let sep_start_byte = i;
            let sep_start_col = vis_col;
            let mut k = i;
            let mut space_count = 0;
            let mut last_space_end = i;
            loop {
                while let Some(after) = skip_csi(line, k) {
                    k = after;
                }
                if k < line.len() && line[k] == b' ' {
                    space_count += 1;
                    k += 1;
                    last_space_end = k;
                } else {
                    break;
                }
            }
            if k >= line.len() {
                return None;
            }
            let (cp, _len) = decode_utf8(line, k);
            if is_graph_char(cp) {
                i = k;
                vis_col += space_count;
            } else {
                if !had_graph {
                    return None;
                }
                return Some(Parsed {
                    graph_end: sep_start_byte,
                    content_start: last_space_end,
                    graph_col: sep_start_col,
                });
            }
        } else {
            let (cp, len) = decode_utf8(line, i);
            if is_graph_char(cp) {
                had_graph = true;
                i += len;
                vis_col += 1;
            } else {
                return None;
            }
        }
    }
    None
}

fn is_node_char(cp: u32) -> bool {
    matches!(cp, 0x40 | 0x25CB | 0x25CF | 0x25C6 | 0xD7)
}

// Replace jj's commit-node glyphs with Nerd Font icons.
fn map_node_char(cp: u32) -> Option<&'static str> {
    let c = cfg();
    match cp {
        0x40 => Some(c.wc_icon.as_str()),
        0x25CB => Some(c.mutable_icon.as_str()),
        0x25C6 => Some(c.immutable_icon.as_str()),
        0xD7 => Some(c.conflict_icon.as_str()),
        0x25CF => Some(c.alternate_icon.as_str()),
        _ => None,
    }
}

// Map jj box-drawing graph chars to single-cell visual equivalents.
fn map_graph_char(cp: u32) -> Option<&'static str> {
    let c = cfg();
    match char::from_u32(cp)? {
        '─' | '┄' | '┈' => Some(c.graph_horizontal.as_str()),
        '│' => Some(c.graph_vertical.as_str()),
        '┌' | '╭' => Some(c.graph_top_left.as_str()),
        '┐' | '╮' => Some(c.graph_top_right.as_str()),
        '└' | '╰' => Some(c.graph_bottom_left.as_str()),
        '┘' | '╯' => Some(c.graph_bottom_right.as_str()),
        '├' => Some(c.graph_tee_right.as_str()),
        '┤' => Some(c.graph_tee_left.as_str()),
        '┬' => Some(c.graph_tee_down.as_str()),
        '┴' => Some(c.graph_tee_up.as_str()),
        '┼' => Some(c.graph_cross.as_str()),
        '~' => Some(c.graph_elision.as_str()),
        _ => None,
    }
}

// Emit ANSI sequences from `bytes`, optionally filtering params.
// `filter` returns true for params we should DROP.
fn emit_filtered_ansi(bytes: &[u8], out: &mut Vec<u8>, filter: impl Fn(&str) -> bool) {
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = skip_csi(bytes, i) {
            if bytes[i] == 0x1b
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'['
                && end > 0
                && bytes[end - 1] == b'm'
            {
                let params = std::str::from_utf8(&bytes[i + 2..end - 1]).unwrap_or("");
                if !filter(params) {
                    out.extend_from_slice(&bytes[i..end]);
                }
            } else {
                out.extend_from_slice(&bytes[i..end]);
            }
            i = end;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
}

// Emit bytes with all visible non-space chars wrapped in dim SGR, except
// commit-node chars (○ ● ◆ @ ×) which pass through with normal intensity.
// Strips jj's fg-color codes; preserves other ANSI sequences.
fn emit_dim_graph(bytes: &[u8], out: &mut Vec<u8>, is_empty: bool, is_immutable: bool) {
    let c = cfg();
    let mut i = 0;
    while i < bytes.len() {
        let ansi_start = i;
        while let Some(after) = skip_csi(bytes, i) {
            i = after;
        }
        let ansi_bytes = &bytes[ansi_start..i];

        if i >= bytes.len() {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            break;
        }

        if bytes[i] == b' ' || bytes[i] == b'\n' {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            out.push(bytes[i]);
            i += 1;
            continue;
        }

        let (cp, len) = decode_utf8(bytes, i);
        if cp == EMPTY_MARKER || cp == IMMUTABLE_MARKER {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            i += len;
            continue;
        }
        if is_node_char(cp) {
            // Mutable (○) and immutable (◆, or @ rendered as immutable) share
            // the darker color override; other nodes preserve jj's original ANSI.
            let darken = cp == 0x25CB || cp == 0x25C6 || (cp == 0x40 && is_immutable);
            if darken {
                emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
                out.extend_from_slice(&c.mutable_node_color);
            } else {
                out.extend_from_slice(ansi_bytes);
            }
            if is_empty {
                let icon = match cp {
                    0x40 if is_immutable => c.empty_immutable_icon.as_str(),
                    0x40 => c.wc_empty_icon.as_str(),
                    0x25C6 => c.empty_immutable_icon.as_str(),
                    _ => c.empty_icon.as_str(),
                };
                out.extend_from_slice(icon.as_bytes());
            } else if cp == 0x40 && is_immutable {
                out.extend_from_slice(c.immutable_icon.as_bytes());
            } else {
                match map_node_char(cp) {
                    Some(replacement) => out.extend_from_slice(replacement.as_bytes()),
                    None => out.extend_from_slice(&bytes[i..i + len]),
                }
            }
            if darken {
                out.extend_from_slice(FG_RESET);
            }
        } else {
            emit_filtered_ansi(ansi_bytes, out, is_fg_color_sgr);
            out.extend_from_slice(&c.edge_dim_on);
            match map_graph_char(cp) {
                Some(replacement) => out.extend_from_slice(replacement.as_bytes()),
                None => out.extend_from_slice(&bytes[i..i + len]),
            }
            out.extend_from_slice(FG_RESET);
        }
        i += len;
    }
}

fn line_flags(body: &[u8]) -> (bool, bool) {
    let mut empty = false;
    let mut immutable = false;
    let mut i = 0;
    while i < body.len() {
        if let Some(after) = skip_csi(body, i) {
            i = after;
            continue;
        }
        let (cp, len) = decode_utf8(body, i);
        if cp == EMPTY_MARKER {
            empty = true;
        } else if cp == IMMUTABLE_MARKER {
            immutable = true;
        }
        i += len;
    }
    (empty, immutable)
}

fn write_stripping_marker(content: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < content.len() {
        if content[i..].starts_with(EMPTY_MARKER_BYTES) {
            i += EMPTY_MARKER_BYTES.len();
            continue;
        }
        if content[i..].starts_with(IMMUTABLE_MARKER_BYTES) {
            i += IMMUTABLE_MARKER_BYTES.len();
            continue;
        }
        out.push(content[i]);
        i += 1;
    }
}

fn is_fg_color_sgr(params: &str) -> bool {
    let parts: Vec<&str> = params.split(';').collect();
    match parts
        .first()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(999)
    {
        30..=37 | 39 | 90..=97 => true,
        38 => parts
            .get(1)
            .map(|p| *p == "5" || *p == "2")
            .unwrap_or(false),
        _ => false,
    }
}

fn main() {
    let _ = CONFIG.set(Config::load());
    if let Err(e) = run() {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return;
        }
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    let mut lines: Vec<&[u8]> = Vec::new();
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

    let parsed: Vec<Option<Parsed>> = lines
        .iter()
        .map(|line| {
            let trimmed = if line.last() == Some(&b'\n') {
                &line[..line.len() - 1]
            } else {
                *line
            };
            find_boundary(trimmed)
        })
        .collect();

    let max_graph = parsed
        .iter()
        .filter_map(|p| p.as_ref().map(|p| p.graph_col))
        .max()
        .unwrap_or(0);
    let target_col = max_graph + 2;

    let c = cfg();
    let mut out: Vec<u8> = Vec::with_capacity(input.len() + lines.len() * 8);

    for (line, p) in lines.iter().zip(parsed.iter()) {
        let trailing_nl = line.last() == Some(&b'\n');
        let body = if trailing_nl {
            &line[..line.len() - 1]
        } else {
            *line
        };

        let (is_empty, is_immutable) = line_flags(body);
        match p {
            Some(p) => {
                let graph = &body[..p.graph_end];
                let content = &body[p.content_start..];

                emit_dim_graph(graph, &mut out, is_empty, is_immutable);

                let gap = target_col - p.graph_col;
                let mut peek = p.content_start;
                while let Some(after) = skip_csi(body, peek) {
                    peek = after;
                }
                let first_byte = body.get(peek).copied();
                let has_change_id = first_byte
                    .map(|b| b.is_ascii_alphanumeric())
                    .unwrap_or(false);
                if gap >= 3 && has_change_id {
                    out.write_all(b" ")?;
                    out.write_all(&c.dim_on)?;
                    for _ in 0..(gap - 2) {
                        out.write_all(c.dash.as_bytes())?;
                    }
                    out.write_all(FG_RESET)?;
                    out.write_all(b" ")?;
                } else {
                    for _ in 0..gap {
                        out.write_all(b" ")?;
                    }
                }

                write_stripping_marker(content, &mut out);
            }
            None => {
                emit_dim_graph(body, &mut out, is_empty, is_immutable);
            }
        }

        if trailing_nl {
            out.write_all(b"\n")?;
        }
    }

    write_output(&out, lines.len())
}

fn terminal_height() -> Option<u16> {
    use core::ffi::c_ulong;
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    extern "C" {
        fn ioctl(fd: i32, request: c_ulong, ...) -> i32;
    }

    #[cfg(target_os = "macos")]
    const TIOCGWINSZ: c_ulong = 0x40087468;
    #[cfg(target_os = "linux")]
    const TIOCGWINSZ: c_ulong = 0x5413;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return None;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut ws: Winsize = unsafe { std::mem::zeroed() };
        let fd = io::stdout().as_raw_fd();
        let r = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut Winsize) };
        if r == 0 && ws.ws_row > 0 {
            Some(ws.ws_row)
        } else {
            None
        }
    }
}

fn write_output(buf: &[u8], line_count: usize) -> io::Result<()> {
    let stdout = io::stdout();
    let is_tty = stdout.is_terminal();
    let height = terminal_height();
    let should_page = is_tty && height.map_or(false, |h| line_count > h as usize);

    if should_page {
        if let Some(()) = try_pager(buf)? {
            return Ok(());
        }
    }

    let mut out = stdout.lock();
    out.write_all(buf)?;
    out.flush()?;
    Ok(())
}

fn try_pager(buf: &[u8]) -> io::Result<Option<()>> {
    use std::process::{Command, Stdio};

    let pager_env = std::env::var("PAGER").ok().filter(|s| !s.trim().is_empty());
    let Some(s) = pager_env else {
        return Ok(None);
    };
    let mut parts = s.split_whitespace().map(|s| s.to_string());
    let Some(cmd) = parts.next() else {
        return Ok(None);
    };
    let args: Vec<String> = parts.collect();

    let mut child = match Command::new(&cmd).args(&args).stdin(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
            Err(e) => return Err(e),
        }
    }
    let _ = child.wait();
    Ok(Some(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- skip_csi -----------------------------------------------------------

    #[test]
    fn skip_csi_basic_sgr() {
        assert_eq!(skip_csi(b"\x1b[31mfoo", 0), Some(5));
    }

    #[test]
    fn skip_csi_no_escape_returns_none() {
        assert_eq!(skip_csi(b"foo", 0), None);
    }

    #[test]
    fn skip_csi_with_multi_param_sgr() {
        assert_eq!(skip_csi(b"\x1b[38;5;245mX", 0), Some(11));
    }

    #[test]
    fn skip_csi_unterminated_returns_buffer_end() {
        assert_eq!(skip_csi(b"\x1b[38;5", 0), Some(6));
    }

    #[test]
    fn skip_csi_at_offset() {
        // Skip past the 'X', start at the CSI.
        assert_eq!(skip_csi(b"X\x1b[1m", 1), Some(5));
    }

    // --- decode_utf8 --------------------------------------------------------

    #[test]
    fn decode_utf8_ascii() {
        assert_eq!(decode_utf8(b"A", 0), (0x41, 1));
    }

    #[test]
    fn decode_utf8_two_byte() {
        // U+00A3 £
        assert_eq!(decode_utf8(b"\xc2\xa3", 0), (0xa3, 2));
    }

    #[test]
    fn decode_utf8_three_byte_circle() {
        // U+25CB ○
        assert_eq!(decode_utf8(b"\xe2\x97\x8b", 0), (0x25CB, 3));
    }

    #[test]
    fn decode_utf8_three_byte_diamond() {
        // U+25C6 ◆
        assert_eq!(decode_utf8(b"\xe2\x97\x86", 0), (0x25C6, 3));
    }

    #[test]
    fn decode_utf8_four_byte_empty_marker() {
        assert_eq!(decode_utf8(EMPTY_MARKER_BYTES, 0), (EMPTY_MARKER, 4));
    }

    #[test]
    fn decode_utf8_four_byte_immutable_marker() {
        assert_eq!(decode_utf8(IMMUTABLE_MARKER_BYTES, 0), (IMMUTABLE_MARKER, 4));
    }

    // --- char classifiers ---------------------------------------------------

    #[test]
    fn is_node_char_recognizes_all_nodes() {
        for &cp in &[0x40u32, 0x25CB, 0x25CF, 0x25C6, 0xD7] {
            assert!(is_node_char(cp), "cp={:#x} should be node char", cp);
        }
    }

    #[test]
    fn is_node_char_rejects_non_nodes() {
        for &cp in &[0x2502u32, 0x2500, 0x41, 0x20] {
            assert!(!is_node_char(cp), "cp={:#x} should not be node char", cp);
        }
    }

    #[test]
    fn is_graph_char_includes_box_drawing() {
        assert!(is_graph_char(0x2500)); // ─
        assert!(is_graph_char(0x2502)); // │
        assert!(is_graph_char(0x256D)); // ╭
        assert!(is_graph_char(0x257F)); // upper bound
    }

    #[test]
    fn is_graph_char_includes_node_chars() {
        assert!(is_graph_char(0x40));
        assert!(is_graph_char(0x25CB));
        assert!(is_graph_char(0x25C6));
        assert!(is_graph_char(0xD7));
        assert!(is_graph_char(0x7E)); // ~ elided marker
    }

    #[test]
    fn is_graph_char_rejects_letters() {
        assert!(!is_graph_char(0x41));
        assert!(!is_graph_char(0x61));
    }

    // --- node/graph icon mapping --------------------------------------------

    #[test]
    fn map_node_char_covers_each_node() {
        assert_eq!(map_node_char(0x40), Some(DEFAULT_WC_ICON));
        assert_eq!(map_node_char(0x25CB), Some(DEFAULT_MUTABLE_ICON));
        assert_eq!(map_node_char(0x25C6), Some(DEFAULT_IMMUTABLE_ICON));
        assert_eq!(map_node_char(0xD7), Some(DEFAULT_CONFLICT_ICON));
        assert_eq!(map_node_char(0x25CF), Some(DEFAULT_ALTERNATE_ICON));
    }

    #[test]
    fn map_node_char_returns_none_for_other() {
        assert_eq!(map_node_char(0x41), None);
        assert_eq!(map_node_char(0x2502), None);
    }

    #[test]
    fn map_graph_char_box_drawings() {
        assert!(map_graph_char(0x2500).is_some()); // ─
        assert!(map_graph_char(0x2502).is_some()); // │
        assert!(map_graph_char(0x256D).is_some()); // ╭
        assert!(map_graph_char(0x2570).is_some()); // ╰
        assert!(map_graph_char(0x251C).is_some()); // ├
        assert!(map_graph_char(0x253C).is_some()); // ┼
        assert!(map_graph_char(0x7E).is_some());   // ~
    }

    #[test]
    fn map_graph_char_unknown_returns_none() {
        assert!(map_graph_char(0x41).is_none());
    }

    // --- is_fg_color_sgr ----------------------------------------------------

    #[test]
    fn fg_color_basic_30s() {
        for code in 30u16..=37 {
            assert!(is_fg_color_sgr(&code.to_string()));
        }
        assert!(is_fg_color_sgr("39")); // default fg
    }

    #[test]
    fn fg_color_bright_90s() {
        for code in 90u16..=97 {
            assert!(is_fg_color_sgr(&code.to_string()));
        }
    }

    #[test]
    fn fg_color_extended_256_and_truecolor() {
        assert!(is_fg_color_sgr("38;5;245"));
        assert!(is_fg_color_sgr("38;2;255;199;83"));
    }

    #[test]
    fn fg_color_rejects_bg_and_attrs() {
        assert!(!is_fg_color_sgr("0"));      // reset
        assert!(!is_fg_color_sgr("1"));      // bold
        assert!(!is_fg_color_sgr("3"));      // italic
        assert!(!is_fg_color_sgr("40"));     // bg color
        assert!(!is_fg_color_sgr("49"));     // default bg
        assert!(!is_fg_color_sgr("48;5;1")); // 256-color bg
    }

    #[test]
    fn fg_color_handles_empty_and_garbage() {
        assert!(!is_fg_color_sgr(""));
        assert!(!is_fg_color_sgr("xyz"));
    }

    // --- line_flags ---------------------------------------------------------

    #[test]
    fn line_flags_plain_line() {
        assert_eq!(line_flags(b"hello world"), (false, false));
    }

    #[test]
    fn line_flags_detects_empty_marker() {
        let mut buf = b"prefix ".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b" suffix");
        assert_eq!(line_flags(&buf), (true, false));
    }

    #[test]
    fn line_flags_detects_immutable_marker() {
        let mut buf = b"x".to_vec();
        buf.extend_from_slice(IMMUTABLE_MARKER_BYTES);
        assert_eq!(line_flags(&buf), (false, true));
    }

    #[test]
    fn line_flags_detects_both() {
        let mut buf = Vec::new();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b" ");
        buf.extend_from_slice(IMMUTABLE_MARKER_BYTES);
        assert_eq!(line_flags(&buf), (true, true));
    }

    #[test]
    fn line_flags_skips_csi_sequences() {
        // CSI bytes must not be misread as content.
        let buf = b"\x1b[38;5;10m\x1b[39m";
        assert_eq!(line_flags(buf), (false, false));
    }

    #[test]
    fn line_flags_finds_marker_inside_colored_segment() {
        let mut buf = b"\x1b[38;5;10m".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"\x1b[39m");
        assert_eq!(line_flags(&buf), (true, false));
    }

    // --- write_stripping_marker --------------------------------------------

    #[test]
    fn strip_no_markers_passthrough() {
        let mut out = Vec::new();
        write_stripping_marker(b"hello world", &mut out);
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn strip_empty_marker_only() {
        let mut buf = b"a".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"b");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"ab");
    }

    #[test]
    fn strip_immutable_marker_only() {
        let mut buf = IMMUTABLE_MARKER_BYTES.to_vec();
        buf.extend_from_slice(b"x");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"x");
    }

    #[test]
    fn strip_both_markers_in_one_pass() {
        let mut buf = b"start".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"mid");
        buf.extend_from_slice(IMMUTABLE_MARKER_BYTES);
        buf.extend_from_slice(b"end");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"startmidend");
    }

    #[test]
    fn strip_preserves_ansi_around_markers() {
        let mut buf = b"\x1b[38;5;10m".to_vec();
        buf.extend_from_slice(EMPTY_MARKER_BYTES);
        buf.extend_from_slice(b"\x1b[39m");
        let mut out = Vec::new();
        write_stripping_marker(&buf, &mut out);
        assert_eq!(out, b"\x1b[38;5;10m\x1b[39m");
    }

    // --- emit_filtered_ansi -------------------------------------------------

    #[test]
    fn filtered_ansi_drops_fg_keeps_attrs_and_text() {
        let mut out = Vec::new();
        emit_filtered_ansi(
            b"\x1b[1m\x1b[31mhello\x1b[39m\x1b[0m",
            &mut out,
            is_fg_color_sgr,
        );
        assert_eq!(out, b"\x1b[1mhello\x1b[0m");
    }

    #[test]
    fn filtered_ansi_passthrough_plain_text() {
        let mut out = Vec::new();
        emit_filtered_ansi(b"plain", &mut out, is_fg_color_sgr);
        assert_eq!(out, b"plain");
    }

    #[test]
    fn filtered_ansi_drops_truecolor_fg() {
        let mut out = Vec::new();
        emit_filtered_ansi(
            b"\x1b[38;2;255;199;83mtext\x1b[39m",
            &mut out,
            is_fg_color_sgr,
        );
        assert_eq!(out, b"text");
    }

    // --- find_boundary ------------------------------------------------------

    #[test]
    fn boundary_single_node_then_content() {
        let line = b"\xe2\x97\x8b  abc"; // ○  abc
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 1);
        assert_eq!(p.graph_end, 3);
        assert_eq!(p.content_start, 5);
    }

    #[test]
    fn boundary_returns_none_when_no_graph() {
        assert!(find_boundary(b"plain text").is_none());
    }

    #[test]
    fn boundary_skips_csi_around_graph() {
        let line = b"\x1b[31m\xe2\x97\x8b\x1b[39m  abc";
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 1);
    }

    #[test]
    fn boundary_multi_graph_columns() {
        // │ │ ○  abc — two leading │ separated by spaces, then ○, then content.
        let line = b"\xe2\x94\x82 \xe2\x94\x82 \xe2\x97\x8b  abc";
        let p = find_boundary(line).expect("expected boundary");
        assert_eq!(p.graph_col, 5);
    }

    #[test]
    fn boundary_requires_at_least_one_graph_char() {
        // Spaces with no graph char before content: returns None.
        assert!(find_boundary(b"   abc").is_none());
    }

    // --- emit_dim_graph -----------------------------------------------------

    fn run_emit(graph: &[u8], is_empty: bool, is_immutable: bool) -> Vec<u8> {
        let mut out = Vec::new();
        emit_dim_graph(graph, &mut out, is_empty, is_immutable);
        out
    }

    fn darken(body: &[u8]) -> Vec<u8> {
        let mut v = DEFAULT_MUTABLE_NODE_COLOR.to_vec();
        v.extend_from_slice(body);
        v.extend_from_slice(FG_RESET);
        v
    }

    #[test]
    fn dim_mutable_circle_gets_darken_and_icon() {
        let out = run_emit(b"\xe2\x97\x8b", false, false);
        assert_eq!(out, darken(DEFAULT_MUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_gets_darken_and_lock() {
        let out = run_emit(b"\xe2\x97\x86", false, true);
        assert_eq!(out, darken(DEFAULT_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_darkens_even_without_flag() {
        // ◆ should darken regardless of is_immutable line flag.
        let out = run_emit(b"\xe2\x97\x86", false, false);
        assert_eq!(out, darken(DEFAULT_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_mutable_wc_preserves_jj_color() {
        // Mutable @ keeps jj's bold green; just swaps glyph for WC_ICON.
        let input = b"\x1b[1m\x1b[38;5;2m@\x1b[0m";
        let out = run_emit(input, false, false);
        let mut expected = b"\x1b[1m\x1b[38;5;2m".to_vec();
        expected.extend_from_slice(DEFAULT_WC_ICON.as_bytes());
        expected.extend_from_slice(b"\x1b[0m");
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_empty_wc_uses_empty_icon() {
        let out = run_emit(b"@", true, false);
        assert_eq!(out, DEFAULT_WC_EMPTY_ICON.as_bytes());
    }

    #[test]
    fn dim_immutable_wc_darkens_and_uses_lock() {
        // @ on an immutable line renders as IMMUTABLE_ICON (lock takes precedence).
        let out = run_emit(b"@", false, true);
        assert_eq!(out, darken(DEFAULT_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_immutable_diamond_empty_uses_empty_immutable_icon() {
        let out = run_emit(b"\xe2\x97\x86", true, true);
        assert_eq!(out, darken(DEFAULT_EMPTY_IMMUTABLE_ICON.as_bytes()));
    }

    #[test]
    fn dim_strips_empty_marker() {
        assert_eq!(run_emit(EMPTY_MARKER_BYTES, false, false), b"");
    }

    #[test]
    fn dim_strips_immutable_marker() {
        assert_eq!(run_emit(IMMUTABLE_MARKER_BYTES, false, false), b"");
    }

    #[test]
    fn dim_box_drawing_gets_edge_dim() {
        let out = run_emit(b"\xe2\x94\x82", false, false); // │
        let mut expected = DEFAULT_EDGE_DIM_ON.to_vec();
        expected.extend_from_slice(DEFAULT_GRAPH_VERTICAL.as_bytes());
        expected.extend_from_slice(FG_RESET);
        assert_eq!(out, expected);
    }

    #[test]
    fn dim_spaces_passthrough() {
        assert_eq!(run_emit(b"   ", false, false), b"   ");
    }

    #[test]
    fn dim_strips_fg_color_around_mutable_node() {
        // jj's fg color must be filtered out before the darken color is applied.
        let out = run_emit(b"\x1b[38;5;14m\xe2\x97\x8b\x1b[39m", false, false);
        // No leading [38;5;14m; trailing [39m preserved (not stripped, it's default-fg reset
        // but is_fg_color_sgr flags it as fg → also dropped).
        assert_eq!(out, darken(DEFAULT_MUTABLE_ICON.as_bytes()));
    }

    // --- TOML parser / Config ------------------------------------------------

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
        assert_eq!(
            cfg.mutable_node_color,
            b"\x1b[38;2;170;187;204m".to_vec()
        );
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
    fn toml_comments_and_blanks_ignored() {
        let s = "# top comment\n\n[separator]\ndash = \"-\"\n# trailing\n";
        let cfg = Config::from_toml(s).unwrap();
        assert_eq!(cfg.dash, "-");
    }
}
