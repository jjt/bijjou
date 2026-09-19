// Package render parses a jj log line into its graph prefix and its content,
// and re-emits the prefix with dim color, glyph swaps, and the optional
// column collapse.
package render

import (
	"bytes"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
)

// StripTrailingNL splits a trailing newline off the line. The second result
// is true when the line carried one.
func StripTrailingNL(line []byte) ([]byte, bool) {
	if n := len(line); n > 0 && line[n-1] == '\n' {
		return line[:n-1], true
	}
	return line, false
}

// EmitLine renders one input line into out. The graph prefix is rewritten
// edge-by-edge (dim color plus glyph swap). Every byte past the graph prefix
// is copied byte-for-byte. A line with no graph prefix passes through
// verbatim. A nil p means the line has no boundary.
func EmitLine(line []byte, p *Parsed, out *bytes.Buffer) {
	body, trailingNL := StripTrailingNL(line)
	collapse := config.Get().GraphCollapse
	switch {
	case p != nil:
		EmitDimGraph(body[:p.GraphEnd], collapse, nil, out)
		out.Write(body[p.GraphEnd:])
	// No boundary: this is a pure connector row (`├─╯`, `│`, `~`) or prose
	// that contains a box-drawing char. Only the connector row can collapse.
	// Collapse of prose shreds it by the loss of every second cell.
	case HasGraphChar(body):
		EmitDimGraph(body, collapse && IsGraphOnly(body), nil, out)
	default:
		out.Write(body)
	}
	if trailingNL {
		out.WriteByte('\n')
	}
}

const elisionCP uint32 = 0x7E // ~

// Parsed is the split of a line into graph prefix and content.
type Parsed struct {
	GraphEnd     int
	ContentStart int
	// GraphCol is the count of cells the graph prefix occupies as jj drew
	// it. GraphColCollapsed is the count that survives `graph.collapse`
	// (pad cells dropped). LastIsEdge and LastIsEdgeCollapsed are the kind
	// of the last glyph in each case. ClassifyRow picks one pair per the
	// config, so the graph-to-content gap matches what EmitDimGraph
	// actually emitted.
	GraphCol            int
	GraphColCollapsed   int
	LastIsEdge          bool
	LastIsEdgeCollapsed bool
}

// isEdgeChar reports whether cp is a graph edge. Graph edges are
// box-drawing glyphs plus the elision `~`. This set is fixed and stable in
// jj. Unlike nodes, bijjou can recognize an edge by its codepoint.
func isEdgeChar(cp uint32) bool {
	return (cp >= 0x2500 && cp <= 0x257F) || cp == elisionCP
}

// isHorizontalChar reports whether cp is one of the horizontals jj draws
// through a column gap.
//
// jj draws the graph in fixed two-cell columns. The glyph sits in the even
// cell. The gap between columns sits in the odd cell. That gap holds a
// space, or a horizontal when a connector runs through it. `graph.collapse`
// drops exactly those cells, so column N lands at cell N instead of cell 2N.
//
// Parity makes this safe. A bare "drop every horizontal and space" rule also
// deletes glyph cells. `├───╯` spans three columns, and two of its glyphs
// are horizontals. An inactive column is two spaces, and one of them is a
// glyph cell. The loss of either cell slides the rest of the row out of its
// column and off the verticals above and below it. The character check on
// top of parity is defensive. An odd cell that holds anything else (a
// corner, tee, or node) is kept. This costs one cell of width instead of the
// loss of a glyph.
func isHorizontalChar(cp uint32) bool {
	return cp == 0x2500 || cp == 0x2504 || cp == 0x2508 // ─ ┄ ┈
}

func isPadCell(cell int, cp uint32) bool {
	return cell%2 == 1 && (cp == ' ' || isHorizontalChar(cp))
}

// isNodeAt reports whether the glyph at pos is a graph node.
//
// A graph "node" is the commit marker jj draws at the rightmost graph column
// (`@ ○ ● ◆ ×`, or any glyph a custom `log_node` template emits — □, Nerd
// Font PUA, and more). bijjou does NOT enumerate node glyphs. A node is any
// non-edge, non-space glyph in the graph region. bijjou recognizes it
// structurally: a space or an edge follows it, after any CSI. jj pads every
// graph column, so a space (the column gap) always follows a node. On a
// merge tip an edge follows it instead. The gap always precedes real
// content, so the first glyph of content is never in this position. This
// keeps bijjou from a misread of plain text as a graph row. The node glyph
// is forwarded unchanged. jj's template owns the glyph and its color.
func isNodeAt(line []byte, pos int, cp uint32, size int) bool {
	if cp == ' ' || isEdgeChar(cp) {
		return false
	}
	j := pos + size
	for {
		after, ok := ansi.SkipCSI(line, j)
		if !ok {
			break
		}
		j = after
	}
	if j >= len(line) {
		return false
	}
	if line[j] == ' ' {
		return true
	}
	nextCP, _ := ansi.DecodeUTF8(line, j)
	return isEdgeChar(nextCP)
}

// mapGraphChar gives the configured replacement glyph for a jj edge
// codepoint. The second result is false for a codepoint with no mapping.
func mapGraphChar(cp uint32) (string, bool) {
	c := config.Get()
	switch cp {
	case '─', '┄', '┈':
		return c.GraphHorizontal, true
	case '│':
		return c.GraphVertical, true
	case '┌', '╭':
		return c.GraphTopLeft, true
	case '┐', '╮':
		return c.GraphTopRight, true
	case '└', '╰':
		return c.GraphBottomLeft, true
	case '┘', '╯':
		return c.GraphBottomRight, true
	case '├':
		return c.GraphTeeRight, true
	case '┤':
		return c.GraphTeeLeft, true
	case '┬':
		return c.GraphTeeDown, true
	case '┴':
		return c.GraphTeeUp, true
	case '┼':
		return c.GraphCross, true
	case '~':
		return c.GraphElision, true
	default:
		return "", false
	}
}

// FindBoundary locates the end of the graph prefix. The second result is
// false when the line carries no graph prefix.
func FindBoundary(line []byte) (Parsed, bool) {
	i := 0
	visCol := 0
	keptCol := 0
	hadGraph := false
	lastIsEdge := false
	lastIsEdgeCollapsed := false

	for i < len(line) {
		if after, ok := ansi.SkipCSI(line, i); ok {
			i = after
			continue
		}

		if line[i] == ' ' {
			sepStartByte := i
			sepStartCol := visCol
			sepStartKept := keptCol
			k := i
			spaceCount := 0
			lastSpaceEnd := i
			for {
				for {
					after, ok := ansi.SkipCSI(line, k)
					if !ok {
						break
					}
					k = after
				}
				if k < len(line) && line[k] == ' ' {
					spaceCount++
					k++
					lastSpaceEnd = k
				} else {
					break
				}
			}
			if k >= len(line) {
				return Parsed{}, false
			}
			cp, size := ansi.DecodeUTF8(line, k)
			if isEdgeChar(cp) || isNodeAt(line, k, cp, size) {
				i = k
				// Odd cells in the run are column pads and vanish under
				// collapse. Even cells are an inactive column's own cell.
				for cell := visCol; cell < visCol+spaceCount; cell++ {
					if !isPadCell(cell, ' ') {
						keptCol++
					}
				}
				visCol += spaceCount
			} else {
				if !hadGraph {
					return Parsed{}, false
				}
				return Parsed{
					GraphEnd:            sepStartByte,
					ContentStart:        lastSpaceEnd,
					GraphCol:            sepStartCol,
					GraphColCollapsed:   sepStartKept,
					LastIsEdge:          lastIsEdge,
					LastIsEdgeCollapsed: lastIsEdgeCollapsed,
				}, true
			}
		} else {
			cp, size := ansi.DecodeUTF8(line, i)
			edge := isEdgeChar(cp)
			if edge || isNodeAt(line, i, cp, size) {
				hadGraph = true
				lastIsEdge = edge
				i += size
				if !isPadCell(visCol, cp) {
					keptCol++
					lastIsEdgeCollapsed = edge
				}
				visCol++
			} else {
				return Parsed{}, false
			}
		}
	}
	return Parsed{}, false
}

// HasGraphChar reports whether the body holds a graph edge glyph.
func HasGraphChar(body []byte) bool {
	i := 0
	for i < len(body) {
		if after, ok := ansi.SkipCSI(body, i); ok {
			i = after
			continue
		}
		cp, size := ansi.DecodeUTF8(body, i)
		if isEdgeChar(cp) {
			return true
		}
		i += size
	}
	return false
}

// IsGraphOnly reports whether every visible cell is a graph edge or a space.
// These are the connector rows jj draws between commits (`├─╯`, `│`, `~`).
// Nodes never appear here. A row with a node carries content, so it has a
// boundary. Text with a stray box-drawing char fails this test. This keeps
// collapse off prose.
func IsGraphOnly(body []byte) bool {
	i := 0
	for i < len(body) {
		if after, ok := ansi.SkipCSI(body, i); ok {
			i = after
			continue
		}
		cp, size := ansi.DecodeUTF8(body, i)
		if cp != ' ' && !isEdgeChar(cp) {
			return false
		}
		i += size
	}
	return true
}

// jjVertical is jj's own vertical. EmitDimGraph maps it to
// `graph.edges.chars.vertical` like any other edge. A row synthesized from
// it dims and collapses exactly like the rows around it.
var jjVertical = []byte("│")

// GraphNodesToVerticals gives a graph prefix with its node turned back into
// a vertical. This is the connector row jj draws when a branch closes there.
// ANSI is dropped. The result goes straight back through EmitDimGraph, which
// colors the edges itself.
func GraphNodesToVerticals(prefix []byte) []byte {
	out := make([]byte, 0, len(prefix))
	i := 0
	for i < len(prefix) {
		if after, ok := ansi.SkipCSI(prefix, i); ok {
			i = after
			continue
		}
		cp, size := ansi.DecodeUTF8(prefix, i)
		if cp == ' ' || isEdgeChar(cp) {
			out = append(out, prefix[i:i+size]...)
		} else {
			out = append(out, jjVertical...)
		}
		i += size
	}
	return out
}

// NodeCell gives the cell that holds the row's graph node, counted the way
// FindBoundary counts cells: jj's own two per column, pad cells included,
// ANSI not counted. A commit row carries exactly one node, so the first
// non-edge glyph is that node. The second result is false for a prefix with
// no node at all: a connector row, or a row jj drew with no graph.
func NodeCell(prefix []byte) (int, bool) {
	i := 0
	cell := 0
	for i < len(prefix) {
		if after, ok := ansi.SkipCSI(prefix, i); ok {
			i = after
			continue
		}
		cp, size := ansi.DecodeUTF8(prefix, i)
		if cp != ' ' && !isEdgeChar(cp) {
			return cell, true
		}
		i += size
		cell++
	}
	return 0, false
}

// emitNode forwards node bytes unchanged. jj's template (or the upstream
// emitter) picks the right glyph and label color. bijjou forwards the bytes
// plus their surrounding ANSI verbatim. A hydra stack color, when in force,
// takes over the glyph's foreground instead.
func emitNode(raw, ansiSeq, color []byte, out *bytes.Buffer) {
	if color == nil {
		out.Write(ansiSeq)
		out.Write(raw)
		return
	}
	ansi.EmitFilteredANSI(ansiSeq, out, ansi.IsFGColorSGR)
	out.Write(color)
	out.Write(raw)
	out.Write(ansi.FGReset)
}

func emitEdge(cp uint32, raw, ansiSeq []byte, out *bytes.Buffer) {
	c := config.Get()
	ansi.EmitFilteredANSI(ansiSeq, out, ansi.IsFGColorSGR)
	out.Write(c.EdgeDimOn)
	if replacement, ok := mapGraphChar(cp); ok {
		out.WriteString(replacement)
	} else {
		out.Write(raw)
	}
	out.Write(ansi.FGReset)
}

// rightSide records what abuts an internal space run on its right when the
// run is flushed. sideNonGraph covers a newline, the end of the buffer, or
// the end of the graph prefix. The run does not terminate at a graph char,
// so it is never dashed.
type rightSide int

const (
	sideNode rightSide = iota
	sideEdge
	sideNonGraph
)

// graphRun tracks a run of spaces inside the graph prefix so it can be
// rewritten as dashes on flush. seenNode and leftWasNode describe the graph
// context to the left of the run. Both reset at each newline through
// resetLine.
type graphRun struct {
	seenNode    bool
	leftWasNode bool
	start       int // -1 when no run is pending
	spaces      int
}

func newGraphRun() graphRun {
	return graphRun{start: -1}
}

func (r *graphRun) resetLine() {
	r.seenNode = false
	r.leftWasNode = false
}

// flush replaces the pending internal space run with dashes when two
// conditions hold: the line already saw its node, and another graph char
// follows the run.
//
// Rules (per the dash spec):
//   - The cell immediately right of a node gets `dash_start` only when that
//     cell is also to the left of whitespace or a graph edge. That is, the
//     run is multi-cell, or it is a single cell between a node and an edge.
//     A single-cell run between two nodes emits NO dash at all (the space is
//     preserved).
//   - `dash_end` is never emitted here. Intra-graph runs always terminate at
//     another graph char, never content. The closing cap attaches the run to
//     the content boundary on the right. The DSL's content-side pad owns
//     that cap.
func (r *graphRun) flush(out *bytes.Buffer, right rightSide, c *config.Config) {
	start := r.start
	if start < 0 {
		return
	}
	r.start = -1
	count := r.spaces
	r.spaces = 0
	rightIsGraph := right == sideNode || right == sideEdge
	if !(r.seenNode && rightIsGraph && count > 0) {
		return
	}
	// Single-cell gap between two nodes: emit no dash and keep the space.
	if count == 1 && r.leftWasNode && right == sideNode {
		return
	}
	original := append([]byte(nil), out.Bytes()[start:]...)
	out.Truncate(start)
	out.Write(c.DimOn)
	// After the early return above, any run with leftWasNode either has
	// count > 1 (next cell is whitespace) or terminates at an edge — both
	// qualify for `dash_start`.
	headCap := r.leftWasNode && c.DashStart != ""
	for idx := 0; idx < count; idx++ {
		if headCap && idx == 0 {
			out.WriteString(c.DashStart)
		} else {
			out.WriteString(c.Dash)
		}
	}
	out.Write(ansi.FGReset)
	// CSI bytes never contain a literal space. The non-space bytes are kept,
	// so any color setup buffered between the spaces is preserved.
	for _, b := range original {
		if b != ' ' {
			out.WriteByte(b)
		}
	}
}

// EmitDimGraph emits bytes with every visible non-space char wrapped in dim
// SGR, except commit-node chars (○ ● ◆ @ ×). Those pass through with normal
// intensity. This strips jj's fg-color codes and preserves other ANSI
// sequences.
//
// After a node char appears on the line, bijjou fills any space run between
// two graph chars (node or edge) with the dash glyph, one dash per space. A
// run before the first node, or a run past the last graph char, stays as
// plain spaces.
//
// collapse drops jj's inter-column pad cells (see isPadCell). This pulls
// every graph column one cell left of the last. A dropped cell still
// forwards its ANSI so color state survives. Only the glyph goes. A nil
// nodeColor leaves the node as jj drew it.
func EmitDimGraph(src []byte, collapse bool, nodeColor []byte, out *bytes.Buffer) {
	c := config.Get()
	i := 0
	cell := 0
	run := newGraphRun()

	for i < len(src) {
		ansiStart := i
		for {
			after, ok := ansi.SkipCSI(src, i)
			if !ok {
				break
			}
			i = after
		}
		ansiSeq := src[ansiStart:i]

		if i >= len(src) {
			run.flush(out, sideNonGraph, c)
			ansi.EmitFilteredANSI(ansiSeq, out, ansi.IsFGColorSGR)
			break
		}

		if src[i] == '\n' {
			run.flush(out, sideNonGraph, c)
			ansi.EmitFilteredANSI(ansiSeq, out, ansi.IsFGColorSGR)
			out.WriteByte('\n')
			run.resetLine()
			cell = 0
			i++
			continue
		}

		if src[i] == ' ' {
			if collapse && isPadCell(cell, ' ') {
				ansi.EmitFilteredANSI(ansiSeq, out, ansi.IsFGColorSGR)
			} else {
				if run.start < 0 {
					run.start = out.Len()
				}
				ansi.EmitFilteredANSI(ansiSeq, out, ansi.IsFGColorSGR)
				out.WriteByte(' ')
				run.spaces++
			}
			cell++
			i++
			continue
		}

		cp, size := ansi.DecodeUTF8(src, i)
		raw := src[i : i+size]
		if collapse && isPadCell(cell, cp) {
			ansi.EmitFilteredANSI(ansiSeq, out, ansi.IsFGColorSGR)
			cell++
			i += size
			continue
		}
		// EmitDimGraph only ever receives a graph prefix, so every glyph
		// here is graph: anything that isn't an edge is the node.
		cpIsEdge := isEdgeChar(cp)
		right := sideNode
		if cpIsEdge {
			right = sideEdge
		}
		run.flush(out, right, c)
		if cpIsEdge {
			emitEdge(cp, raw, ansiSeq, out)
			run.leftWasNode = false
		} else {
			emitNode(raw, ansiSeq, nodeColor, out)
			run.seenNode = true
			run.leftWasNode = true
		}
		i += size
		cell++
	}

	run.flush(out, sideNonGraph, c)
}
