package config

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func mustFromTOML(t *testing.T, s string) *Config {
	t.Helper()
	cfg, err := FromTOML(s)
	if err != nil {
		t.Fatalf("FromTOML(%q): %v", s, err)
	}
	return cfg
}

func TestTOMLParsesSectionStringInt(t *testing.T) {
	s := `
[graph.edges.chars]
horizontal = "X"

[colors]
graph-edge = 200
`
	cfg := mustFromTOML(t, s)
	if cfg.GraphHorizontal != "X" {
		t.Errorf("GraphHorizontal = %q, want %q", cfg.GraphHorizontal, "X")
	}
	if want := []byte("\x1b[38;5;200m"); !bytes.Equal(cfg.EdgeDimOn, want) {
		t.Errorf("EdgeDimOn = %q, want %q", cfg.EdgeDimOn, want)
	}
}

func TestTOMLColorHexString(t *testing.T) {
	cfg := mustFromTOML(t, "[colors]\ngraph-edge = \"#aabbcc\"\n")
	if want := []byte("\x1b[38;2;170;187;204m"); !bytes.Equal(cfg.EdgeDimOn, want) {
		t.Errorf("EdgeDimOn = %q, want %q", cfg.EdgeDimOn, want)
	}
}

func TestTOMLUnknownKeyErrors(t *testing.T) {
	if _, err := FromTOML("[graph.edges.chars]\nbogus = \"x\"\n"); err == nil {
		t.Fatal("want error for unknown key")
	}
}

func TestTOMLColorOutOfRangeErrors(t *testing.T) {
	if _, err := FromTOML("[colors]\ngraph-edge = 999\n"); err == nil {
		t.Fatal("want error for out-of-range color")
	}
}

func TestTOMLEmptyInputYieldsDefault(t *testing.T) {
	cfg := mustFromTOML(t, "")
	if cfg.Dash != DefaultDash {
		t.Errorf("Dash = %q, want %q", cfg.Dash, DefaultDash)
	}
	if !bytes.Equal(cfg.DimOn, DefaultDimOn) {
		t.Errorf("DimOn = %q, want %q", cfg.DimOn, DefaultDimOn)
	}
}

func TestTOMLActivationMarkerKeyIsNowUnknown(t *testing.T) {
	// The configurable marker was removed. Auto mode now keys off the
	// `bijjou_template_name` field in the input instead.
	if _, err := FromTOML("activation-marker = \"XX\"\n"); err == nil {
		t.Fatal("want error for activation-marker")
	}
}

func TestTOMLActivateDefaultIsAlways(t *testing.T) {
	if got := mustFromTOML(t, "").Activate; got != ModeAlways {
		t.Errorf("Activate = %v, want ModeAlways", got)
	}
}

func TestTOMLActivateAcceptsEachVariant(t *testing.T) {
	for _, tc := range []struct {
		s    string
		want Mode
	}{{"auto", ModeAuto}, {"always", ModeAlways}, {"never", ModeNever}} {
		cfg := mustFromTOML(t, "activate = \""+tc.s+"\"\n")
		if cfg.Activate != tc.want {
			t.Errorf("activate = %q: got %v, want %v", tc.s, cfg.Activate, tc.want)
		}
	}
}

func TestTOMLActivateRejectsBadValue(t *testing.T) {
	if _, err := FromTOML("activate = \"sometimes\"\n"); err == nil {
		t.Fatal("want error for bad activate value")
	}
}

func TestTOMLPagerDefaultIsAuto(t *testing.T) {
	if got := mustFromTOML(t, "").Pager; got != ModeAuto {
		t.Errorf("Pager = %v, want ModeAuto", got)
	}
}

func TestTOMLPagerAcceptsEachVariant(t *testing.T) {
	for _, tc := range []struct {
		s    string
		want Mode
	}{{"auto", ModeAuto}, {"always", ModeAlways}, {"never", ModeNever}} {
		cfg := mustFromTOML(t, "pager = \""+tc.s+"\"\n")
		if cfg.Pager != tc.want {
			t.Errorf("pager = %q: got %v, want %v", tc.s, cfg.Pager, tc.want)
		}
	}
}

func TestTOMLPagerRejectsBadValue(t *testing.T) {
	if _, err := FromTOML("pager = \"sometimes\"\n"); err == nil {
		t.Fatal("want error for bad pager value")
	}
}

func TestCLIPagerValue(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--pager=never"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.Pager != ModeNever {
		t.Errorf("Pager = %v, want ModeNever", cfg.Pager)
	}
}

func TestTOMLColorDefaultIsAuto(t *testing.T) {
	if got := mustFromTOML(t, "").Color; got != ModeAuto {
		t.Errorf("Color = %v, want ModeAuto", got)
	}
}

func TestTOMLColorAcceptsEachVariant(t *testing.T) {
	for _, tc := range []struct {
		s    string
		want Mode
	}{{"auto", ModeAuto}, {"always", ModeAlways}, {"never", ModeNever}} {
		cfg := mustFromTOML(t, "[ui]\ncolor = \""+tc.s+"\"\n")
		if cfg.Color != tc.want {
			t.Errorf("ui.color = %q: got %v, want %v", tc.s, cfg.Color, tc.want)
		}
	}
}

func TestTOMLColorRejectsBadValue(t *testing.T) {
	if _, err := FromTOML("[ui]\ncolor = \"sometimes\"\n"); err == nil {
		t.Fatal("want error for bad ui.color value")
	}
}

func TestCLIBareColorSetsAlways(t *testing.T) {
	cfg := Default()
	cfg.Color = ModeNever
	if err := cfg.ApplyCLI([]string{"--color"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.Color != ModeAlways {
		t.Errorf("Color = %v, want ModeAlways", cfg.Color)
	}
}

func TestCLIColorValue(t *testing.T) {
	for _, tc := range []struct {
		s    string
		want Mode
	}{{"auto", ModeAuto}, {"always", ModeAlways}, {"never", ModeNever}} {
		cfg := Default()
		if err := cfg.ApplyCLI([]string{"--color=" + tc.s}); err != nil {
			t.Fatalf("ApplyCLI(--color=%s): %v", tc.s, err)
		}
		if cfg.Color != tc.want {
			t.Errorf("--color=%s: got %v, want %v", tc.s, cfg.Color, tc.want)
		}
	}
}

func TestCLIColorInvalidValueErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--color=sometimes"}); err == nil {
		t.Fatal("want error for --color=sometimes")
	}
}

func TestCLIColorNestedForm(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--ui__color=never"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.Color != ModeNever {
		t.Errorf("Color = %v, want ModeNever", cfg.Color)
	}
}

func TestTOMLCommentsAndBlanksIgnored(t *testing.T) {
	cfg := mustFromTOML(t, "# top comment\n\n[layout]\ndash = \"-\"\n# trailing\n")
	if cfg.Dash != "-" {
		t.Errorf("Dash = %q, want %q", cfg.Dash, "-")
	}
}

func TestCLIBareActivateSetsAlways(t *testing.T) {
	cfg := Default()
	cfg.Activate = ModeNever
	if err := cfg.ApplyCLI([]string{"--activate"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.Activate != ModeAlways {
		t.Errorf("Activate = %v, want ModeAlways", cfg.Activate)
	}
}

func TestCLIActivateValue(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--activate=always"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.Activate != ModeAlways {
		t.Errorf("Activate = %v, want ModeAlways", cfg.Activate)
	}
}

func TestCLINestedKeyDotsViaDoubleUnderscore(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--graph__edges__chars__horizontal=X"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.GraphHorizontal != "X" {
		t.Errorf("GraphHorizontal = %q, want %q", cfg.GraphHorizontal, "X")
	}
}

func TestCLIColorIntAndHex(t *testing.T) {
	cfg := Default()
	err := cfg.ApplyCLI([]string{"--colors__graph-edge=200", "--colors__dash-filler=#aabbcc"})
	if err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if want := []byte("\x1b[38;5;200m"); !bytes.Equal(cfg.EdgeDimOn, want) {
		t.Errorf("EdgeDimOn = %q, want %q", cfg.EdgeDimOn, want)
	}
	if want := []byte("\x1b[38;2;170;187;204m"); !bytes.Equal(cfg.DimOn, want) {
		t.Errorf("DimOn = %q, want %q", cfg.DimOn, want)
	}
}

func TestCLIUnknownKeyErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--bogus=x"}); err == nil {
		t.Fatal("want error for unknown key")
	} else if got, want := err.Error(), "cli: unknown key: bogus"; got != want {
		t.Errorf("err = %q, want %q", got, want)
	}
}

func TestCLIMissingValueErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--layout__dash"}); err == nil {
		t.Fatal("want error for missing value")
	} else if got, want := err.Error(), "cli: missing value for --layout__dash"; got != want {
		t.Errorf("err = %q, want %q", got, want)
	}
}

func TestCLIOverridesExistingValue(t *testing.T) {
	cfg := Default()
	if err := cfg.applyKV("layout.dash", "-", ""); err != nil {
		t.Fatalf("applyKV: %v", err)
	}
	if err := cfg.ApplyCLI([]string{"--layout__dash=="}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.Dash != "=" {
		t.Errorf("Dash = %q, want %q", cfg.Dash, "=")
	}
}

func TestApplyKVUnknownKeyErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.applyKV("nope.x", "v", "env"); err == nil {
		t.Fatal("want error for unknown key")
	} else if got, want := err.Error(), "env: unknown key: nope.x"; got != want {
		t.Errorf("err = %q, want %q", got, want)
	}
}

func TestEnvKeyUppercaseLowercases(t *testing.T) {
	if got := envKeyToConfigKey("ACTIVATE"); got != "activate" {
		t.Errorf("got %q, want %q", got, "activate")
	}
}

func TestEnvKeyDoubleUnderscoreBecomesDot(t *testing.T) {
	got := envKeyToConfigKey("GRAPH__EDGES__CHARS__TOP_LEFT")
	if want := "graph.edges.chars.top-left"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestEnvKeyLowercaseHyphenFormUnchanged(t *testing.T) {
	got := envKeyToConfigKey("graph__edges__chars__top-left")
	if want := "graph.edges.chars.top-left"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestEnvKeySingleUnderscoreBecomesHyphen(t *testing.T) {
	got := envKeyToConfigKey("LAYOUT__DASH_START")
	if want := "layout.dash-start"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestApplyEnvAppliesPrefixedVars(t *testing.T) {
	t.Setenv("BIJJOU__LAYOUT__DASH", ".")
	t.Setenv("BIJJOU__UI__COLOR", "never")
	t.Setenv("BIJJOU_NOT_A_KEY", "x")
	cfg := Default()
	if err := cfg.ApplyEnv(); err != nil {
		t.Fatalf("ApplyEnv: %v", err)
	}
	if cfg.Dash != "." {
		t.Errorf("Dash = %q, want %q", cfg.Dash, ".")
	}
	if cfg.Color != ModeNever {
		t.Errorf("Color = %v, want ModeNever", cfg.Color)
	}
}

func TestApplyEnvUnknownKeyErrors(t *testing.T) {
	t.Setenv("BIJJOU__NOPE", "x")
	cfg := Default()
	if err := cfg.ApplyEnv(); err == nil {
		t.Fatal("want error for unknown env key")
	} else if got, want := err.Error(), "env: unknown key: nope"; got != want {
		t.Errorf("err = %q, want %q", got, want)
	}
}

func TestStreamDefaults(t *testing.T) {
	cfg := mustFromTOML(t, "")
	if !cfg.StreamEnabled {
		t.Error("StreamEnabled = false, want true")
	}
	if want := (BatchSize{Fixed: DefaultStreamBatchSize}); cfg.StreamBatchSize != want {
		t.Errorf("StreamBatchSize = %+v, want %+v", cfg.StreamBatchSize, want)
	}
}

func TestStreamTOMLSection(t *testing.T) {
	cfg := mustFromTOML(t, "[stream]\nenabled = true\nbatch-size = 64\n")
	if !cfg.StreamEnabled {
		t.Error("StreamEnabled = false, want true")
	}
	if want := (BatchSize{Fixed: 64}); cfg.StreamBatchSize != want {
		t.Errorf("StreamBatchSize = %+v, want %+v", cfg.StreamBatchSize, want)
	}
}

func TestStreamTOMLBatchSizeZeroErrors(t *testing.T) {
	if _, err := FromTOML("[stream]\nbatch-size = 0\n"); err == nil {
		t.Fatal("want error for batch-size = 0")
	}
}

func TestStreamTOMLBatchSizeNegativeErrors(t *testing.T) {
	if _, err := FromTOML("[stream]\nbatch-size = -1\n"); err == nil {
		t.Fatal("want error for batch-size = -1")
	}
}

func TestCLIBareStreamSetsEnabled(t *testing.T) {
	cfg := Default()
	cfg.StreamEnabled = false
	if err := cfg.ApplyCLI([]string{"--stream"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if !cfg.StreamEnabled {
		t.Error("StreamEnabled = false, want true")
	}
}

func TestCLIStreamTrue(t *testing.T) {
	cfg := Default()
	cfg.StreamEnabled = false
	if err := cfg.ApplyCLI([]string{"--stream=true"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if !cfg.StreamEnabled {
		t.Error("StreamEnabled = false, want true")
	}
}

func TestCLIStreamFalse(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--stream=false"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.StreamEnabled {
		t.Error("StreamEnabled = true, want false")
	}
}

func TestCLIStreamInvalidValueErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--stream=sometimes"}); err == nil {
		t.Fatal("want error for --stream=sometimes")
	}
}

func TestCLIStreamBatchSize(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--stream", "--stream__batch-size=32"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if !cfg.StreamEnabled {
		t.Error("StreamEnabled = false, want true")
	}
	if want := (BatchSize{Fixed: 32}); cfg.StreamBatchSize != want {
		t.Errorf("StreamBatchSize = %+v, want %+v", cfg.StreamBatchSize, want)
	}
}

func TestCLIStreamBatchSizeZeroErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--stream__batch-size=0"}); err == nil {
		t.Fatal("want error for batch-size 0")
	}
}

func TestCLIStreamBatchSizeNonIntegerErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--stream__batch-size=abc"}); err == nil {
		t.Fatal("want error for non-integer batch-size")
	}
}

func TestStreamTOMLBatchSizeHalfPager(t *testing.T) {
	cfg := mustFromTOML(t, "[stream]\nbatch-size = \"half-pager\"\n")
	if want := (BatchSize{HalfPager: true}); cfg.StreamBatchSize != want {
		t.Errorf("StreamBatchSize = %+v, want %+v", cfg.StreamBatchSize, want)
	}
}

func TestCLIStreamBatchSizeHalfPager(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--stream__batch-size=half-pager"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if want := (BatchSize{HalfPager: true}); cfg.StreamBatchSize != want {
		t.Errorf("StreamBatchSize = %+v, want %+v", cfg.StreamBatchSize, want)
	}
}

func TestLayoutDashDefaults(t *testing.T) {
	cfg := mustFromTOML(t, "")
	if cfg.Dash != DefaultDash || cfg.DashStart != DefaultDashStart || cfg.DashEnd != DefaultDashEnd {
		t.Errorf("dashes = %q/%q/%q", cfg.Dash, cfg.DashStart, cfg.DashEnd)
	}
}

func TestLayoutTOMLDashOverride(t *testing.T) {
	cfg := mustFromTOML(t, "[layout]\ndash = \".\"\ndash-start = \"<\"\ndash-end = \">\"\n")
	if cfg.Dash != "." || cfg.DashStart != "<" || cfg.DashEnd != ">" {
		t.Errorf("dashes = %q/%q/%q, want \".\"/\"<\"/\">\"", cfg.Dash, cfg.DashStart, cfg.DashEnd)
	}
}

func TestCLILayoutDashStart(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--layout__dash-start=<"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.DashStart != "<" {
		t.Errorf("DashStart = %q, want %q", cfg.DashStart, "<")
	}
}

func TestCLILayoutDashEnd(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--layout__dash-end=>"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.DashEnd != ">" {
		t.Errorf("DashEnd = %q, want %q", cfg.DashEnd, ">")
	}
}

func TestGraphCollapseDefaultsOff(t *testing.T) {
	if mustFromTOML(t, "").GraphCollapse {
		t.Error("GraphCollapse = true, want false")
	}
}

func TestGraphCollapseTOMLAndCLI(t *testing.T) {
	if !mustFromTOML(t, "[graph]\ncollapse = true\n").GraphCollapse {
		t.Error("GraphCollapse = false, want true")
	}
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--graph__collapse=true"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if !cfg.GraphCollapse {
		t.Error("GraphCollapse = false, want true")
	}
}

func TestGraphCollapseInvalidValueErrors(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--graph__collapse=sometimes"}); err == nil {
		t.Fatal("want error for non-bool collapse")
	}
}

func TestHydraDefaultsAreOnWithHashedColors(t *testing.T) {
	cfg := mustFromTOML(t, "")
	if !cfg.HydraEnable {
		t.Error("HydraEnable = false, want true")
	}
	if !cfg.HydraTopStackPadding {
		t.Error("HydraTopStackPadding = false, want true")
	}
	if cfg.HydraColors.Mode != HydraColorsHash || cfg.HydraColors.Palette != nil {
		t.Errorf("HydraColors = %+v, want hash", cfg.HydraColors)
	}
	if !cfg.HydraColorBookmarks {
		t.Error("HydraColorBookmarks = false, want true")
	}
	if cfg.HydraPrefixes != defaultHydraPrefixes() {
		t.Errorf("HydraPrefixes = %+v, want defaults", cfg.HydraPrefixes)
	}
}

func TestHydraColorBookmarksParsesFromTOMLAndCLI(t *testing.T) {
	if mustFromTOML(t, "[hydra]\ncolor-bookmarks = false\n").HydraColorBookmarks {
		t.Error("HydraColorBookmarks = true, want false")
	}
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--hydra__color-bookmarks=false"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.HydraColorBookmarks {
		t.Error("HydraColorBookmarks = true, want false")
	}
}

func TestHydraPrefixesComeFromTOMLAndCLI(t *testing.T) {
	cfg := mustFromTOML(t, "[hydra.prefixes]\nprefix = \"ZZ\"\nconflict-resolution = \"RES\"\n")
	if cfg.HydraPrefixes.Prefix != "ZZ" {
		t.Errorf("Prefix = %q, want %q", cfg.HydraPrefixes.Prefix, "ZZ")
	}
	if cfg.HydraPrefixes.ConflictResolution != "RES" {
		t.Errorf("ConflictResolution = %q, want %q", cfg.HydraPrefixes.ConflictResolution, "RES")
	}
	if cfg.HydraPrefixes.StackHead != DefaultHydraStackHead {
		t.Errorf("StackHead = %q, want %q", cfg.HydraPrefixes.StackHead, DefaultHydraStackHead)
	}
	anchors := cfg.HydraPrefixes.Anchors()
	want := []string{"ZZB", "ZZH", "ZZRES"}
	if len(anchors) != len(want) {
		t.Fatalf("Anchors() = %v, want %v", anchors, want)
	}
	for i := range want {
		if anchors[i] != want[i] {
			t.Errorf("Anchors()[%d] = %q, want %q", i, anchors[i], want[i])
		}
	}

	cfg = Default()
	if err := cfg.ApplyCLI([]string{"--hydra__prefixes__stack-working-copy=W"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if got := cfg.HydraPrefixes.WorkingCopy(); got != "HYW-" {
		t.Errorf("WorkingCopy() = %q, want %q", got, "HYW-")
	}
}

func TestHydraPrefixReplacementsComeFromTOMLAndCLI(t *testing.T) {
	set := func(s string) Replacement { return Replacement{Value: s, Set: true} }

	cfg := mustFromTOML(t, "[hydra.prefixes-replace]\nstack-head = \"Ψ\"\n")
	r := &cfg.HydraPrefixesReplace
	// The key that was set stands in. Every other bookmark is untouched.
	if got := cfg.HydraPrefixes.StackMarkerReplace(r); got != set("Ψ") {
		t.Errorf("StackMarkerReplace = %+v, want Ψ", got)
	}
	if got := cfg.HydraPrefixes.WorkingCopyReplace(r); got.Set {
		t.Errorf("WorkingCopyReplace = %+v, want unset", got)
	}
	for i, got := range cfg.HydraPrefixes.AnchorsReplace(r) {
		if got.Set {
			t.Errorf("AnchorsReplace[%d] = %+v, want unset", i, got)
		}
	}

	// `prefix` replaces the shared leader only, and a per-bookmark key wins
	// over `prefix`.
	cfg = mustFromTOML(t, "[hydra.prefixes-replace]\nprefix = \"⋔\"\nbase = \"◆\"\n")
	r = &cfg.HydraPrefixesReplace
	if got := cfg.HydraPrefixes.StackMarkerReplace(r); got != set("⋔S-") {
		t.Errorf("StackMarkerReplace = %+v, want ⋔S-", got)
	}
	if got := cfg.HydraPrefixes.WorkingCopyReplace(r); got != set("⋔WC-") {
		t.Errorf("WorkingCopyReplace = %+v, want ⋔WC-", got)
	}
	wantAnchors := []Replacement{set("◆"), set("⋔H"), set("⋔CR")}
	got := cfg.HydraPrefixes.AnchorsReplace(r)
	if len(got) != len(wantAnchors) {
		t.Fatalf("AnchorsReplace = %+v, want %+v", got, wantAnchors)
	}
	for i := range wantAnchors {
		if got[i] != wantAnchors[i] {
			t.Errorf("AnchorsReplace[%d] = %+v, want %+v", i, got[i], wantAnchors[i])
		}
	}

	cfg = Default()
	if err := cfg.ApplyCLI([]string{"--hydra__prefixes-replace__stack-working-copy=ψ"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if got := cfg.HydraPrefixes.WorkingCopyReplace(&cfg.HydraPrefixesReplace); got != set("ψ") {
		t.Errorf("WorkingCopyReplace = %+v, want ψ", got)
	}
}

func TestHydraColorsBoolForms(t *testing.T) {
	if got := mustFromTOML(t, "[hydra]\ncolors = false\n").HydraColors; got.Mode != HydraColorsOff {
		t.Errorf("HydraColors = %+v, want off", got)
	}
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--hydra__colors=true"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	if cfg.HydraColors.Mode != HydraColorsHash {
		t.Errorf("HydraColors = %+v, want hash", cfg.HydraColors)
	}
}

func wantPalette(t *testing.T, got HydraColors) {
	t.Helper()
	want := [][]byte{[]byte("\x1b[38;5;1m"), []byte("\x1b[38;2;170;187;204m")}
	if got.Mode != HydraColorsPalette || len(got.Palette) != len(want) {
		t.Fatalf("HydraColors = %+v, want palette of %d", got, len(want))
	}
	for i := range want {
		if !bytes.Equal(got.Palette[i], want[i]) {
			t.Errorf("palette[%d] = %q, want %q", i, got.Palette[i], want[i])
		}
	}
}

func TestHydraColorsTOMLListFlattensToAPalette(t *testing.T) {
	wantPalette(t, mustFromTOML(t, "[hydra]\ncolors = [1, \"#aabbcc\"]\n").HydraColors)
}

func TestHydraColorsCLIListIsCommaSeparated(t *testing.T) {
	cfg := Default()
	if err := cfg.ApplyCLI([]string{"--hydra__colors=1,#aabbcc"}); err != nil {
		t.Fatalf("ApplyCLI: %v", err)
	}
	wantPalette(t, cfg.HydraColors)
}

func TestHydraColorsRejectsBadEntries(t *testing.T) {
	for _, arg := range []string{"--hydra__colors=1,999", "--hydra__colors=1,,2", "--hydra__colors=sometimes"} {
		cfg := Default()
		if err := cfg.ApplyCLI([]string{arg}); err == nil {
			t.Errorf("%s: want error", arg)
		}
	}
}

func TestHydraTogglesRejectNonBools(t *testing.T) {
	for _, arg := range []string{"--hydra__enable=sometimes", "--hydra__top-stack-padding=sometimes"} {
		cfg := Default()
		if err := cfg.ApplyCLI([]string{arg}); err == nil {
			t.Errorf("%s: want error", arg)
		}
	}
}

func TestTOMLDatetimeNotSupported(t *testing.T) {
	_, err := FromTOML("when = 1979-05-27\n")
	if err == nil {
		t.Fatal("want error for a datetime value")
	}
	if want := "when: datetimes not supported"; err.Error() != want {
		t.Errorf("err = %q, want %q", err.Error(), want)
	}
}

func TestTOMLKeysApplyInSortedOrder(t *testing.T) {
	_, err := FromTOML("zbogus = 1\nabogus = 2\n")
	if err == nil {
		t.Fatal("want error for unknown keys")
	}
	if want := "unknown key: abogus"; err.Error() != want {
		t.Errorf("err = %q, want %q", err.Error(), want)
	}
}

func TestTOMLArrayOfNonScalarsErrors(t *testing.T) {
	if _, err := FromTOML("[hydra]\ncolors = [[1]]\n"); err == nil {
		t.Fatal("want error for array of arrays")
	}
}

func TestTemplatesKeyMustBeFlat(t *testing.T) {
	cfg := Default()
	if err := cfg.applyKV("templates.log_oneline", "x", ""); err != nil {
		t.Fatalf("applyKV: %v", err)
	}
	if cfg.Templates["log_oneline"] != "x" {
		t.Errorf("templates = %v", cfg.Templates)
	}
	err := cfg.applyKV("templates.a.b", "x", "")
	if err == nil {
		t.Fatal("want error for nested template name")
	}
	want := "templates.a.b: expected `templates.<name>` with a flat name, got \"templates.a.b\""
	if err.Error() != want {
		t.Errorf("err = %q, want %q", err.Error(), want)
	}
}

func TestLoadReadsNamedConfigPath(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(path, []byte("[layout]\ndash = \".\"\n"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	t.Setenv("BIJJOU_CONFIG", path)
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.Dash != "." {
		t.Errorf("Dash = %q, want %q", cfg.Dash, ".")
	}
}

func TestLoadMissingNamedConfigFallsBackToDefaults(t *testing.T) {
	t.Setenv("BIJJOU_CONFIG", filepath.Join(t.TempDir(), "nope.toml"))
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.Dash != DefaultDash {
		t.Errorf("Dash = %q, want %q", cfg.Dash, DefaultDash)
	}
}

func TestLoadBadConfigErrorNamesThePath(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(path, []byte("bogus = 1\n"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	t.Setenv("BIJJOU_CONFIG", path)
	_, err := Load()
	if err == nil {
		t.Fatal("want error for unknown key")
	}
	if want := path + ": unknown key: bogus"; err.Error() != want {
		t.Errorf("err = %q, want %q", err.Error(), want)
	}
}
