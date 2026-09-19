// Package dsl holds the templating DSL and the NUL/RS-framed record parser.
package dsl

import (
	"bytes"
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
)

// NodeKind names the three template AST node shapes.
type NodeKind int

const (
	// NodeLiteral is verbatim template text. It carries Literal.
	NodeLiteral NodeKind = iota
	// NodeField is a `%{field}` lookup. It carries Name.
	NodeField
	// NodeElasticTab is a `%{elastic_tab(field)}` align point. It carries
	// Name, which is empty for an arg-less tab.
	NodeElasticTab
)

// Node is one template AST node.
type Node struct {
	Kind    NodeKind
	Literal []byte
	Name    string
}

// Template is the template AST. You author it as `%{field}` or
// `%{func(field)}` tokens with arbitrary literal text between them. The
// render path walks Nodes left to right per commit. Elastic-tab nodes pad to
// a column shared across commits, so the field's left edge lines up
// vertically.
type Template struct {
	Nodes []Node
}

// prepare runs the parse pre-pass: it collapses real newlines to spaces and
// treats the two-character sequence `\n` as a real newline. Other backslash
// sequences pass through.
//
// Every other byte widens latin-1 style, one byte to one codepoint, the way
// the Rust `prepped.push(b as char)` does. So a byte at or above 0x80 leaves
// as its two-byte UTF-8 encoding, and the scan below works on the re-encoded
// bytes. A non-ASCII literal in a template body therefore renders as its
// latin-1 mojibake. The Go port keeps that, because the two must agree
// byte for byte.
func prepare(src string) []byte {
	out := make([]byte, 0, len(src))
	for i := 0; i < len(src); {
		b := src[i]
		if b == '\\' && i+1 < len(src) && src[i+1] == 'n' {
			out = append(out, '\n')
			i += 2
			continue
		}
		if b == '\n' {
			out = append(out, ' ')
			i++
			continue
		}
		if b < 0x80 {
			out = append(out, b)
		} else {
			out = append(out, 0xC0|b>>6, 0x80|b&0x3F)
		}
		i++
	}
	return out
}

// Parse compiles a template body into its AST.
func Parse(src string) (*Template, error) {
	pb := prepare(src)

	var nodes []Node
	start := 0
	i := 0
	for i < len(pb) {
		if pb[i] == '%' && i+1 < len(pb) && pb[i+1] == '{' {
			if i > start {
				nodes = append(nodes, Node{Kind: NodeLiteral, Literal: pb[start:i]})
			}
			rel := bytes.IndexByte(pb[i+2:], '}')
			if rel < 0 {
				return nil, fmt.Errorf("template: unterminated %%{ at byte %d", i)
			}
			raw := pb[i+2 : i+2+rel]
			if !utf8.Valid(raw) {
				return nil, errors.New("template: invalid utf-8 inside %{...}")
			}
			node, err := parseExpr(strings.TrimSpace(string(raw)))
			if err != nil {
				return nil, err
			}
			nodes = append(nodes, node)
			i += 2 + rel + 1
			start = i
			continue
		}
		i++
	}
	if len(pb) > start {
		nodes = append(nodes, Node{Kind: NodeLiteral, Literal: pb[start:]})
	}
	return &Template{Nodes: nodes}, nil
}

// parseExpr turns the text inside one `%{...}` token into a node.
// `elastic_tab` is the only known function.
func parseExpr(s string) (Node, error) {
	open := strings.IndexByte(s, '(')
	if open < 0 {
		return Node{Kind: NodeField, Name: s}, nil
	}
	if !strings.HasSuffix(s, ")") {
		return Node{}, fmt.Errorf("template: missing `)` in `%s`", s)
	}
	name := strings.TrimSpace(s[:open])
	arg := strings.TrimSpace(s[open+1 : len(s)-1])
	if name != "elastic_tab" {
		return Node{}, fmt.Errorf("template: unknown function `%s`", name)
	}
	return Node{Kind: NodeElasticTab, Name: arg}, nil
}

// ParseNULOneline is the flat NUL/RS-framed parser. Record shape:
//
//	key1\0val1\0key2\0val2\0...\0keyN\0valN\x1e
//
// A trailing `\x1e` is required as the record terminator. Values pass through
// verbatim (ANSI ESC bytes survive). No escaping is needed because neither
// `\0` nor `\x1e` occur in jj's normal output. ok is false when the bytes do
// not hold a record.
func ParseNULOneline(b []byte) (map[string][]byte, bool) {
	rs := bytes.IndexByte(b, 0x1E)
	if rs < 0 {
		return nil, false
	}
	if rs == 0 {
		return nil, false
	}
	// One copy of the record body backs every value, so the fields outlive
	// the caller's line buffer.
	body := append([]byte(nil), b[:rs]...)
	parts := bytes.Split(body, []byte{0})
	if len(parts) < 2 || len(parts)%2 != 0 {
		return nil, false
	}
	fields := make(map[string][]byte, len(parts)/2)
	for i := 0; i < len(parts); i += 2 {
		if !utf8.Valid(parts[i]) {
			return nil, false
		}
		key := string(parts[i])
		if key == "" {
			return nil, false
		}
		fields[key] = parts[i+1]
	}
	return fields, true
}

// VisibleWidth counts visible cells in a byte slice. CSI escapes are skipped.
// Each remaining codepoint counts as one cell.
func VisibleWidth(b []byte) int {
	i := 0
	w := 0
	for i < len(b) {
		if after, ok := ansi.SkipCSI(b, i); ok {
			i = after
			continue
		}
		_, size := ansi.DecodeUTF8(b, i)
		w++
		i += size
	}
	return w
}

// Override substitutes one field's bytes for one row. A nil *Override means
// no substitution.
type Override struct {
	Key   string
	Value []byte
}

// CollectAnchors is pass 1: it records, per elastic-tab position, the max
// natural column across rows. This is the row-relative column the tab lands at
// when nothing pads. Tabs are keyed by their left-to-right order in the
// template (0-indexed), NOT by any arg string, so distinct tabs never collide.
// Pass 2 left-pads each row up to its tab's recorded column, so the left edge
// of the next content lines up. An arg-ful tab advances the column by the
// width of its field (it emits that field). An arg-less tab advances by zero
// (the next `%{field}` node accounts for the width instead).
//
// over substitutes one field's bytes for this row, the same way RenderRow
// takes them. The anchors must be measured on what pass 2 emits. hydra's
// `hydra.prefixes-replace` stand-ins are not the width of the names they
// replace.
func CollectAnchors(t *Template, fields map[string][]byte, over *Override, anchors *[]int) {
	col := 0
	tabI := 0
	for i := range t.Nodes {
		node := &t.Nodes[i]
		switch node.Kind {
		case NodeLiteral:
			col += VisibleWidth(node.Literal)
		case NodeField:
			col += VisibleWidth(fieldValue(fields, over, node.Name))
		case NodeElasticTab:
			for tabI >= len(*anchors) {
				*anchors = append(*anchors, 0)
			}
			if col > (*anchors)[tabI] {
				(*anchors)[tabI] = col
			}
			col += VisibleWidth(fieldValue(fields, over, node.Name))
			tabI++
		}
	}
}

// segKind names the render segment shapes.
type segKind int

const (
	// segContent is opaque bytes (a tag value, or non-space literal text
	// from the template) that must pass through unchanged.
	segContent segKind = iota
	// segWs is touchable whitespace (literal spaces from the template or
	// the leading graph-to-content gap) that the rules in RenderRow can
	// strip or fill with dashes.
	segWs
	// segAnchor holds left-pad cells emitted ahead of an elastic tab to
	// align its left edge across rows. It combines with adjacent segWs for
	// dash-fill, but rule 2 does not remove it when the elastic tab's value
	// is empty. The column-alignment commitment survives empty cells.
	segAnchor
	// segEmptyTag marks a zero-width `%{}` block.
	segEmptyTag
)

// seg is one chunk of output.
type seg struct {
	kind  segKind
	bytes []byte // segContent payload
	n     int    // segWs / segAnchor cell count
}

// LeftSide says what sits immediately to the left of a Ws run. This drives
// whether the left end of the run emits a `╶` cap (next to a node or interior
// content), a plain `─` (next to a graph edge, because caps never face edges),
// or a space (the run is only one cell wide with no graph context).
type LeftSide int

const (
	LeftGraphNode LeftSide = iota
	LeftGraphEdge
	LeftContent
)

// RenderRow renders one row with the four-rule model documented in
// `bijjou-config.toml`:
//
//  1. Leading whitespace before the first non-whitespace character is
//     preserved verbatim.
//  2. When a `%{}` block emits empty bytes, every whitespace cell between that
//     block and the nearest non-whitespace character to its left collapses to
//     zero.
//  3. After steps 1-2 and the elastic-tab column alignment, any run of
//     consecutive whitespace cells is filled with dashes (a single cell stays
//     a space, a run of two or more becomes a capped dash run).
//  4. Bytes that came out of a `%{}` block (a field or elastic-tab value) are
//     never modified. Internal whitespace inside a value passes through
//     untouched.
//
// leadingPad is prepended as a Ws segment, so the graph-to-content gap that
// the emit path adds takes part in steps 2-3 alongside the template's own
// whitespace.
//
// over substitutes one field's bytes for this row (hydra's rewritten
// `bookmarks`). Its visible width can differ from the field jj printed, so
// pass 1 must get the same substitution. CollectAnchors takes it.
func RenderRow(t *Template, fields map[string][]byte, over *Override, leadingPad int, leadingLeft LeftSide, anchors []int, out *bytes.Buffer) {
	var segs []seg
	if leadingPad > 0 {
		segs = append(segs, seg{kind: segWs, n: leadingPad})
	}
	// Track the per-row "natural" visible column (relative to the start of
	// the template; leadingPad is uniform across rows, so it stays out of
	// this counter). It drives the elastic-tab left-pad: when the row's
	// current natural column is behind the tab's recorded anchor, emit the
	// difference so the following content's left edge lands consistently.
	col := 0
	tabI := 0
	for i := range t.Nodes {
		node := &t.Nodes[i]
		switch node.Kind {
		case NodeLiteral:
			col += VisibleWidth(node.Literal)
			segs = pushLiteralSegs(node.Literal, segs)
		case NodeField:
			value := fieldValue(fields, over, node.Name)
			col += VisibleWidth(value)
			if len(value) == 0 {
				segs = append(segs, seg{kind: segEmptyTag})
			} else {
				segs = append(segs, seg{kind: segContent, bytes: value})
			}
		case NodeElasticTab:
			anchorTarget := col
			if tabI < len(anchors) {
				anchorTarget = anchors[tabI]
			}
			leftPad := 0
			if anchorTarget > col {
				leftPad = anchorTarget - col
			}
			if leftPad > 0 {
				segs = append(segs, seg{kind: segAnchor, n: leftPad})
				col += leftPad
			}
			// An arg-ful tab emits its field inline. An arg-less tab emits
			// nothing (the next `%{field}` node emits the value).
			if node.Name != "" {
				value := fieldValue(fields, over, node.Name)
				vw := VisibleWidth(value)
				if len(value) == 0 {
					segs = append(segs, seg{kind: segEmptyTag})
				} else {
					segs = append(segs, seg{kind: segContent, bytes: value})
				}
				col += vw
			}
			tabI++
		}
	}
	segs = applyRule2(segs)
	emitSegs(segs, leadingLeft, out)
}

// fieldValue reads one field, honouring the per-row override.
func fieldValue(fields map[string][]byte, over *Override, name string) []byte {
	if over != nil && over.Key == name {
		return over.Value
	}
	return fields[name]
}

// pushLiteralSegs splits a literal node into alternating Ws / Content segments
// based on runs of ASCII space. Multi-byte UTF-8 sequences are not space
// chars, so they go into Content runs.
func pushLiteralSegs(b []byte, segs []seg) []seg {
	i := 0
	for i < len(b) {
		if b[i] == ' ' {
			start := i
			for i < len(b) && b[i] == ' ' {
				i++
			}
			segs = append(segs, seg{kind: segWs, n: i - start})
		} else {
			start := i
			for i < len(b) && b[i] != ' ' {
				i++
			}
			segs = append(segs, seg{kind: segContent, bytes: b[start:i]})
		}
	}
	return segs
}

// applyRule2 implements rule 2: for each segEmptyTag, walk left and drop every
// preceding segWs up to the first segContent. segEmptyTag segments are
// transparent for the walk (they represent zero-width tags). When no content
// lies to the left, rule 1 wins and nothing is stripped.
func applyRule2(segs []seg) []seg {
	i := 0
	for i < len(segs) {
		if segs[i].kind != segEmptyTag {
			i++
			continue
		}
		hasAnchorLeft := false
		for _, s := range segs[:i] {
			if s.kind == segContent {
				hasAnchorLeft = true
				break
			}
		}
		if hasAnchorLeft {
			// Pop trailing Ws and EmptyTag entries leftward from i, then
			// drop the EmptyTag itself. Anchor segments stop the walk. They
			// encode column-alignment that an empty value must not erase.
			j := i
			for j > 0 {
				if segs[j-1].kind == segWs {
					segs = removeSeg(segs, j-1)
					j--
					i--
					continue
				}
				if segs[j-1].kind == segEmptyTag {
					j--
					continue
				}
				break
			}
		}
		// Drop the EmptyTag marker. It carried no bytes anyway.
		segs = removeSeg(segs, i)
	}
	return segs
}

func removeSeg(segs []seg, i int) []seg {
	copy(segs[i:], segs[i+1:])
	return segs[:len(segs)-1]
}

// emitSegs walks the segments and combines adjacent Ws into single dash-fill
// calls, so rule 3 (consecutive whitespace becomes dashes) applies uniformly
// across literal, pad, and graph-gap cells.
//
// leadingLeft describes the prefix to the left of the first Ws, before any
// content is emitted. After content appears, every later Ws sees content on
// its left.
//
// A row whose graph prefix ends in an edge (leadingLeft == LeftGraphEdge)
// still dash-fills. emitPad drops the left cap, so the run abuts the edge
// glyph with a plain dash instead of a `╶`. The dashes (and the closing `╴`
// against content) are emitted as on any other row.
func emitSegs(segs []seg, leadingLeft LeftSide, out *bytes.Buffer) {
	i := 0
	contentEmitted := false
	for i < len(segs) {
		switch segs[i].kind {
		case segContent:
			out.Write(segs[i].bytes)
			contentEmitted = true
			i++
		case segWs, segAnchor:
			total := 0
			for i < len(segs) && (segs[i].kind == segWs || segs[i].kind == segAnchor) {
				total += segs[i].n
				i++
			}
			left := leadingLeft
			if contentEmitted {
				left = LeftContent
			}
			emitPad(total, left, out)
		default:
			// Rule 2 removes the EmptyTag segments. Treat any survivor as
			// zero-width and skip.
			i++
		}
	}
}

// EmitNodePad emits a fixed-width pad run directly to the right of a graph
// node (the gap between a root commit's graph prefix and its value).
func EmitNodePad(cells int, out *bytes.Buffer) {
	emitPad(cells, LeftGraphNode, out)
}

// emitPad decides which glyphs fill the run, based on what sits to the left:
//
//   - LeftGraphNode / LeftContent: the run opens with `dash-start` (the cell
//     to the right of the node, or of the preceding content).
//   - LeftGraphEdge: the opening cap is suppressed. Dashes never overwrite or
//     abut a graph edge glyph directly.
//
// The closing cell is the cell next to the content the run ends at. It is
// never a dash. It holds `layout.dash-end`, or a plain space when that is
// unset (the default), so content always has a space to its left. A one-cell
// run is only a closing cell.
func emitPad(cells int, left LeftSide, out *bytes.Buffer) {
	if cells == 0 {
		return
	}
	c := config.Get()
	if cells == 1 {
		if c.DashEnd == "" {
			out.WriteByte(' ')
		} else {
			out.Write(c.DimOn)
			out.WriteString(c.DashEnd)
			out.Write(ansi.FGReset)
		}
		return
	}
	closing := c.DashEnd
	if closing == "" {
		closing = " "
	}
	openingCap := left != LeftGraphEdge && c.DashStart != ""
	out.Write(c.DimOn)
	for idx := 0; idx < cells; idx++ {
		switch {
		case openingCap && idx == 0:
			out.WriteString(c.DashStart)
		case idx+1 == cells:
			out.WriteString(closing)
		default:
			out.WriteString(c.Dash)
		}
	}
	out.Write(ansi.FGReset)
}
