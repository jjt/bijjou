package main_test

import (
	"bytes"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"unicode/utf8"
)

// update rewrites the golden files under testdata/golden instead of comparing
// against them.
var update = flag.Bool("update", false, "rewrite the golden files in testdata/golden")

var (
	// binPath is the bijjou binary every test pipes its fixture through.
	binPath string
	// rootDir is the repository root, which is also the test working
	// directory.
	rootDir string
)

// TestMain builds the binary under test. BIJJOU_TEST_BIN names an existing
// executable to use instead, so the harness can run against a binary the test
// run did not build.
func TestMain(m *testing.M) {
	flag.Parse()

	var err error
	rootDir, err = os.Getwd()
	if err != nil {
		fmt.Fprintf(os.Stderr, "working directory: %v\n", err)
		os.Exit(1)
	}

	tmp, err := prepareBinary()
	if err != nil {
		fmt.Fprintf(os.Stderr, "%v\n", err)
		os.Exit(1)
	}

	code := m.Run()
	if tmp != "" {
		os.RemoveAll(tmp)
	}
	os.Exit(code)
}

// prepareBinary sets binPath. It returns the temporary directory the binary
// was built into, or the empty string when BIJJOU_TEST_BIN supplied one.
func prepareBinary() (string, error) {
	if name := os.Getenv("BIJJOU_TEST_BIN"); name != "" {
		if info, err := os.Stat(name); err == nil && !info.IsDir() && info.Mode().Perm()&0o111 != 0 {
			abs, err := filepath.Abs(name)
			if err != nil {
				return "", fmt.Errorf("BIJJOU_TEST_BIN: %w", err)
			}
			binPath = abs
			return "", nil
		}
	}

	tmp, err := os.MkdirTemp("", "bijjou-golden")
	if err != nil {
		return "", fmt.Errorf("temporary directory: %w", err)
	}
	binPath = filepath.Join(tmp, "bijjou")
	build := exec.Command("go", "build", "-o", binPath, ".")
	build.Dir = rootDir
	build.Stdout = os.Stderr
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		os.RemoveAll(tmp)
		return "", fmt.Errorf("go build: %w", err)
	}
	return tmp, nil
}

// visualize renders bytes for the golden files. CSI escapes appear as \e[...X
// so the parameters and the final byte stay visible and stable. Control
// characters become \xNN. Valid UTF-8 passes through. Newlines stay literal so
// multiline output reads naturally.
func visualize(b []byte) string {
	var out strings.Builder
	i := 0
	for i < len(b) {
		c := b[i]
		if c == 0x1b {
			if i+1 < len(b) && b[i+1] == '[' {
				j := i + 2
				for j < len(b) && !(b[j] >= 0x40 && b[j] <= 0x7e) {
					j++
				}
				if j < len(b) {
					j++
					params := "?"
					if raw := b[i+2 : j-1]; utf8.Valid(raw) {
						params = string(raw)
					}
					out.WriteString("\\e[")
					out.WriteString(params)
					out.WriteByte(b[j-1])
					i = j
					continue
				}
				out.WriteString("\\e[")
				i += 2
				continue
			}
			out.WriteString("\\e")
			i++
			continue
		}
		if c == '\n' {
			out.WriteByte('\n')
			i++
			continue
		}
		if c < 0x20 || c == 0x7f {
			fmt.Fprintf(&out, "\\x%02x", c)
			i++
			continue
		}
		if c < 0x80 {
			out.WriteByte(c)
			i++
			continue
		}
		length := 1
		switch {
		case c >= 0xf0:
			length = 4
		case c >= 0xe0:
			length = 3
		case c >= 0xc0:
			length = 2
		}
		end := i + length
		if end > len(b) {
			end = len(b)
		}
		if utf8.Valid(b[i:end]) {
			out.Write(b[i:end])
			i = end
			continue
		}
		fmt.Fprintf(&out, "\\x%02x", c)
		i++
	}
	return out.String()
}

// renderFixtureWith pipes a fixture through bijjou under bijjou-config.toml
// and visualizes the result. ui.color is forced to "always" so the SGR
// sequences are stable whether or not the test runner is a terminal. env adds
// further BIJJOU__ overrides.
func renderFixtureWith(t *testing.T, fixture string, env ...string) string {
	t.Helper()
	return renderFixtureUnder(t, fixture, filepath.Join(rootDir, "bijjou-config.toml"), env...)
}

// renderFixtureUnder does the same under a configuration file of the test's
// own, for a template the stock configuration does not carry, say.
func renderFixtureUnder(t *testing.T, fixture, config string, env ...string) string {
	t.Helper()

	path := filepath.Join(rootDir, "testdata", "fixtures", fixture)
	input, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("%v", err)
	}

	cmd := exec.Command(binPath)
	cmd.Dir = rootDir
	cmd.Env = append(os.Environ(),
		"BIJJOU_CONFIG="+config,
		"BIJJOU__UI__COLOR=always",
		// Hydra markup is opt-in per test: without this the result would
		// depend on whether the checkout bijjou runs in happens to be a hydra,
		// and on whether hydra is on PATH at all.
		"BIJJOU__HYDRA__ENABLE=false",
	)
	cmd.Env = append(cmd.Env, env...)
	cmd.Stdin = bytes.NewReader(input)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		t.Fatalf("%s: %v\nstderr:\n%s", binPath, err, stderr.String())
	}

	return visualize(stdout.Bytes())
}

func renderFixture(t *testing.T, fixture string) string {
	t.Helper()
	return renderFixtureWith(t, fixture)
}

// hydraEnv is hydra markup on, plus graph.collapse, which is how the hydra
// logs this is modelled on are actually read.
func hydraEnv(extra ...string) []string {
	env := []string{
		"BIJJOU__HYDRA__ENABLE=true",
		"BIJJOU__GRAPH__COLLAPSE=true",
	}
	return append(env, extra...)
}

// checkGolden compares the visualized output against testdata/golden/<name>.txt.
// The -update flag rewrites that file instead.
func checkGolden(t *testing.T, name, got string) {
	t.Helper()

	path := filepath.Join(rootDir, "testdata", "golden", name+".txt")
	if *update {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatalf("%v", err)
		}
		if err := os.WriteFile(path, []byte(got), 0o644); err != nil {
			t.Fatalf("%v", err)
		}
		return
	}

	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("%v (run `go test -update` to write it)", err)
	}
	if string(want) == got {
		return
	}
	t.Errorf("output does not match %s (run `go test -update` to accept):\n%s",
		path, goldenDiff(string(want), got))
}

// goldenDiff reports the differing lines, want against got, with the line
// numbers to find them by.
func goldenDiff(want, got string) string {
	wantLines := strings.Split(want, "\n")
	gotLines := strings.Split(got, "\n")

	var out strings.Builder
	shown := 0
	n := max(len(wantLines), len(gotLines))
	for i := 0; i < n && shown < 20; i++ {
		var w, g string
		if i < len(wantLines) {
			w = wantLines[i]
		}
		if i < len(gotLines) {
			g = gotLines[i]
		}
		if w == g {
			continue
		}
		fmt.Fprintf(&out, "line %d:\n  want: %q\n   got: %q\n", i+1, w, g)
		shown++
	}
	if shown == 0 {
		fmt.Fprintf(&out, "want %d bytes, got %d bytes\n", len(want), len(got))
	}
	return out.String()
}

// TestGoldenLocal renders local.txt under bijjou-config.toml (matches
// out.local.txt).
func TestGoldenLocal(t *testing.T) {
	checkGolden(t, "local", renderFixture(t, "local.txt"))
}

// TestGoldenCustomNodes is a regression: rows whose graph node is a custom
// log_node glyph (■ U+25A0, Nerd-Font PUA U+F28D) must render, not pass
// through raw. Built-in nodes (●) and edges share the fixture for contrast.
// Guards the structural node detection in render.
func TestGoldenCustomNodes(t *testing.T) {
	checkGolden(t, "custom_nodes", renderFixture(t, "custom_nodes.txt"))
}

// TestGoldenLocalCollapsed covers graph.collapse = true: jj's inter-column pad
// cells are dropped, so every graph column sits one cell from the last and the
// graph-to-content gap shrinks with it. Guards the pad-cell test plus the
// collapsed graph column and last-is-edge pair that feeds the dash run.
func TestGoldenLocalCollapsed(t *testing.T) {
	checkGolden(t, "local_collapsed",
		renderFixtureWith(t, "local.txt", "BIJJOU__GRAPH__COLLAPSE=true"))
}

// TestGoldenHydra renders a hydra log: hydra.top-stack-padding draws the
// separator row jj skips under the log's top stack (delta here), hydra.colors
// colours each stack's nodes — the hashed default, so this also pins the name
// to colour mapping, and each HYWC-* row up top matching its own stack — and
// hydra.color-bookmarks puts that same colour on the HYS-* and HYWC-* names
// themselves. The bookmark naming comes from hydra.prefixes, so no hydra call
// and no live hydra are involved. The row above HYB is an extra head off the
// base, in no stack and in its own graph column, so it keeps jj's colours.
func TestGoldenHydra(t *testing.T) {
	checkGolden(t, "hydra", renderFixtureWith(t, "hydra.txt", hydraEnv()...))
}

// TestGoldenHydraPalette covers hydra.colors = [...]: the palette is indexed by
// the order the log first names each stack — the working-copy rows up top here
// (delta to 1, gamma to 2, beta to 3, alpha to 4), and each stack matches its
// own working copy.
func TestGoldenHydraPalette(t *testing.T) {
	env := hydraEnv("BIJJOU__HYDRA__COLORS=1,2,3,4")
	checkGolden(t, "hydra_palette", renderFixtureWith(t, "hydra.txt", env...))
}

// TestGoldenHydraDisabled turns both hydra knobs off: the result is
// byte-for-byte the pre-hydra rendering, so a repository with no hydra (or
// hydra.enable = false) is unaffected.
func TestGoldenHydraDisabled(t *testing.T) {
	env := hydraEnv(
		"BIJJOU__HYDRA__TOP_STACK_PADDING=false",
		"BIJJOU__HYDRA__COLORS=false",
	)
	got := renderFixtureWith(t, "hydra.txt", env...)
	want := renderFixtureWith(t, "hydra.txt", "BIJJOU__GRAPH__COLLAPSE=true")
	if got != want {
		t.Errorf("hydra knobs off differs from the pre-hydra rendering:\n%s", goldenDiff(want, got))
	}
}

// TestGoldenHydraPrefixesRename covers hydra.prefixes: rename the prefix and
// the fixture's HY* bookmarks stop being hydra bookmarks, so the markup drops
// out entirely.
func TestGoldenHydraPrefixesRename(t *testing.T) {
	env := hydraEnv("BIJJOU__HYDRA__PREFIXES__PREFIX=ZZ")
	got := renderFixtureWith(t, "hydra.txt", env...)
	want := renderFixtureWith(t, "hydra.txt", "BIJJOU__GRAPH__COLLAPSE=true")
	if got != want {
		t.Errorf("renamed prefix differs from the pre-hydra rendering:\n%s", goldenDiff(want, got))
	}
}

// TestGoldenHydraPlainBookmarks covers hydra.color-bookmarks = false: the
// stacks keep their node colours, but every bookmark name is left in jj's own
// colour (magenta here).
func TestGoldenHydraPlainBookmarks(t *testing.T) {
	env := hydraEnv("BIJJOU__HYDRA__COLOR_BOOKMARKS=false")
	off := renderFixtureWith(t, "hydra.txt", env...)
	on := renderFixtureWith(t, "hydra.txt", hydraEnv()...)
	// delta's hashed colour, which its node carries in both renderings.
	delta := "\\e[38;2;156;224;92m"
	// The node glyph is the Nerd-Font PUA U+F28D.
	if !strings.Contains(off, delta+"\uf28d") {
		t.Errorf("%s", off)
	}
	if !strings.Contains(off, "\\e[38;5;5mHYS-delta") {
		t.Errorf("%s", off)
	}
	if strings.Contains(off, delta+"HYS-delta") {
		t.Errorf("%s", off)
	}
	if !strings.Contains(on, delta+"HYS-delta") {
		t.Errorf("%s", on)
	}
	if !strings.Contains(on, delta+"HYWC-delta") {
		t.Errorf("%s", on)
	}
	// The anchors are nobody's stack, so they keep jj's colour either way.
	if !strings.Contains(off, "\\e[38;5;5mHYH") {
		t.Errorf("%s", off)
	}
	if !strings.Contains(on, "\\e[38;5;5mHYH") {
		t.Errorf("%s", on)
	}
}

// TestGoldenHydraPrefixesReplace covers hydra.prefixes-replace: stack-head and
// stack-working-copy stand in for the whole leader, dash included, so
// HYS-delta reads Ψdelta and HYWC-delta reads ψdelta; head replaces the whole
// HYH name. base and conflict-resolution are unset, so HYB main is left as jj
// printed it. The stack colours are unchanged — the names are renamed, not
// reclassified.
func TestGoldenHydraPrefixesReplace(t *testing.T) {
	env := hydraEnv(
		"BIJJOU__HYDRA__PREFIXES_REPLACE__STACK_HEAD=Ψ",
		"BIJJOU__HYDRA__PREFIXES_REPLACE__STACK_WORKING_COPY=ψ",
		"BIJJOU__HYDRA__PREFIXES_REPLACE__HEAD=◆",
	)
	checkGolden(t, "hydra_prefixes_replace", renderFixtureWith(t, "hydra.txt", env...))
}

// TestGoldenHydraPrefixesReplaceKeepsElasticTabsAligned checks that pass 1
// measures the elastic-tab anchors on the replaced field: a stand-in is not the
// width of the name it replaces. With a tab behind %{bookmarks} and a stand-in
// wider than the leader, every row's tab must still land in one column.
func TestGoldenHydraPrefixesReplaceKeepsElasticTabsAligned(t *testing.T) {
	config := filepath.Join(t.TempDir(), "tab-after-bookmarks.toml")
	body := "[templates]\nlog_oneline = ''' %{elastic_tab(change_id)} %{bookmarks} %{elastic_tab()}|%{description}'''\n"
	if err := os.WriteFile(config, []byte(body), 0o644); err != nil {
		t.Fatalf("%v", err)
	}
	env := hydraEnv("BIJJOU__HYDRA__PREFIXES_REPLACE__STACK_HEAD=stack/")
	out := renderFixtureUnder(t, "hydra.txt", config, env...)
	if !strings.Contains(out, "stack/delta") {
		t.Fatalf("%s", out)
	}

	// Rule 2 collapses a cell on the rows whose bookmarks field is empty, so
	// the comparison is over the rows that carry a bookmark: a replaced name
	// and an untouched one have to land in the same column.
	var columns []int
	for _, raw := range strings.Split(out, "\n") {
		line := plain(strings.TrimSuffix(raw, "\r"))
		if !strings.Contains(line, "|hydra ") && !strings.Contains(line, "|Update A") {
			continue
		}
		if col := runeIndex(line, '|'); col >= 0 {
			columns = append(columns, col)
		}
	}
	// Four working copies, the head, four stack markers and the base.
	if len(columns) != 10 {
		t.Fatalf("got %d bookmark rows, want 10\n%s", len(columns), out)
	}
	for _, col := range columns {
		if col != columns[0] {
			t.Fatalf("%v\n%s", columns, out)
		}
	}
}

// plain is one visualized line with its \e[...X sequences taken back out, so a
// column count is a column count.
func plain(line string) string {
	var out strings.Builder
	rest := line
	for {
		at := strings.Index(rest, "\\e[")
		if at < 0 {
			break
		}
		out.WriteString(rest[:at])
		rest = rest[at+3:]
		end := strings.IndexFunc(rest, func(r rune) bool { return r >= '@' && r <= '~' })
		if end < 0 {
			return out.String()
		}
		rest = rest[end+1:]
	}
	out.WriteString(rest)
	return out.String()
}

// runeIndex is the position of the first want character, counted in
// characters, or -1 when the line has none.
func runeIndex(line string, want rune) int {
	col := 0
	for _, r := range line {
		if r == want {
			return col
		}
		col++
	}
	return -1
}
