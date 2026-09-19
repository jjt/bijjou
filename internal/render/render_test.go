package render

import (
	"bytes"
	"testing"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
)

func TestIsEdgeCharIncludesBoxDrawingAndElision(t *testing.T) {
	for _, cp := range []uint32{0x2500, 0x2502, 0x256D, 0x257F, elisionCP} {
		if !isEdgeChar(cp) {
			t.Errorf("cp=%#x should be edge", cp)
		}
	}
}

func TestIsEdgeCharRejectsNodesLettersSpace(t *testing.T) {
	// Node glyphs (built-in and custom) are NOT edges.
	for _, cp := range []uint32{0x40, 0x25CB, 0x25CF, 0x25C6, 0xD7, 0x25A1, 0xF28D} {
		if isEdgeChar(cp) {
			t.Errorf("cp=%#x should not be edge", cp)
		}
	}
	// Letters and space are not edges.
	for _, cp := range []uint32{0x41, 0x61, 0x20} {
		if isEdgeChar(cp) {
			t.Errorf("cp=%#x should not be edge", cp)
		}
	}
}

func TestNodeCellCountsJJsOwnCells(t *testing.T) {
	some := func(prefix string, want int) {
		t.Helper()
		got, ok := NodeCell([]byte(prefix))
		if !ok {
			t.Errorf("NodeCell(%q) = none, want %d", prefix, want)
			return
		}
		if got != want {
			t.Errorf("NodeCell(%q) = %d, want %d", prefix, got, want)
		}
	}
	none := func(prefix string) {
		t.Helper()
		if got, ok := NodeCell([]byte(prefix)); ok {
			t.Errorf("NodeCell(%q) = %d, want none", prefix, got)
		}
	}
	// A stack's node sits in the leftmost column. A neighbor's branch runs
	// past it in the next column.
	some("● │ ", 0)
	some("│ ● ", 2)
	// ANSI is not a cell, and a merge tip's node abuts its edges.
	some("\x1b[38;5;8m│\x1b[39m ● ", 2)
	some("│ ●─╮", 2)
	// Custom `log_node` glyphs are nodes like any other.
	some("│ □ ", 2)
	// A connector row has no node at all.
	none("├─╯")
	none("")
}

func TestMapGraphCharBoxDrawings(t *testing.T) {
	for _, cp := range []uint32{0x2500, 0x2502, 0x256D, 0x2570, 0x251C, 0x253C, elisionCP} {
		if _, ok := mapGraphChar(cp); !ok {
			t.Errorf("mapGraphChar(%#x) = none, want some", cp)
		}
	}
}

func TestMapGraphCharUnknownReturnsNone(t *testing.T) {
	if _, ok := mapGraphChar(0x41); ok {
		t.Errorf("mapGraphChar(0x41) = some, want none")
	}
}

func boundary(t *testing.T, line []byte, msg string) Parsed {
	t.Helper()
	p, ok := FindBoundary(line)
	if !ok {
		t.Fatalf("%s", msg)
	}
	return p
}

func TestBoundarySingleNodeThenContent(t *testing.T) {
	line := []byte("\xe2\x97\x8b  abc")
	p := boundary(t, line, "expected boundary")
	if p.GraphCol != 1 {
		t.Errorf("graph_col = %d, want 1", p.GraphCol)
	}
	if p.GraphEnd != 3 {
		t.Errorf("graph_end = %d, want 3", p.GraphEnd)
	}
	if p.ContentStart != 5 {
		t.Errorf("content_start = %d, want 5", p.ContentStart)
	}
}

func TestBoundaryReturnsNoneWhenNoGraph(t *testing.T) {
	if _, ok := FindBoundary([]byte("plain text")); ok {
		t.Errorf("expected no boundary")
	}
}

func TestBoundaryCustomNodeWhiteSquare(t *testing.T) {
	// U+25A1 □ is not a built-in jj node, but jj emits it as a custom node
	// glyph: `□  content`. Structural detection must recognize it.
	line := []byte("□  abc")
	p := boundary(t, line, "expected boundary for custom node")
	if p.GraphCol != 1 {
		t.Errorf("graph_col = %d, want 1", p.GraphCol)
	}
	if p.GraphEnd != len("□") {
		t.Errorf("graph_end = %d, want %d", p.GraphEnd, len("□"))
	}
	if p.ContentStart != len("□  ") {
		t.Errorf("content_start = %d, want %d", p.ContentStart, len("□  "))
	}
	if p.LastIsEdge {
		t.Errorf("node, not edge")
	}
}

func TestBoundaryCustomNodePUA(t *testing.T) {
	// Nerd Font Private Use Area glyph (U+F28D) as a node.
	line := []byte("\uf28d  abc")
	p := boundary(t, line, "expected boundary for PUA node")
	if p.GraphCol != 1 {
		t.Errorf("graph_col = %d, want 1", p.GraphCol)
	}
	if p.LastIsEdge {
		t.Errorf("last_is_edge = true, want false")
	}
}

func TestBoundaryNodeFollowedByEdge(t *testing.T) {
	// Merge tip `●─` then content: an edge follows the node, not a space.
	// bijjou must still recognize it as a node.
	line := []byte("●─ abc")
	p := boundary(t, line, "expected boundary")
	if p.GraphCol != 2 {
		t.Errorf("graph_col = %d, want 2", p.GraphCol)
	}
	if !p.LastIsEdge {
		t.Errorf("last graph glyph is the ─ edge")
	}
}

func TestBoundaryGlyphAbuttingLetterIsContentNotNode(t *testing.T) {
	// A non-edge glyph with a letter directly after it (no gap) is content,
	// not a node. This guards the payload against a misread. No graph →
	// none.
	if _, ok := FindBoundary([]byte("□bc")); ok {
		t.Errorf("expected no boundary")
	}
}

func TestBoundaryCustomNodeAfterEdgeColumn(t *testing.T) {
	// `│ □  content`: edge column, then custom node, then gap.
	line := []byte("│ □  abc")
	p := boundary(t, line, "expected boundary")
	if p.GraphCol != 3 {
		t.Errorf("graph_col = %d, want 3", p.GraphCol)
	}
	if p.LastIsEdge {
		t.Errorf("last_is_edge = true, want false")
	}
}

func TestBoundarySkipsCSIAroundGraph(t *testing.T) {
	line := []byte("\x1b[31m\xe2\x97\x8b\x1b[39m  abc")
	p := boundary(t, line, "expected boundary")
	if p.GraphCol != 1 {
		t.Errorf("graph_col = %d, want 1", p.GraphCol)
	}
}

func TestBoundaryMultiGraphColumns(t *testing.T) {
	line := []byte("\xe2\x94\x82 \xe2\x94\x82 \xe2\x97\x8b  abc")
	p := boundary(t, line, "expected boundary")
	if p.GraphCol != 5 {
		t.Errorf("graph_col = %d, want 5", p.GraphCol)
	}
}

func TestBoundaryRequiresAtLeastOneGraphChar(t *testing.T) {
	if _, ok := FindBoundary([]byte("   abc")); ok {
		t.Errorf("expected no boundary")
	}
}

func runEmit(graph []byte) []byte {
	var out bytes.Buffer
	EmitDimGraph(graph, false, nil, &out)
	return out.Bytes()
}

func runEmitCollapsed(graph []byte) []byte {
	var out bytes.Buffer
	EmitDimGraph(graph, true, nil, &out)
	return out.Bytes()
}

func dim(glyph string) []byte {
	v := append([]byte(nil), config.DefaultEdgeDimOn...)
	v = append(v, glyph...)
	v = append(v, ansi.FGReset...)
	return v
}

func TestCollapseDropsColumnPadsKeepsGlyphs(t *testing.T) {
	// `├─╯` is two columns: `├` + pad `─`, then `╯`.
	expected := dim(config.DefaultGraphTeeRight)
	expected = append(expected, dim(config.DefaultGraphBottomRight)...)
	if got := runEmitCollapsed([]byte("├─╯")); !bytes.Equal(got, expected) {
		t.Errorf("got %q, want %q", got, expected)
	}
}

func TestCollapseKeepsHorizontalsSittingInGlyphCells(t *testing.T) {
	// `├───╯` spans three columns. The middle column's own glyph is a
	// horizontal (cell 2). It must survive, or `╯` slides off its column.
	expected := dim(config.DefaultGraphTeeRight)
	expected = append(expected, dim(config.DefaultGraphHorizontal)...)
	expected = append(expected, dim(config.DefaultGraphBottomRight)...)
	if got := runEmitCollapsed([]byte("├───╯")); !bytes.Equal(got, expected) {
		t.Errorf("got %q, want %q", got, expected)
	}
}

func TestCollapseKeepsOneCellPerInactiveColumn(t *testing.T) {
	// `│   ○`: vertical, an inactive column (two spaces), then the node. One
	// space survives so the node stays in column 2.
	expected := dim(config.DefaultGraphVertical)
	expected = append(expected, ' ')
	expected = append(expected, "○"...)
	if got := runEmitCollapsed([]byte("│   ○")); !bytes.Equal(got, expected) {
		t.Errorf("got %q, want %q", got, expected)
	}
}

func TestCollapseKeepsUnexpectedGlyphInAPadCell(t *testing.T) {
	// Defensive: a non-horizontal glyph in an odd cell is not a pad. It is
	// kept (one cell wider) rather than deleted.
	expected := dim(config.DefaultGraphVertical)
	expected = append(expected, dim(config.DefaultGraphVertical)...)
	if got := runEmitCollapsed([]byte("││")); !bytes.Equal(got, expected) {
		t.Errorf("got %q, want %q", got, expected)
	}
}

func TestCollapseForwardsANSIOfDroppedCells(t *testing.T) {
	// The pad's own SGR must survive even though its glyph does not. A
	// dropped reset leaks color into the rest of the line.
	got := runEmitCollapsed([]byte("│\x1b[1m \x1b[22m○"))
	expected := dim(config.DefaultGraphVertical)
	expected = append(expected, "\x1b[1m\x1b[22m"...)
	expected = append(expected, "○"...)
	if !bytes.Equal(got, expected) {
		t.Errorf("got %q, want %q", got, expected)
	}
}

func TestCollapsedGraphColCountsKeptCells(t *testing.T) {
	// `│ │ ○  abc`: 5 cells drawn, 3 after collapse.
	line := []byte("│ │ ○  abc")
	p := boundary(t, line, "expected boundary")
	if p.GraphCol != 5 {
		t.Errorf("graph_col = %d, want 5", p.GraphCol)
	}
	if p.GraphColCollapsed != 3 {
		t.Errorf("graph_col_collapsed = %d, want 3", p.GraphColCollapsed)
	}
	if p.LastIsEdge {
		t.Errorf("last_is_edge = true, want false")
	}
	if p.LastIsEdgeCollapsed {
		t.Errorf("last_is_edge_collapsed = true, want false")
	}
}

func TestCollapsedLastGlyphKindIgnoresDroppedPad(t *testing.T) {
	// `○─ abc`: the trailing `─` is a pad cell, so the collapsed prefix ends
	// on the node and its gap gets a node-side dash cap.
	line := []byte("○─ abc")
	p := boundary(t, line, "expected boundary")
	if !p.LastIsEdge {
		t.Errorf("last_is_edge = false, want true")
	}
	if p.LastIsEdgeCollapsed {
		t.Errorf("last_is_edge_collapsed = true, want false")
	}
	if p.GraphColCollapsed != 1 {
		t.Errorf("graph_col_collapsed = %d, want 1", p.GraphColCollapsed)
	}
}

func TestIsGraphOnlySeparatesConnectorRowsFromProse(t *testing.T) {
	for _, s := range []string{"├─╯", "│ │", "~"} {
		if !IsGraphOnly([]byte(s)) {
			t.Errorf("IsGraphOnly(%q) = false, want true", s)
		}
	}
	// A description that contains a box-drawing char.
	if IsGraphOnly([]byte("fix: draw ─ separators")) {
		t.Errorf("IsGraphOnly(prose) = true, want false")
	}
}

func TestDimNodeCharsPassThroughVerbatim(t *testing.T) {
	for _, s := range [][]byte{[]byte("\xe2\x97\x8b"), []byte("\xe2\x97\x86"), []byte("@")} {
		if got := runEmit(s); !bytes.Equal(got, s) {
			t.Errorf("got %q, want %q", got, s)
		}
	}
	withANSI := []byte("\x1b[1m\x1b[38;5;2m@\x1b[0m")
	if got := runEmit(withANSI); !bytes.Equal(got, withANSI) {
		t.Errorf("got %q, want %q", got, withANSI)
	}
}

func TestDimCustomNodePassthrough(t *testing.T) {
	// A graph prefix is all bijjou ever feeds EmitDimGraph, so any non-edge
	// glyph in it is a node and must pass through verbatim (jj owns the
	// glyph + its color), not get the edge-dim wrapper.
	for _, s := range []string{"□", "\uf28d", "■"} {
		if got := runEmit([]byte(s)); !bytes.Equal(got, []byte(s)) {
			t.Errorf("got %q, want %q", got, s)
		}
	}
}

func TestDimBoxDrawingGetsEdgeDim(t *testing.T) {
	got := runEmit([]byte("\xe2\x94\x82"))
	expected := append([]byte(nil), config.DefaultEdgeDimOn...)
	expected = append(expected, config.DefaultGraphVertical...)
	expected = append(expected, ansi.FGReset...)
	if !bytes.Equal(got, expected) {
		t.Errorf("got %q, want %q", got, expected)
	}
}

func TestDimSpacesPassthrough(t *testing.T) {
	if got := runEmit([]byte("   ")); !bytes.Equal(got, []byte("   ")) {
		t.Errorf("got %q, want %q", got, "   ")
	}
}
