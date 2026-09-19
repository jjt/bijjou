// Package config holds every runtime setting of bijjou, and the file, env
// and CLI layers that build one.
package config

import (
	"bytes"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/charmbracelet/x/term"
)

const (
	DefaultDash      = "─"
	DefaultDashStart = "╶"
	// DefaultDashEnd is the closing cell of a dash run, the cell next to the
	// content. This cell is empty by default, so the content always has a
	// plain space to its left. To cap the run against the content, set the
	// cell to a glyph (`╴` U+2574, the half-line that matches dash-start).
	DefaultDashEnd = ""

	DefaultGraphHorizontal  = "𜸟"
	DefaultGraphVertical    = "𜸩"
	DefaultGraphTopLeft     = "𜸚"
	DefaultGraphTopRight    = "𜸤"
	DefaultGraphBottomLeft  = "𜸾"
	DefaultGraphBottomRight = "𜹃"
	DefaultGraphTeeRight    = "𜸨"
	DefaultGraphTeeLeft     = "𜸶"
	DefaultGraphTeeDown     = "𜸠"
	DefaultGraphTeeUp       = "𜹀"
	DefaultGraphCross       = "𜸺"
	DefaultGraphElision     = "𜹀"

	DefaultStreamBatchSize = 128

	DefaultLogOnelineName = "log_oneline"
	DefaultLogOnelineBody = " %{elastic_tab(change_id)} %{elastic_tab(commit_id)} %{elastic_tab(author)} %{elastic_tab(timestamp)} %{working_copies} %{bookmarks} %{tags} %{description}"

	BijjouTemplateNameField = "bijjou_template_name"

	DefaultHydraPrefix             = "HY"
	DefaultHydraBase               = "B"
	DefaultHydraHead               = "H"
	DefaultHydraConflictResolution = "CR"
	DefaultHydraStackHead          = "S"
	DefaultHydraStackWorkingCopy   = "WC"
)

// DefaultDimOn is the color of the dash filler.
var DefaultDimOn = []byte("\x1b[38;5;8m")

// DefaultEdgeDimOn is the color of the graph edges.
var DefaultEdgeDimOn = []byte("\x1b[38;5;8m")

// HydraColorsMode selects one of the three modes of `hydra.colors`. The
// nodes can be left alone, each stack's name can be hashed into a color, or
// an explicit palette can be indexed by the stack's position in the graph.
// The top of the log comes first. The index wraps when there are more stacks
// than colors.
type HydraColorsMode int

const (
	HydraColorsOff HydraColorsMode = iota
	HydraColorsHash
	HydraColorsPalette
)

// HydraColors is the value of `hydra.colors`. Palette is used by
// HydraColorsPalette only.
type HydraColors struct {
	Mode    HydraColorsMode
	Palette [][]byte
}

// Replacement is an optional string: Set reports whether the key was given.
type Replacement struct {
	Value string
	Set   bool
}

// HydraPrefixes is the bookmark naming that this repo's hydra uses. A row is
// classified from its `bookmarks` field alone. `hydra status --toml` reports
// the same naming, but it shells out to jj several times per log. This cost
// is more than every other thing that bijjou does put together.
type HydraPrefixes struct {
	// Prefix is the shared leader of every hydra bookmark: `HY`.
	Prefix string
	// Base, Head and ConflictResolution are the anchor suffixes: `HYB`,
	// `HYH`, `HYCR`.
	Base               string
	Head               string
	ConflictResolution string
	// StackHead and StackWorkingCopy are the per-stack suffixes, each
	// followed by `-<stack name>`: `HYS-foo`, `HYWC-foo`.
	StackHead        string
	StackWorkingCopy string
}

func defaultHydraPrefixes() HydraPrefixes {
	return HydraPrefixes{
		Prefix:             DefaultHydraPrefix,
		Base:               DefaultHydraBase,
		Head:               DefaultHydraHead,
		ConflictResolution: DefaultHydraConflictResolution,
		StackHead:          DefaultHydraStackHead,
		StackWorkingCopy:   DefaultHydraStackWorkingCopy,
	}
}

// StackMarker gives `HYS-`, the marker bookmark that opens a stack.
func (p *HydraPrefixes) StackMarker() string {
	return p.Prefix + p.StackHead + "-"
}

// WorkingCopy gives `HYWC-`, a stack's working copy, which sits outside the
// stack.
func (p *HydraPrefixes) WorkingCopy() string {
	return p.Prefix + p.StackWorkingCopy + "-"
}

// Anchors gives `HYB` / `HYH` / `HYCR`, whole bookmark names, matched
// exactly.
func (p *HydraPrefixes) Anchors() []string {
	return []string{p.Prefix + p.Base, p.Prefix + p.Head, p.Prefix + p.ConflictResolution}
}

// StackMarkerReplace gives what `HYS-` reads as. If the per-bookmark key is
// set, that key is used. If the key is unset, the `HY` leader is replaced
// and the rest of the marker is kept. If neither applies, the result is
// unset.
func (p *HydraPrefixes) StackMarkerReplace(r *HydraPrefixReplace) Replacement {
	return replaceLeader(r.StackHead, r.Prefix, p.StackHead+"-")
}

// WorkingCopyReplace gives what `HYWC-` reads as, under the same rule.
func (p *HydraPrefixes) WorkingCopyReplace(r *HydraPrefixReplace) Replacement {
	return replaceLeader(r.StackWorkingCopy, r.Prefix, p.StackWorkingCopy+"-")
}

// AnchorsReplace gives what `HYB` / `HYH` / `HYCR` read as. The order
// matches the order that Anchors returns them in.
func (p *HydraPrefixes) AnchorsReplace(r *HydraPrefixReplace) []Replacement {
	return []Replacement{
		replaceLeader(r.Base, r.Prefix, p.Base),
		replaceLeader(r.Head, r.Prefix, p.Head),
		replaceLeader(r.ConflictResolution, r.Prefix, p.ConflictResolution),
	}
}

// HydraPrefixReplace is `hydra.prefixes-replace`: what a hydra bookmark
// reads as once rendered. It is keyed the same way as HydraPrefixes. A set
// key stands in for the leader that HydraPrefixes builds out of it, and the
// name behind that leader is kept. For example, `base = "◆"` renders `HYB`
// as `◆`, and `stack-head = "Ψ"` renders `HYS-foo` as `Ψfoo`. The dash
// belongs to the leader, so it goes with the leader. Prefix replaces the
// shared `HY` leader only, so `prefix = "Ψ"` renders `HYS-foo` as `ΨS-foo`.
// A per-bookmark key wins over Prefix. If a key is left unset, the bookmarks
// it names stay exactly as jj printed them.
type HydraPrefixReplace struct {
	Prefix             Replacement
	Base               Replacement
	Head               Replacement
	ConflictResolution Replacement
	StackHead          Replacement
	StackWorkingCopy   Replacement
}

// replaceLeader gives one bookmark's rendered leader. If the whole-leader
// key is set, that replacement is used. If not, the prefix replacement is
// used, and the rest of the leader is kept behind it. If neither is set, the
// result is unset and that bookmark is left alone.
func replaceLeader(whole, prefix Replacement, rest string) Replacement {
	switch {
	case whole.Set:
		return whole
	case prefix.Set:
		return Replacement{Value: prefix.Value + rest, Set: true}
	default:
		return Replacement{}
	}
}

// BatchSize is `stream.batch-size`: a fixed line count, or half of the
// pager's screen.
type BatchSize struct {
	HalfPager bool
	Fixed     int
}

func defaultBatchSize() BatchSize {
	return BatchSize{Fixed: DefaultStreamBatchSize}
}

// Mode is the tri-state shared by `activate`, `pager` and `ui.color`.
type Mode int

const (
	ModeAuto Mode = iota
	ModeAlways
	ModeNever
)

// ParseMode reads one of auto, always or never.
func ParseMode(s string) (Mode, error) {
	switch s {
	case "auto":
		return ModeAuto, nil
	case "always":
		return ModeAlways, nil
	case "never":
		return ModeNever, nil
	default:
		return ModeAuto, fmt.Errorf("expected auto|always|never, got %s", strconv.Quote(s))
	}
}

// Config is the whole runtime configuration.
type Config struct {
	Dash      string
	DashStart string
	DashEnd   string

	GraphHorizontal  string
	GraphVertical    string
	GraphTopLeft     string
	GraphTopRight    string
	GraphBottomLeft  string
	GraphBottomRight string
	GraphTeeRight    string
	GraphTeeLeft     string
	GraphTeeDown     string
	GraphTeeUp       string
	GraphCross       string
	GraphElision     string
	GraphCollapse    bool

	DimOn     []byte
	EdgeDimOn []byte

	Activate Mode
	Pager    Mode
	Color    Mode

	StreamEnabled   bool
	StreamBatchSize BatchSize

	// DebugForceScreenHeight is 0 when unset.
	DebugForceScreenHeight int

	Templates map[string]string

	HydraEnable          bool
	HydraTopStackPadding bool
	HydraColorBookmarks  bool
	HydraColors          HydraColors
	HydraPrefixes        HydraPrefixes
	HydraPrefixesReplace HydraPrefixReplace
}

// Default gives the built-in configuration, the one that applies when no
// layer changes anything.
func Default() *Config {
	return &Config{
		Dash:             DefaultDash,
		DashStart:        DefaultDashStart,
		DashEnd:          DefaultDashEnd,
		GraphHorizontal:  DefaultGraphHorizontal,
		GraphVertical:    DefaultGraphVertical,
		GraphTopLeft:     DefaultGraphTopLeft,
		GraphTopRight:    DefaultGraphTopRight,
		GraphBottomLeft:  DefaultGraphBottomLeft,
		GraphBottomRight: DefaultGraphBottomRight,
		GraphTeeRight:    DefaultGraphTeeRight,
		GraphTeeLeft:     DefaultGraphTeeLeft,
		GraphTeeDown:     DefaultGraphTeeDown,
		GraphTeeUp:       DefaultGraphTeeUp,
		GraphCross:       DefaultGraphCross,
		GraphElision:     DefaultGraphElision,
		GraphCollapse:    false,
		DimOn:            bytes.Clone(DefaultDimOn),
		EdgeDimOn:        bytes.Clone(DefaultEdgeDimOn),
		Activate:         ModeAlways,
		Pager:            ModeAuto,
		Color:            ModeAuto,
		StreamEnabled:    true,
		StreamBatchSize:  defaultBatchSize(),
		Templates: map[string]string{
			DefaultLogOnelineName: DefaultLogOnelineBody,
		},
		HydraEnable:          true,
		HydraTopStackPadding: true,
		HydraColors:          HydraColors{Mode: HydraColorsHash},
		HydraColorBookmarks:  true,
		HydraPrefixes:        defaultHydraPrefixes(),
	}
}

type keyValue struct {
	key   string
	value string
}

// flattenTOML walks a decoded table and gives one flat `a.b.c` key per
// scalar. Keys are walked in sorted order at every level, so the result does
// not depend on the file's layout.
func flattenTOML(prefix string, table map[string]any, out *[]keyValue) error {
	keys := make([]string, 0, len(table))
	for k := range table {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		key := k
		if prefix != "" {
			key = prefix + "." + k
		}
		switch v := table[k].(type) {
		case map[string]any:
			if err := flattenTOML(key, v, out); err != nil {
				return err
			}
		case string:
			*out = append(*out, keyValue{key, v})
		case int64:
			*out = append(*out, keyValue{key, strconv.FormatInt(v, 10)})
		case bool:
			*out = append(*out, keyValue{key, strconv.FormatBool(v)})
		case float64:
			*out = append(*out, keyValue{key, formatFloat(v)})
		// A list of scalars flattens to the comma-joined form. The env and
		// CLI layers must spell it in this form anyway, so one syntax covers
		// every layer.
		case []any:
			joined, err := joinScalars(key, v)
			if err != nil {
				return err
			}
			*out = append(*out, keyValue{key, joined})
		// An array of tables is not a scalar list.
		case []map[string]any:
			return fmt.Errorf("%s: array items must be scalars", key)
		// The decoder gives every datetime form as a time.Time.
		default:
			return fmt.Errorf("%s: datetimes not supported", key)
		}
	}
	return nil
}

func joinScalars(key string, items []any) (string, error) {
	parts := make([]string, 0, len(items))
	for _, item := range items {
		switch v := item.(type) {
		case string:
			parts = append(parts, v)
		case int64:
			parts = append(parts, strconv.FormatInt(v, 10))
		case bool:
			parts = append(parts, strconv.FormatBool(v))
		case float64:
			parts = append(parts, formatFloat(v))
		default:
			return "", fmt.Errorf("%s: array items must be scalars", key)
		}
	}
	return strings.Join(parts, ","), nil
}

// formatFloat prints a float the way the other layers spell one.
func formatFloat(f float64) string {
	switch {
	case math.IsNaN(f):
		return "NaN"
	case math.IsInf(f, 1):
		return "inf"
	case math.IsInf(f, -1):
		return "-inf"
	}
	return strconv.FormatFloat(f, 'f', -1, 64)
}

func parseBoolStr(s string) (bool, error) {
	switch s {
	case "true":
		return true, nil
	case "false":
		return false, nil
	default:
		return false, fmt.Errorf("expected true|false, got %s", strconv.Quote(s))
	}
}

// envKeyToConfigKey maps an env-var name onto a config key. The env-var
// convention is uppercase. `__` separates config-path segments and becomes
// `.`. A single `_` becomes `-`. The lowercase or hyphenated form is
// accepted unchanged.
func envKeyToConfigKey(name string) string {
	s := strings.ReplaceAll(name, "__", ".")
	s = strings.ReplaceAll(s, "_", "-")
	return strings.ToLower(s)
}

func parseBatchSize(s string) (BatchSize, error) {
	if s == "half-pager" {
		return BatchSize{HalfPager: true}, nil
	}
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil {
		return BatchSize{}, fmt.Errorf("expected integer >= 1 or \"half-pager\", got %s", strconv.Quote(s))
	}
	if n < 1 {
		return BatchSize{}, fmt.Errorf("expected integer >= 1, got %d", n)
	}
	return BatchSize{Fixed: int(n)}, nil
}

func parseColorStr(s string) ([]byte, error) {
	if hex, ok := strings.CutPrefix(s, "#"); ok {
		if len(hex) != 6 {
			return nil, fmt.Errorf("expected #rrggbb, got %s", strconv.Quote(s))
		}
		r, errR := strconv.ParseUint(hex[0:2], 16, 8)
		g, errG := strconv.ParseUint(hex[2:4], 16, 8)
		b, errB := strconv.ParseUint(hex[4:6], 16, 8)
		if errR != nil || errG != nil || errB != nil {
			return nil, fmt.Errorf("bad hex: %s", strconv.Quote(s))
		}
		return fmt.Appendf(nil, "\x1b[38;2;%d;%d;%dm", r, g, b), nil
	}
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil {
		return nil, fmt.Errorf("expected integer or \"#rrggbb\", got %s", strconv.Quote(s))
	}
	if n < 0 || n > 255 {
		return nil, fmt.Errorf("expected 0-255, got %d", n)
	}
	return fmt.Appendf(nil, "\x1b[38;5;%dm", n), nil
}

func parseHydraColors(s string) (HydraColors, error) {
	switch s {
	case "true":
		return HydraColors{Mode: HydraColorsHash}, nil
	case "false":
		return HydraColors{Mode: HydraColorsOff}, nil
	}
	var palette [][]byte
	for _, part := range strings.Split(s, ",") {
		part = strings.TrimSpace(part)
		if part == "" {
			return HydraColors{}, fmt.Errorf("empty color in palette %s", strconv.Quote(s))
		}
		color, err := parseColorStr(part)
		if err != nil {
			return HydraColors{}, err
		}
		palette = append(palette, color)
	}
	return HydraColors{Mode: HydraColorsPalette, Palette: palette}, nil
}

// FromTOML reads a whole config file over the defaults.
func FromTOML(s string) (*Config, error) {
	cfg := Default()
	var table map[string]any
	if _, err := toml.Decode(s, &table); err != nil {
		return nil, err
	}
	var kvs []keyValue
	if err := flattenTOML("", table, &kvs); err != nil {
		return nil, err
	}
	for _, kv := range kvs {
		if err := cfg.applyKV(kv.key, kv.value, ""); err != nil {
			return nil, err
		}
	}
	return cfg, nil
}

// ApplyEnv applies every `BIJJOU__*` env var over the current config. The
// vars are applied in sorted order, so one run is like the next.
func (c *Config) ApplyEnv() error {
	const prefix = "BIJJOU__"
	var vars []string
	for _, entry := range os.Environ() {
		if strings.HasPrefix(entry, prefix) {
			vars = append(vars, entry)
		}
	}
	sort.Strings(vars)
	for _, entry := range vars {
		name, value, _ := strings.Cut(entry, "=")
		if !strings.HasPrefix(name, prefix) {
			continue
		}
		key := envKeyToConfigKey(name[len(prefix):])
		if err := c.applyKV(key, value, "env"); err != nil {
			return err
		}
	}
	return nil
}

// ApplyCLI applies bijjou's own arguments over the current config. Every
// config key has a generic `--<key>=<value>` form, with `__` standing in for
// the dots. A few keys have a shorthand as well.
func (c *Config) ApplyCLI(args []string) error {
	for _, arg := range args {
		if arg == "--activate" {
			if err := c.applyKV("activate", "always", "cli"); err != nil {
				return err
			}
			continue
		}
		if arg == "--stream" {
			if err := c.applyKV("stream.enabled", "true", "cli"); err != nil {
				return err
			}
			continue
		}
		if value, ok := strings.CutPrefix(arg, "--stream="); ok {
			if err := c.applyKV("stream.enabled", value, "cli"); err != nil {
				return err
			}
			continue
		}
		if arg == "--color" {
			if err := c.applyKV("ui.color", "always", "cli"); err != nil {
				return err
			}
			continue
		}
		if value, ok := strings.CutPrefix(arg, "--color="); ok {
			if err := c.applyKV("ui.color", value, "cli"); err != nil {
				return err
			}
			continue
		}
		rest, ok := strings.CutPrefix(arg, "--")
		if !ok {
			return fmt.Errorf("cli: unknown argument: %s", arg)
		}
		rawKey, value, ok := strings.Cut(rest, "=")
		if !ok {
			return fmt.Errorf("cli: missing value for --%s", rest)
		}
		key := strings.ReplaceAll(rawKey, "__", ".")
		if err := c.applyKV(key, value, "cli"); err != nil {
			return err
		}
	}
	return nil
}

// applyKV applies one flat key over the config. Every layer goes through
// here, so every layer accepts the same keys and gives the same errors. src
// names the layer that the key came from.
func (c *Config) applyKV(key, value, src string) error {
	prefix := ""
	if src != "" {
		prefix = src + ": "
	}
	mkerr := func(err error) error {
		return fmt.Errorf("%s%s: %s", prefix, key, err)
	}
	switch key {
	case "activate":
		v, err := ParseMode(value)
		if err != nil {
			return mkerr(err)
		}
		c.Activate = v
	case "pager":
		v, err := ParseMode(value)
		if err != nil {
			return mkerr(err)
		}
		c.Pager = v
	case "ui.color":
		v, err := ParseMode(value)
		if err != nil {
			return mkerr(err)
		}
		c.Color = v
	case "graph.edges.chars.horizontal":
		c.GraphHorizontal = value
	case "graph.edges.chars.vertical":
		c.GraphVertical = value
	case "graph.edges.chars.top-left":
		c.GraphTopLeft = value
	case "graph.edges.chars.top-right":
		c.GraphTopRight = value
	case "graph.edges.chars.bottom-left":
		c.GraphBottomLeft = value
	case "graph.edges.chars.bottom-right":
		c.GraphBottomRight = value
	case "graph.edges.chars.tee-right":
		c.GraphTeeRight = value
	case "graph.edges.chars.tee-left":
		c.GraphTeeLeft = value
	case "graph.edges.chars.tee-down":
		c.GraphTeeDown = value
	case "graph.edges.chars.tee-up":
		c.GraphTeeUp = value
	case "graph.edges.chars.cross":
		c.GraphCross = value
	case "graph.edges.chars.elision":
		c.GraphElision = value
	case "graph.collapse":
		v, err := parseBoolStr(value)
		if err != nil {
			return mkerr(err)
		}
		c.GraphCollapse = v
	case "layout.dash":
		c.Dash = value
	case "layout.dash-start":
		c.DashStart = value
	case "layout.dash-end":
		c.DashEnd = value
	case "stream.enabled":
		v, err := parseBoolStr(value)
		if err != nil {
			return mkerr(err)
		}
		c.StreamEnabled = v
	case "stream.batch-size":
		v, err := parseBatchSize(value)
		if err != nil {
			return mkerr(err)
		}
		c.StreamBatchSize = v
	case "colors.dash-filler":
		v, err := parseColorStr(value)
		if err != nil {
			return mkerr(err)
		}
		c.DimOn = v
	case "colors.graph-edge":
		v, err := parseColorStr(value)
		if err != nil {
			return mkerr(err)
		}
		c.EdgeDimOn = v
	case "hydra.enable":
		v, err := parseBoolStr(value)
		if err != nil {
			return mkerr(err)
		}
		c.HydraEnable = v
	case "hydra.top-stack-padding":
		v, err := parseBoolStr(value)
		if err != nil {
			return mkerr(err)
		}
		c.HydraTopStackPadding = v
	case "hydra.colors":
		v, err := parseHydraColors(value)
		if err != nil {
			return mkerr(err)
		}
		c.HydraColors = v
	case "hydra.color-bookmarks":
		v, err := parseBoolStr(value)
		if err != nil {
			return mkerr(err)
		}
		c.HydraColorBookmarks = v
	case "hydra.prefixes.prefix":
		c.HydraPrefixes.Prefix = value
	case "hydra.prefixes.base":
		c.HydraPrefixes.Base = value
	case "hydra.prefixes.head":
		c.HydraPrefixes.Head = value
	case "hydra.prefixes.conflict-resolution":
		c.HydraPrefixes.ConflictResolution = value
	case "hydra.prefixes.stack-head":
		c.HydraPrefixes.StackHead = value
	case "hydra.prefixes.stack-working-copy":
		c.HydraPrefixes.StackWorkingCopy = value
	case "hydra.prefixes-replace.prefix":
		c.HydraPrefixesReplace.Prefix = Replacement{Value: value, Set: true}
	case "hydra.prefixes-replace.base":
		c.HydraPrefixesReplace.Base = Replacement{Value: value, Set: true}
	case "hydra.prefixes-replace.head":
		c.HydraPrefixesReplace.Head = Replacement{Value: value, Set: true}
	case "hydra.prefixes-replace.conflict-resolution":
		c.HydraPrefixesReplace.ConflictResolution = Replacement{Value: value, Set: true}
	case "hydra.prefixes-replace.stack-head":
		c.HydraPrefixesReplace.StackHead = Replacement{Value: value, Set: true}
	case "hydra.prefixes-replace.stack-working-copy":
		c.HydraPrefixesReplace.StackWorkingCopy = Replacement{Value: value, Set: true}
	case "debug.force-screen-height":
		n, err := strconv.ParseInt(value, 10, 64)
		if err != nil {
			return mkerr(fmt.Errorf("expected integer >= 1, got %s", strconv.Quote(value)))
		}
		if n < 1 {
			return mkerr(fmt.Errorf("expected integer >= 1, got %d", n))
		}
		c.DebugForceScreenHeight = int(n)
	default:
		if name, ok := strings.CutPrefix(key, "templates."); ok {
			if name == "" || strings.Contains(name, ".") {
				return mkerr(fmt.Errorf("expected `templates.<name>` with a flat name, got %s", strconv.Quote(key)))
			}
			c.Templates[name] = value
			return nil
		}
		return fmt.Errorf("%sunknown key: %s", prefix, key)
	}
	return nil
}

// Load reads the config file. The file is created from the example when it
// is missing and the path was not named by the user.
func Load() (*Config, error) {
	path, ok := configPath()
	if !ok {
		return Default(), nil
	}
	content, err := os.ReadFile(path)
	if err != nil {
		if _, named := os.LookupEnv("BIJJOU_CONFIG"); named {
			return Default(), nil
		}
		created, ok := createDefaultConfig()
		if !ok {
			return Default(), nil
		}
		content, err = os.ReadFile(created)
		if err != nil {
			content = nil
		}
	}
	cfg, err := FromTOML(string(content))
	if err != nil {
		return nil, fmt.Errorf("%s: %s", path, err)
	}
	return cfg, nil
}

// ExampleTOML is the bundled example config. The root package embeds
// bijjou-config.toml into it before Load runs.
var ExampleTOML string

func defaultConfigTarget() (string, bool) {
	if v := os.Getenv("XDG_CONFIG_HOME"); v != "" {
		return filepath.Join(v, "bijjou", "config.toml"), true
	}
	h := os.Getenv("HOME")
	if h == "" {
		return "", false
	}
	return filepath.Join(h, ".config", "bijjou", "config.toml"), true
}

func createDefaultConfig() (string, bool) {
	target, ok := defaultConfigTarget()
	if !ok {
		return "", false
	}
	parent := filepath.Dir(target)
	if err := os.MkdirAll(parent, 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "bijjou: failed to create %s: %s\n", parent, err)
		return "", false
	}
	if err := os.WriteFile(target, []byte(ExampleTOML), 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "bijjou: failed to write %s: %s\n", target, err)
		return "", false
	}
	fmt.Fprintf(os.Stderr, "bijjou: created default config at %s\n", target)
	return target, true
}

func configPath() (string, bool) {
	if v := os.Getenv("BIJJOU_CONFIG"); v != "" {
		return v, true
	}
	base := os.Getenv("XDG_CONFIG_HOME")
	if base == "" {
		home := os.Getenv("HOME")
		if home == "" {
			return "", false
		}
		base = filepath.Join(home, ".config")
	}
	return filepath.Join(base, "bijjou", "config.toml"), true
}

var installed *Config

// Get gives the installed config, or the defaults when Init never ran.
func Get() *Config {
	if installed == nil {
		installed = Default()
	}
	return installed
}

// Init installs the config that Get gives from here on.
func Init(c *Config) {
	installed = c
}

// ColorEnabled reports whether the output carries color.
func ColorEnabled() bool {
	switch Get().Color {
	case ModeAlways:
		return true
	case ModeNever:
		return false
	default:
		return term.IsTerminal(os.Stdout.Fd())
	}
}
