// Package pipeline holds the row core shared by the buffered and streaming
// paths: it classifies one jj log line, folds the elastic-tab metrics, and
// renders the line back out.
package pipeline

import (
	"bytes"
	"fmt"
	"io"
	"os"
	"unicode/utf8"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
	"tangled.org/jjt.io/bijjou/internal/dsl"
	"tangled.org/jjt.io/bijjou/internal/hydra"
	"tangled.org/jjt.io/bijjou/internal/output"
	"tangled.org/jjt.io/bijjou/internal/render"
)

// RowKind names the three shapes a classified line can take.
type RowKind int

const (
	// RowPassthrough carries the graph boundary that ClassifyRow already
	// located. Parsed is nil when the line has no graph prefix. As a result,
	// EmitClassified does not need to re-run render.FindBoundary.
	RowPassthrough RowKind = iota
	RowCommit
	RowRoot
)

// Row is one classified input line.
type Row struct {
	Kind            RowKind
	GraphEnd        int
	GraphCol        int
	LastIsEdge      bool
	TemplateName    string
	HasTemplateName bool
	Fields          map[string][]byte
	// Value is the RowRoot payload.
	Value []byte
	// Parsed is the RowPassthrough boundary, nil when the line has none.
	Parsed *render.Parsed
}

// CompiledTemplate is a templates.<name> entry compiled at startup. Empty
// carries no template body. bijjou drops the row content and emits only the
// graph prefix.
type CompiledTemplate struct {
	Empty    bool
	Template *dsl.Template
}

// Metrics is the per-template alignment state. Anchors[i] is the row-wide
// maximum natural column before the i-th elastic_tab in the template. The
// left-to-right order keys the tabs. The anchors grow monotonically as bijjou
// scans the rows.
type Metrics struct {
	Anchors []int
}

// emptyMetrics is a shared, allocation-free empty metrics table for the
// no-recorded-anchors path. This path handles a template whose only row is the
// one bijjou renders now, or the synthetic missing-template notice.
var emptyMetrics = &Metrics{}

// CompileTemplates parses every configured template body once.
func CompileTemplates(m map[string]string) (map[string]*CompiledTemplate, error) {
	out := make(map[string]*CompiledTemplate, len(m))
	for name, body := range m {
		entry := &CompiledTemplate{}
		if body == "" {
			entry.Empty = true
		} else {
			tpl, err := dsl.Parse(body)
			if err != nil {
				return nil, fmt.Errorf("templates.%s: %w", name, err)
			}
			entry.Template = tpl
		}
		out[name] = entry
	}
	return out, nil
}

// ClassifyRow decides what one line is: a commit record, the root record, or
// a line bijjou passes through.
func ClassifyRow(body []byte) Row {
	p, ok := render.FindBoundary(body)
	if !ok {
		return Row{Kind: RowPassthrough}
	}
	payload := body[p.ContentStart:]
	i := 0
	for i < len(payload) {
		if after, ok := ansi.SkipCSI(payload, i); ok {
			i = after
			continue
		}
		if payload[i] == ' ' || payload[i] == '\t' {
			i++
			continue
		}
		break
	}
	rest := payload[i:]
	// NUL/RS-framed record: a \x1e terminator is the format marker.
	if bytes.IndexByte(rest, 0x1E) < 0 {
		return Row{Kind: RowPassthrough, Parsed: &p}
	}
	fields, ok := dsl.ParseNULOneline(rest)
	if !ok {
		return Row{Kind: RowPassthrough, Parsed: &p}
	}
	if len(fields) == 1 {
		if value, isRoot := fields["root"]; isRoot {
			return Row{Kind: RowRoot, GraphEnd: p.GraphEnd, Value: value}
		}
	}
	var name string
	hasName := false
	if raw, present := fields[config.BijjouTemplateNameField]; present {
		delete(fields, config.BijjouTemplateNameField)
		if utf8.Valid(raw) {
			name = string(raw)
			hasName = true
		}
	}
	// Under graph.collapse the prefix render.EmitDimGraph writes is narrower
	// than the one jj drew, so the row's column count (and the kind of its
	// last glyph) must be the collapsed pair or the graph-to-content gap
	// over/under-shoots.
	graphCol, lastIsEdge := p.GraphCol, p.LastIsEdge
	if config.Get().GraphCollapse {
		graphCol, lastIsEdge = p.GraphColCollapsed, p.LastIsEdgeCollapsed
	}
	return Row{
		Kind:            RowCommit,
		GraphEnd:        p.GraphEnd,
		GraphCol:        graphCol,
		LastIsEdge:      lastIsEdge,
		TemplateName:    name,
		HasTemplateName: hasName,
		Fields:          fields,
	}
}

// AccumulateMetrics runs the pass-1 accumulation the buffered and streaming
// paths share. It folds every commit row's elastic-tab anchors into metrics
// and widens maxGraphCol. Anchors only grow because dsl.CollectAnchors takes
// maxima. As a result, a call across successive streaming batches widens
// monotonically. It never invalidates the rows already emitted above.
//
// hydra.prefixes-replace renders a bookmark leader with its own width. As a
// result, bijjou measures the anchors on the replaced field. Pass 2 emits the
// same substitution.
func AccumulateMetrics(rows []Row, templates map[string]*CompiledTemplate, metrics map[string]*Metrics, maxGraphCol *int) {
	renamer := hydra.NewRenamer()
	var ov dsl.Override
	for i := range rows {
		row := &rows[i]
		if row.Kind != RowCommit {
			continue
		}
		if row.HasTemplateName {
			if ct, found := templates[row.TemplateName]; found && ct.Template != nil {
				var over *dsl.Override
				if names, ok := renamer.ReplaceNames(row.Fields); ok {
					ov.Key = hydra.BookmarksField
					ov.Value = names
					over = &ov
				}
				entry := metrics[row.TemplateName]
				if entry == nil {
					entry = &Metrics{}
					metrics[row.TemplateName] = entry
				}
				dsl.CollectAnchors(ct.Template, row.Fields, over, &entry.Anchors)
			}
		}
		if row.GraphCol > *maxGraphCol {
			*maxGraphCol = row.GraphCol
		}
	}
}

// EmitClassified writes one classified line to out.
func EmitClassified(line []byte, row *Row, templates map[string]*CompiledTemplate, metrics map[string]*Metrics, maxGraphCol int, hy *hydra.Walk, out *bytes.Buffer) {
	body, trailingNL := render.StripTrailingNL(line)
	switch row.Kind {
	case RowCommit:
		prefix := body[:row.GraphEnd]
		markup := hy.Markup(row.Fields, prefix, out)
		render.EmitDimGraph(prefix, config.Get().GraphCollapse, markup.Node, out)
		// Pass the graph-to-content gap through to dsl.RenderRow as a leading
		// ws segment so it participates in rules 1-3 (collapse on empty
		// fields, dash-fill across adjacent whitespace) alongside the
		// template's own whitespace.
		leadingPad := 0
		if maxGraphCol > row.GraphCol {
			leadingPad = maxGraphCol - row.GraphCol
		}
		leadingLeft := dsl.LeftGraphNode
		if row.LastIsEdge {
			leadingLeft = dsl.LeftGraphEdge
		}
		if !row.HasTemplateName {
			// Row parsed as fields but carried no bijjou_template_name - we
			// have nothing to render with. Pass the rest of the line through
			// verbatim instead of dropping the payload.
			out.Write(body[row.GraphEnd:])
			if trailingNL {
				out.WriteByte('\n')
			}
			return
		}
		switch ct, found := templates[row.TemplateName]; {
		case !found:
			emitMissingTemplate(row.TemplateName, leadingPad, leadingLeft, out)
		case ct.Empty:
			// Configured but empty - render the graph only and drop the rest
			// of the row.
		default:
			m := metrics[row.TemplateName]
			if m == nil {
				m = emptyMetrics
			}
			var over *dsl.Override
			if markup.RewroteBookmarks {
				over = &dsl.Override{Key: hydra.BookmarksField, Value: markup.Bookmarks}
			}
			dsl.RenderRow(ct.Template, row.Fields, over, leadingPad, leadingLeft, m.Anchors, out)
		}
	case RowRoot:
		render.EmitDimGraph(body[:row.GraphEnd], config.Get().GraphCollapse, nil, out)
		dsl.EmitNodePad(2, out)
		out.Write(row.Value)
	default:
		// render.EmitLine owns trailing newline handling for the passthrough
		// branch, so return early without our own \n append.
		render.EmitLine(line, row.Parsed, out)
		return
	}
	if trailingNL {
		out.WriteByte('\n')
	}
}

// emitMissingTemplate renders a single-row notice for a row whose
// bijjou_template_name does not match any configured templates.<name>. The dim
// SGR pair wraps the message, so it renders in bright black to match the graph
// filler.
func emitMissingTemplate(name string, leadingPad int, leadingLeft dsl.LeftSide, out *bytes.Buffer) {
	c := config.Get()
	const msg = "no bijjou template for "
	buf := make([]byte, 0, len(c.DimOn)+len(msg)+len(name)+len(ansi.FGReset))
	buf = append(buf, c.DimOn...)
	buf = append(buf, msg...)
	buf = append(buf, name...)
	buf = append(buf, ansi.FGReset...)
	synth := &dsl.Template{Nodes: []dsl.Node{
		{Kind: dsl.NodeLiteral, Literal: []byte(" ")},
		{Kind: dsl.NodeField, Name: "__bijjou_msg"},
	}}
	fields := map[string][]byte{"__bijjou_msg": buf}
	dsl.RenderRow(synth, fields, nil, leadingPad, leadingLeft, emptyMetrics.Anchors, out)
}

// SplitLines cuts input into lines that keep their trailing newline. A final
// line without a newline is kept as is.
func SplitLines(input []byte) [][]byte {
	lines := make([][]byte, 0, bytes.Count(input, []byte{'\n'})+1)
	start := 0
	for i, b := range input {
		if b == '\n' {
			lines = append(lines, input[start:i+1])
			start = i + 1
		}
	}
	if start < len(input) {
		lines = append(lines, input[start:])
	}
	return lines
}

// RunBuffered reads all of stdin, renders it in one pass, and writes the
// result through the output sink.
func RunBuffered() error {
	c := config.Get()

	// Bookmark naming comes from hydra.prefixes; no lookup to overlap.
	hy := hydra.NewWalk()
	input, err := io.ReadAll(os.Stdin)
	if err != nil {
		return err
	}

	if c.Activate == config.ModeAuto && !bytes.Contains(input, []byte(config.BijjouTemplateNameField)) {
		_, err := os.Stdout.Write(input)
		return err
	}

	templates, err := CompileTemplates(c.Templates)
	if err != nil {
		return err
	}
	lines := SplitLines(input)
	rows := make([]Row, len(lines))
	for i, l := range lines {
		body, _ := render.StripTrailingNL(l)
		rows[i] = ClassifyRow(body)
	}

	metrics := make(map[string]*Metrics)
	maxGraphCol := 0
	AccumulateMetrics(rows, templates, metrics, &maxGraphCol)

	var out bytes.Buffer
	out.Grow(len(input) + len(lines)*16)
	for i, line := range lines {
		EmitClassified(line, &rows[i], templates, metrics, maxGraphCol, hy, &out)
	}
	return output.WriteOutput(out.Bytes())
}
