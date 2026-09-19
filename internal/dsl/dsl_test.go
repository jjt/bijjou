package dsl

import (
	"bytes"
	"strings"
	"testing"
)

func countKind(t *Template, kind NodeKind) int {
	n := 0
	for _, node := range t.Nodes {
		if node.Kind == kind {
			n++
		}
	}
	return n
}

func TestParseDefaultTemplate(t *testing.T) {
	src := " %{elastic_tab(change_id)}\n%{elastic_tab(commit_id)}\n%{elastic_tab(author)}\n%{elastic_tab(timestamp)}\n%{working_copies}\n%{bookmarks}\n%{tags}\n%{description}"
	tmpl, err := Parse(src)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if got := countKind(tmpl, NodeElasticTab); got != 4 {
		t.Errorf("elastic count = %d, want 4", got)
	}
	if got := countKind(tmpl, NodeField); got != 4 {
		t.Errorf("field count = %d, want 4", got)
	}
}

func TestParseLiteralNewlineEscape(t *testing.T) {
	// `\n` literal in source becomes a real newline in output.
	tmpl, err := Parse("a\\nb")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if tmpl.Nodes[0].Kind != NodeLiteral {
		t.Fatalf("expected literal, got kind %d", tmpl.Nodes[0].Kind)
	}
	if !bytes.Equal(tmpl.Nodes[0].Literal, []byte("a\nb")) {
		t.Errorf("literal = %q, want %q", tmpl.Nodes[0].Literal, "a\nb")
	}
}

func TestParseUTF8Literal(t *testing.T) {
	// The parse pre-pass widens every byte latin-1 style, so a multi-byte
	// UTF-8 literal re-encodes byte by byte. The Rust binary emits the same
	// bytes.
	tmpl, err := Parse("X\u25cbY%{change_id}Z")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	want := []byte("X\xc3\xa2\xc2\x97\xc2\x8bY")
	if tmpl.Nodes[0].Kind != NodeLiteral || !bytes.Equal(tmpl.Nodes[0].Literal, want) {
		t.Errorf("literal = %q, want %q", tmpl.Nodes[0].Literal, want)
	}
	if len(tmpl.Nodes) != 3 || tmpl.Nodes[1].Kind != NodeField || tmpl.Nodes[1].Name != "change_id" {
		t.Fatalf("nodes = %+v", tmpl.Nodes)
	}
	if !bytes.Equal(tmpl.Nodes[2].Literal, []byte("Z")) {
		t.Errorf("tail literal = %q, want %q", tmpl.Nodes[2].Literal, "Z")
	}
}

func TestParseUnterminatedBraceErrors(t *testing.T) {
	_, err := Parse("%{foo")
	if err == nil {
		t.Fatal("expected error")
	}
	if err.Error() != "template: unterminated %{ at byte 0" {
		t.Errorf("err = %q", err.Error())
	}
}

func TestParseUnknownFunctionErrors(t *testing.T) {
	_, err := Parse("%{wat(foo)}")
	if err == nil {
		t.Fatal("expected error")
	}
	if err.Error() != "template: unknown function `wat`" {
		t.Errorf("err = %q", err.Error())
	}
}

func TestParseMissingCloseParenErrors(t *testing.T) {
	_, err := Parse("%{elastic_tab(foo}")
	if err == nil {
		t.Fatal("expected error")
	}
	if err.Error() != "template: missing `)` in `elastic_tab(foo`" {
		t.Errorf("err = %q", err.Error())
	}
}

func TestNULOnelineBasic(t *testing.T) {
	m, ok := ParseNULOneline([]byte("change_id\x00abc\x00commit_id\x00123\x1e"))
	if !ok {
		t.Fatal("expected a record")
	}
	if !bytes.Equal(m["change_id"], []byte("abc")) {
		t.Errorf("change_id = %q", m["change_id"])
	}
	if !bytes.Equal(m["commit_id"], []byte("123")) {
		t.Errorf("commit_id = %q", m["commit_id"])
	}
}

func TestNULOnelinePreservesRawESCAndNewlines(t *testing.T) {
	m, ok := ParseNULOneline([]byte("k\x00\x1b[1mhi\nthere\x1b[0m\x1e"))
	if !ok {
		t.Fatal("expected a record")
	}
	if !bytes.Equal(m["k"], []byte("\x1b[1mhi\nthere\x1b[0m")) {
		t.Errorf("k = %q", m["k"])
	}
}

func TestNULOnelineEmptyValue(t *testing.T) {
	m, ok := ParseNULOneline([]byte("labels\x00\x00description\x00hi\x1e"))
	if !ok {
		t.Fatal("expected a record")
	}
	v, present := m["labels"]
	if !present || len(v) != 0 {
		t.Errorf("labels = %q, present=%v", v, present)
	}
	if !bytes.Equal(m["description"], []byte("hi")) {
		t.Errorf("description = %q", m["description"])
	}
}

func TestNULOnelineRootRecord(t *testing.T) {
	m, ok := ParseNULOneline([]byte("root\x00zzzzzz root() 000000\x1e"))
	if !ok {
		t.Fatal("expected a record")
	}
	if len(m) != 1 {
		t.Errorf("len = %d, want 1", len(m))
	}
	if !bytes.Equal(m["root"], []byte("zzzzzz root() 000000")) {
		t.Errorf("root = %q", m["root"])
	}
}

func TestNULOnelineRejectsNoTerminator(t *testing.T) {
	if _, ok := ParseNULOneline([]byte("k\x00v")); ok {
		t.Error("expected no record")
	}
}

func TestNULOnelineRejectsOddParts(t *testing.T) {
	if _, ok := ParseNULOneline([]byte("k\x00v\x00orphan\x1e")); ok {
		t.Error("expected no record")
	}
}

func TestNULOnelineRejectsEmptyKey(t *testing.T) {
	if _, ok := ParseNULOneline([]byte("\x00v\x1e")); ok {
		t.Error("expected no record")
	}
}

func TestVisibleWidthSkipsCSI(t *testing.T) {
	if got := VisibleWidth([]byte("\x1b[1mhi\x1b[0m")); got != 2 {
		t.Errorf("width = %d, want 2", got)
	}
}

func TestVisibleWidthMultibyte(t *testing.T) {
	if got := VisibleWidth([]byte("○")); got != 1 {
		t.Errorf("width = %d, want 1", got)
	}
}

func TestTrailingTabAlignsFollowingField(t *testing.T) {
	// To align non-elastic content after an elastic column, put a tab before
	// it. The short row gets dash fill up to the aligned column. The widest
	// row has no pad.
	tmpl, err := Parse("%{elastic_tab(change_id)} %{elastic_tab()}%{description}")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	r1 := map[string][]byte{"change_id": []byte("abc"), "description": []byte("short")}
	r2 := map[string][]byte{"change_id": []byte("abcdef"), "description": []byte("longer")}
	var anchors []int
	CollectAnchors(tmpl, r1, nil, &anchors)
	CollectAnchors(tmpl, r2, nil, &anchors)
	// tab0 (change_id) at col 0. tab1 (before description) at
	// max(change_id width) + 1 literal space = 6 + 1 = 7.
	if len(anchors) != 2 || anchors[0] != 0 || anchors[1] != 7 {
		t.Fatalf("anchors = %v, want [0 7]", anchors)
	}

	var out bytes.Buffer
	RenderRow(tmpl, r1, nil, 0, LeftContent, anchors, &out)
	s := out.String()
	if !strings.HasPrefix(s, "abc") {
		t.Errorf("row1 = %q, want prefix abc", s)
	}
	if !strings.HasSuffix(s, "short") {
		t.Errorf("row1 = %q, want suffix short", s)
	}
	if !strings.Contains(s, "╶") && !strings.Contains(s, "─") {
		t.Errorf("expected dash pad: %s", s)
	}

	var out2 bytes.Buffer
	RenderRow(tmpl, r2, nil, 0, LeftContent, anchors, &out2)
	if out2.String() != "abcdef longer" {
		t.Errorf("row2 = %q, want %q", out2.String(), "abcdef longer")
	}
}

func TestArglessTabEqualsArgful(t *testing.T) {
	// `%{elastic_tab()}%{X}` must render byte-identically to
	// `%{elastic_tab(X)}` for every row.
	ta, err := Parse("%{elastic_tab(change_id)} %{description}")
	if err != nil {
		t.Fatalf("parse ta: %v", err)
	}
	tb, err := Parse("%{elastic_tab()}%{change_id} %{description}")
	if err != nil {
		t.Fatalf("parse tb: %v", err)
	}
	rows := []map[string][]byte{
		{"change_id": []byte("abc"), "description": []byte("short")},
		{"change_id": []byte("abcdef"), "description": []byte("longer")},
	}

	var anchorsA, anchorsB []int
	for _, r := range rows {
		CollectAnchors(ta, r, nil, &anchorsA)
		CollectAnchors(tb, r, nil, &anchorsB)
	}
	for _, r := range rows {
		var oa, ob bytes.Buffer
		RenderRow(ta, r, nil, 0, LeftContent, anchorsA, &oa)
		RenderRow(tb, r, nil, 0, LeftContent, anchorsB, &ob)
		if !bytes.Equal(oa.Bytes(), ob.Bytes()) {
			t.Errorf("argless and argful must match: %q vs %q", oa.String(), ob.String())
		}
	}
}

func TestEmptyFieldCollapsesPrecedingWs(t *testing.T) {
	// Rule 2: when %{labels} is empty, the literal " " between %{change_id}
	// and %{labels} is stripped. %{description} then sits directly after its
	// own preceding literal space.
	tmpl, err := Parse("%{change_id} %{labels} %{description}")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	fields := map[string][]byte{
		"change_id":   []byte("abc"),
		"labels":      []byte(""),
		"description": []byte("hi"),
	}
	var out bytes.Buffer
	RenderRow(tmpl, fields, nil, 0, LeftContent, nil, &out)
	if out.String() != "abc hi" {
		t.Errorf("out = %q, want %q", out.String(), "abc hi")
	}
}

func TestLeadingTemplateWsIsPreserved(t *testing.T) {
	// Rule 1: leading whitespace before the first non-ws content is preserved
	// verbatim.
	tmpl, err := Parse(" %{description}")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	fields := map[string][]byte{"description": []byte("hi")}
	var out bytes.Buffer
	RenderRow(tmpl, fields, nil, 0, LeftContent, nil, &out)
	if out.String() != " hi" {
		t.Errorf("out = %q, want %q", out.String(), " hi")
	}
}

func TestEmptyFirstFieldKeepsLeadingWsAndCollapsesRight(t *testing.T) {
	// Rule 1 protects the leading " " (no non-ws content to its left). Rule 2
	// is strictly left-only, so the " " after the empty field also survives.
	// The two cells combine under rule 3 into a dash fill before the next
	// non-ws content.
	tmpl, err := Parse(" %{labels} %{description}")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	fields := map[string][]byte{"labels": []byte(""), "description": []byte("hi")}
	var out bytes.Buffer
	RenderRow(tmpl, fields, nil, 0, LeftContent, nil, &out)
	s := out.String()
	if !strings.HasSuffix(s, "hi") {
		t.Errorf("out = %q, want suffix hi", s)
	}
	if !strings.Contains(s, "╶") && !strings.Contains(s, "─") {
		t.Errorf("expected dash fill: %s", s)
	}
}

func TestLeadingPadCombinesWithTemplateLeadingWs(t *testing.T) {
	// The graph pad passed through leadingPad joins the template's own
	// leading " " into a single dash run.
	tmpl, err := Parse(" %{change_id}")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	fields := map[string][]byte{"change_id": []byte("abc")}
	var out bytes.Buffer
	RenderRow(tmpl, fields, nil, 2, LeftGraphNode, nil, &out)
	// 2 leadingPad + 1 literal = 3 ws cells → dashes. This abuts "abc".
	s := out.String()
	if !strings.HasSuffix(s, "abc") {
		t.Errorf("out = %q, want suffix abc", s)
	}
	if !strings.Contains(s, "╶") && !strings.Contains(s, "─") {
		t.Errorf("expected dash pad: %s", s)
	}
}

func TestDashRunClosingCellIsASpace(t *testing.T) {
	// Default `dash-end` is "": the content keeps a space to its left, and
	// the run's width is unchanged.
	var out bytes.Buffer
	emitPad(4, LeftGraphNode, &out)
	if got := VisibleWidth(out.Bytes()); got != 4 {
		t.Errorf("width = %d, want 4", got)
	}
	if !strings.Contains(out.String(), "── ") {
		t.Errorf("expected space-terminated run: %q", out.String())
	}
}

func TestOneCellRunIsASpace(t *testing.T) {
	// A one-cell run is only its closing cell, whatever sits to the left.
	for _, left := range []LeftSide{LeftGraphNode, LeftGraphEdge, LeftContent} {
		var out bytes.Buffer
		emitPad(1, left, &out)
		if out.String() != " " {
			t.Errorf("left=%d: out = %q, want %q", left, out.String(), " ")
		}
	}
}
