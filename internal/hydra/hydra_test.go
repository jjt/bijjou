package hydra

import (
	"bytes"
	"math"
	"strconv"
	"strings"
	"testing"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
)

func testTopology() *topology {
	c := config.Default()
	return newTopology(&c.HydraPrefixes, &config.HydraPrefixReplace{})
}

// The same naming with `hydra.prefixes-replace` in force.
func topologyWith(r config.HydraPrefixReplace) *topology {
	c := config.Default()
	return newTopology(&c.HydraPrefixes, &r)
}

// One set `hydra.prefixes-replace` key.
func stands(s string) config.Replacement {
	return config.Replacement{Value: s, Set: true}
}

func TestDefaultPrefixesExpandToTheStockBookmarkNames(t *testing.T) {
	topo := testTopology()
	if topo.stackPrefix != "HYS-" {
		t.Errorf("stackPrefix = %q, want %q", topo.stackPrefix, "HYS-")
	}
	if topo.wcPrefix != "HYWC-" {
		t.Errorf("wcPrefix = %q, want %q", topo.wcPrefix, "HYWC-")
	}
	if want := []string{"HYB", "HYH", "HYCR"}; !equalStrings(topo.anchors, want) {
		t.Errorf("anchors = %v, want %v", topo.anchors, want)
	}
}

func TestRenamedPrefixesExpandToTheRepoNames(t *testing.T) {
	p := config.Default().HydraPrefixes
	p.Prefix = "ZZ"
	p.StackHead = "ST"
	topo := newTopology(&p, &config.HydraPrefixReplace{})
	if topo.stackPrefix != "ZZST-" {
		t.Errorf("stackPrefix = %q, want %q", topo.stackPrefix, "ZZST-")
	}
	if topo.wcPrefix != "ZZWC-" {
		t.Errorf("wcPrefix = %q, want %q", topo.wcPrefix, "ZZWC-")
	}
	if want := []string{"ZZB", "ZZH", "ZZCR"}; !equalStrings(topo.anchors, want) {
		t.Errorf("anchors = %v, want %v", topo.anchors, want)
	}
}

// The row's `bookmarks` field as it renders, with names replaced and nothing
// recolored. A false hit means "no name to take over", which leaves the caller
// with jj's own field.
func rewritten(topo *topology, bookmarks string) (string, bool) {
	var scratch, out []byte
	hit := rewriteBookmarks(topo, nil, &scratch, []byte(bookmarks), &out)
	return string(out), hit
}

func wantRewrite(t *testing.T, topo *topology, bookmarks, want string) {
	t.Helper()
	got, hit := rewritten(topo, bookmarks)
	if !hit {
		t.Errorf("rewrite %q was skipped, want %q", bookmarks, want)
		return
	}
	if got != want {
		t.Errorf("rewrite %q = %q, want %q", bookmarks, got, want)
	}
}

func wantNoRewrite(t *testing.T, topo *topology, bookmarks string) {
	t.Helper()
	if got, hit := rewritten(topo, bookmarks); hit {
		t.Errorf("rewrite %q = %q, want the field left alone", bookmarks, got)
	}
}

func TestAPerBookmarkKeyStandsInForTheWholeLeader(t *testing.T) {
	topo := topologyWith(config.HydraPrefixReplace{
		Base:             stands("◆"),
		StackHead:        stands("Ψ"),
		StackWorkingCopy: stands("ψ"),
	})
	// The dash belongs to the leader, so it goes with it.
	wantRewrite(t, topo, "HYS-foo", "Ψfoo")
	wantRewrite(t, topo, "HYWC-foo", "ψfoo")
	// An anchor is a whole name, so the stand-in is the whole name.
	wantRewrite(t, topo, "HYB main", "◆ main")
	// jj's out-of-sync flag is not part of the name and rides along.
	wantRewrite(t, topo, "HYS-foo*", "Ψfoo*")
	// A key left unset leaves the bookmarks it names alone.
	wantNoRewrite(t, topo, "HYH")
	wantNoRewrite(t, topo, "HYCR")
}

func TestPrefixAloneStandsInForTheSharedLeader(t *testing.T) {
	topo := topologyWith(config.HydraPrefixReplace{Prefix: stands("⋔")})
	wantRewrite(t, topo, "HYS-foo", "⋔S-foo")
	wantRewrite(t, topo, "HYWC-foo", "⋔WC-foo")
	wantRewrite(t, topo, "HYB", "⋔B")
	wantRewrite(t, topo, "HYH", "⋔H")
	wantRewrite(t, topo, "HYCR", "⋔CR")
}

func TestAPerBookmarkKeyWinsOverPrefix(t *testing.T) {
	topo := topologyWith(config.HydraPrefixReplace{
		Prefix:    stands("⋔"),
		StackHead: stands("Ψ"),
	})
	wantRewrite(t, topo, "HYS-foo", "Ψfoo")
	wantRewrite(t, topo, "HYWC-foo", "⋔WC-foo")
}

func TestOnlyThisReposHydraBookmarksAreReplaced(t *testing.T) {
	topo := topologyWith(config.HydraPrefixReplace{Prefix: stands("⋔")})
	wantNoRewrite(t, topo, "main jjt/foo")
	// A marker that only exists on a remote is not a local bookmark.
	wantNoRewrite(t, topo, "HYS-foo@origin")
	// A bare leader names no stack.
	wantNoRewrite(t, topo, "HYS-")
	// Neighbors on the row pass through beside the replaced name.
	wantRewrite(t, topo, "main HYS-foo v1", "main ⋔S-foo v1")
}

func TestAReplacedNameStillTakesItsStackColour(t *testing.T) {
	topo := topologyWith(config.HydraPrefixReplace{StackHead: stands("Ψ")})
	var seen []string
	var scratch, out []byte
	hit := rewriteBookmarks(topo, &seen, &scratch, []byte("\x1b[38;5;5mHYS-foo\x1b[39m"), &out)
	if !hit {
		t.Fatalf("rewrite was skipped")
	}
	// jj's foreground drops out, and the stack's color leads the stand-in.
	want := append([]byte(nil), stackColor("foo", 0)...)
	want = append(want, "Ψfoo"...)
	want = append(want, ansi.FGReset...)
	if !bytes.Equal(out, want) {
		t.Errorf("rewrite = %q, want %q", out, want)
	}
}

func markDefault(bookmarks string) mark {
	return markOf(testTopology(), []byte(bookmarks))
}

func TestStackMarkerOpensItsStack(t *testing.T) {
	if m := markDefault("HYS-delta"); m.kind != markStack || m.name != "delta" {
		t.Errorf("mark(HYS-delta) = %v/%q, want stack of delta", m.kind, m.name)
	}
	// jj's out-of-sync flag is not part of the name.
	if m := markDefault("HYS-delta*"); m.kind != markStack || m.name != "delta" {
		t.Errorf("mark(HYS-delta*) = %v/%q, want stack of delta", m.kind, m.name)
	}
}

func TestAnchorsAreOutsideEveryStack(t *testing.T) {
	for _, bookmarks := range []string{"HYB main", "HYH", "HYCR"} {
		if m := markDefault(bookmarks); m.kind != markOutside {
			t.Errorf("mark(%q) = %v, want outside", bookmarks, m.kind)
		}
	}
}

func TestWorkingCopiesNameTheirStack(t *testing.T) {
	for _, bookmarks := range []string{"HYWC-delta", "HYWC-delta*"} {
		if m := markDefault(bookmarks); m.kind != markWorkingCopy || m.name != "delta" {
			t.Errorf("mark(%q) = %v/%q, want working copy of delta", bookmarks, m.kind, m.name)
		}
	}
}

func TestContentCommitsStayInTheStackAboveThem(t *testing.T) {
	for _, bookmarks := range []string{"", "jjt/delta"} {
		if m := markDefault(bookmarks); m.kind != markInside {
			t.Errorf("mark(%q) = %v, want inside", bookmarks, m.kind)
		}
	}
}

func TestRemoteRefsAreNotLocalBookmarks(t *testing.T) {
	// A row's bookmarks carry remote refs. A stack marker that only exists on
	// a remote must not open a stack locally.
	for _, bookmarks := range []string{"HYS-delta@origin", "HYWC-delta@origin"} {
		if m := markDefault(bookmarks); m.kind != markInside {
			t.Errorf("mark(%q) = %v, want inside", bookmarks, m.kind)
		}
	}
}

func testWalk() *Walk {
	return &Walk{topo: testTopology()}
}

// One commit row with `bookmarks` set, under the default config (hashed
// colors) and an empty graph prefix, so no padding row is in play.
func row(w *Walk, bookmarks string) (node, names []byte) {
	return rowAt(w, "", bookmarks)
}

func node(w *Walk, bookmarks string) []byte {
	n, _ := rowAt(w, "", bookmarks)
	return n
}

// The same, with the row's graph prefix, so the stack's column is in play.
// prefix is jj's own drawing, one node glyph among the edges.
func nodeAt(w *Walk, prefix, bookmarks string) []byte {
	n, _ := rowAt(w, prefix, bookmarks)
	return n
}

func rowAt(w *Walk, prefix, bookmarks string) (node, names []byte) {
	fields := map[string][]byte{BookmarksField: []byte(bookmarks)}
	var out bytes.Buffer
	markup := w.Markup(fields, []byte(prefix), &out)
	return bytes.Clone(markup.Node), bytes.Clone(markup.Bookmarks)
}

func TestACommitOutsideTheStacksTakesNoStackColour(t *testing.T) {
	w := testWalk()
	// The log's bottom stack: marker and content in column 0.
	marker := nodeAt(w, "● │ ", "HYS-alpha")
	if marker == nil {
		t.Fatalf("marker is not coloured")
	}
	if got := nodeAt(w, "● │ ", ""); !bytes.Equal(got, marker) {
		t.Errorf("content node = %q, want %q", got, marker)
	}
	// An extra head off the base, drawn in its own column once the stack
	// closed above it. This head is nobody's stack, so jj's colors stand. The
	// stack does not resume below it either.
	if got := nodeAt(w, "│ ● ", ""); got != nil {
		t.Errorf("node in another column = %q, want jj's own colors", got)
	}
	if got := nodeAt(w, "● │ ", ""); got != nil {
		t.Errorf("node below it = %q, want jj's own colors", got)
	}
}

func TestAWorkingCopyTakesItsStackColourWithoutCarryingIt(t *testing.T) {
	w := testWalk()
	wc := node(w, "HYWC-delta")
	if wc == nil {
		t.Fatalf("working copy is not coloured")
	}
	// The head sits between the working copies and the stacks. It is
	// uncolored, and it carries nothing down from the working copy above it.
	if got := node(w, "HYH"); got != nil {
		t.Errorf("head node = %q, want uncoloured", got)
	}
	if got := node(w, ""); got != nil {
		t.Errorf("row under the head = %q, want uncoloured", got)
	}
	// The stack itself, and its content commits, match its working copy.
	marker := node(w, "HYS-delta")
	if marker == nil {
		t.Fatalf("stack marker is not coloured")
	}
	if !bytes.Equal(marker, wc) {
		t.Errorf("marker = %q, want the working copy's %q", marker, wc)
	}
	if got := node(w, ""); !bytes.Equal(got, marker) {
		t.Errorf("content node = %q, want %q", got, marker)
	}
}

func TestHydraBookmarkNamesTakeTheirStackColour(t *testing.T) {
	w := testWalk()
	sgr, bookmarks := row(w, "\x1b[38;5;5mHYS-delta\x1b[39m")
	if sgr == nil {
		t.Fatalf("stack marker is not coloured")
	}
	if bookmarks == nil {
		t.Fatalf("its name is not recoloured")
	}
	// jj's own foreground drops out, and the stack's color leads the name.
	want := append([]byte(nil), sgr...)
	want = append(want, "HYS-delta"...)
	want = append(want, ansi.FGReset...)
	if !bytes.Equal(bookmarks, want) {
		t.Errorf("bookmarks = %q, want %q", bookmarks, want)
	}
	// The working copy of the same stack reads the same.
	_, wc := row(w, "HYWC-delta")
	if wc == nil {
		t.Fatalf("working copy name is not recoloured")
	}
	if !bytes.HasPrefix(wc, sgr) {
		t.Errorf("working copy bookmarks = %q, want the %q prefix", wc, sgr)
	}
}

func TestBookmarksOutsideTheHydraKeepJjsColours(t *testing.T) {
	w := testWalk()
	// An anchor row, a plain bookmark, and a remote-only stack marker are not
	// a stack's name. So the field passes through.
	for _, bookmarks := range []string{"\x1b[38;5;5mHYB main\x1b[39m", "jjt/delta", "HYS-delta@origin"} {
		if _, got := row(w, bookmarks); got != nil {
			t.Errorf("row(%q) rewrote the field to %q", bookmarks, got)
		}
	}
}

func TestAHydraBookmarkIsRecolouredBesideItsNeighbours(t *testing.T) {
	w := testWalk()
	sgr, bookmarks := row(w, "main HYS-delta* v1")
	if sgr == nil {
		t.Fatalf("stack marker is not coloured")
	}
	if bookmarks == nil {
		t.Fatalf("its name is not recoloured")
	}
	want := []byte("main ")
	want = append(want, sgr...)
	want = append(want, "HYS-delta*"...)
	want = append(want, ansi.FGReset...)
	want = append(want, " v1"...)
	if !bytes.Equal(bookmarks, want) {
		t.Errorf("bookmarks = %q, want %q", bookmarks, want)
	}
}

func TestPaletteIndexFollowsLogOrderThenFirstSight(t *testing.T) {
	var seen []string
	steps := []struct {
		name string
		want int
	}{
		{"delta", 0},
		{"gamma", 1},
		// Later sightings append, stably.
		{"later", 2},
		{"other", 3},
		{"later", 2},
	}
	for _, step := range steps {
		if got := indexOf(&seen, step.name); got != step.want {
			t.Errorf("indexOf(%q) = %d, want %d", step.name, got, step.want)
		}
	}
}

func TestHashedColorsAreStableAndPerName(t *testing.T) {
	if !bytes.Equal(hashColor("delta"), hashColor("delta")) {
		t.Errorf("hashColor(delta) is not stable")
	}
	if bytes.Equal(hashColor("delta"), hashColor("gamma")) {
		t.Errorf("hashColor(delta) == hashColor(gamma) = %q", hashColor("delta"))
	}
}

func TestHashedColorsAreTruecolorSGR(t *testing.T) {
	text := string(hashColor("delta"))
	if !strings.HasPrefix(text, "\x1b[38;2;") {
		t.Errorf("hashColor(delta) = %q, want a truecolor prefix", text)
	}
	if !strings.HasSuffix(text, "m") {
		t.Errorf("hashColor(delta) = %q, want an SGR terminator", text)
	}
}

func TestHueSpaceIsTheCircleLessTheReservedBands(t *testing.T) {
	if want := uint64(360 - 21 - 21); hueSpace != want {
		t.Errorf("hueSpace = %d, want %d", hueSpace, want)
	}
}

func TestEveryHueOnOfferClearsTheReservedBands(t *testing.T) {
	last := -1
	for index := uint32(0); index < uint32(hueSpace); index++ {
		hue := hueOf(index)
		if hue >= 360 {
			t.Fatalf("index %d left the circle at %d", index, hue)
		}
		for _, band := range reservedHues {
			if hue >= band[0] && hue <= band[1] {
				t.Fatalf("index %d landed on reserved hue %d", index, hue)
			}
		}
		// Strictly increasing, so no hue is handed out twice.
		if int(hue) <= last {
			t.Fatalf("index %d went back to %d", index, hue)
		}
		last = int(hue)
	}
	// The bands are skipped, not clipped. The circle's top is still in play.
	if got := hueOf(0); got != 0 {
		t.Errorf("hueOf(0) = %d, want 0", got)
	}
	if got := hueOf(uint32(hueSpace) - 1); got != 359 {
		t.Errorf("hueOf(last) = %d, want 359", got)
	}
	// Either side of each band.
	if got := hueOf(104); got != 104 {
		t.Errorf("hueOf(104) = %d, want 104", got)
	}
	if got := hueOf(105); got != 126 {
		t.Errorf("hueOf(105) = %d, want 126", got)
	}
}

func TestHashedColorsAvoidTheReservedHues(t *testing.T) {
	// The reserved bands are on hue, so the check is on hue. This check
	// rebuilds the hue from the rgb the hash actually emitted. `report` hashed
	// into the pink band before the bands existed.
	names := []string{"alpha", "beta", "gamma", "delta", "report", "retry", "green", "pink"}
	for _, name := range names {
		hue := hueOfSGR(t, hashColor(name))
		for _, band := range reservedHues {
			if hue+1.0 < float64(band[0]) || hue > float64(band[1])+1.0 {
				continue
			}
			t.Errorf("%s hashed into reserved hue %v", name, hue)
		}
	}
}

// The hue of a `\x1b[38;2;r;g;bm` SGR, back out of the rgb.
func hueOfSGR(t *testing.T, sgr []byte) float64 {
	t.Helper()
	text := string(sgr)
	body, ok := strings.CutPrefix(text, "\x1b[38;2;")
	if ok {
		body, ok = strings.CutSuffix(body, "m")
	}
	if !ok {
		t.Fatalf("%q is not a truecolor sgr", text)
	}
	parts := strings.Split(body, ";")
	rgb := make([]float64, len(parts))
	for i, part := range parts {
		v, err := strconv.ParseFloat(part, 64)
		if err != nil {
			t.Fatalf("%q holds no rgb: %v", text, err)
		}
		rgb[i] = v / 255.0
	}
	r, g, b := rgb[0], rgb[1], rgb[2]
	max := math.Max(math.Max(r, g), b)
	min := math.Min(math.Min(r, g), b)
	delta := max - min
	if delta <= 0.0 {
		t.Fatalf("grey has no hue: %v", rgb)
	}
	var hue float64
	switch max {
	case r:
		hue = 60.0 * math.Mod((g-b)/delta, 6.0)
	case g:
		hue = 60.0 * ((b-r)/delta + 2.0)
	default:
		hue = 60.0 * ((r-g)/delta + 4.0)
	}
	return math.Mod(hue+360.0, 360.0)
}

func TestStripAnsiLeavesTheBookmarkNames(t *testing.T) {
	var out []byte
	stripANSIInto([]byte("\x1b[38;5;5mHYS-delta\x1b[39m"), &out)
	if want := "HYS-delta"; string(out) != want {
		t.Errorf("stripped = %q, want %q", out, want)
	}
}

func equalStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range got {
		if got[i] != want[i] {
			return false
		}
	}
	return true
}
