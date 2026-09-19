package pipeline

import (
	"bytes"
	"testing"
)

func TestSplitLinesKeepsNewlines(t *testing.T) {
	got := SplitLines([]byte("a\nb\nc\n"))
	want := []string{"a\n", "b\n", "c\n"}
	if len(got) != len(want) {
		t.Fatalf("len = %d, want %d", len(got), len(want))
	}
	for i := range want {
		if string(got[i]) != want[i] {
			t.Errorf("line %d = %q, want %q", i, got[i], want[i])
		}
	}
}

func TestSplitLinesKeepsTrailingPartialLine(t *testing.T) {
	got := SplitLines([]byte("a\nb"))
	if len(got) != 2 {
		t.Fatalf("len = %d, want 2", len(got))
	}
	if string(got[1]) != "b" {
		t.Errorf("last line = %q, want %q", got[1], "b")
	}
}

func TestSplitLinesEmptyInput(t *testing.T) {
	if got := SplitLines(nil); len(got) != 0 {
		t.Errorf("len = %d, want 0", len(got))
	}
}

func TestClassifyRowCommitRecord(t *testing.T) {
	row := ClassifyRow([]byte("@  change_id\x00abc\x00bijjou_template_name\x00log_oneline\x1e"))
	if row.Kind != RowCommit {
		t.Fatalf("kind = %v, want RowCommit", row.Kind)
	}
	if !row.HasTemplateName || row.TemplateName != "log_oneline" {
		t.Errorf("template name = %q (has=%v), want %q", row.TemplateName, row.HasTemplateName, "log_oneline")
	}
	if !bytes.Equal(row.Fields["change_id"], []byte("abc")) {
		t.Errorf("change_id = %q, want %q", row.Fields["change_id"], "abc")
	}
	if _, ok := row.Fields["bijjou_template_name"]; ok {
		t.Error("bijjou_template_name must be removed from the field map")
	}
	if row.GraphEnd != 1 {
		t.Errorf("graph end = %d, want 1", row.GraphEnd)
	}
	if row.GraphCol != 1 {
		t.Errorf("graph col = %d, want 1", row.GraphCol)
	}
	if row.LastIsEdge {
		t.Error("node glyph must not report last_is_edge")
	}
}

func TestClassifyRowRootRecord(t *testing.T) {
	row := ClassifyRow([]byte("@  root\x00zzzzzzzz\x1e"))
	if row.Kind != RowRoot {
		t.Fatalf("kind = %v, want RowRoot", row.Kind)
	}
	if !bytes.Equal(row.Value, []byte("zzzzzzzz")) {
		t.Errorf("value = %q, want %q", row.Value, "zzzzzzzz")
	}
	if row.GraphEnd != 1 {
		t.Errorf("graph end = %d, want 1", row.GraphEnd)
	}
}

func TestClassifyRowFramedWithoutTemplateName(t *testing.T) {
	row := ClassifyRow([]byte("@  author\x00pat\x00description\x00hi\x1e"))
	if row.Kind != RowCommit {
		t.Fatalf("kind = %v, want RowCommit", row.Kind)
	}
	if row.HasTemplateName {
		t.Errorf("template name = %q, want none", row.TemplateName)
	}
}

func TestClassifyRowUnframedTextIsPassthroughWithoutBoundary(t *testing.T) {
	row := ClassifyRow([]byte("hello world"))
	if row.Kind != RowPassthrough {
		t.Fatalf("kind = %v, want RowPassthrough", row.Kind)
	}
	if row.Parsed != nil {
		t.Error("a line with no graph prefix must carry no boundary")
	}
}

func TestClassifyRowGraphLineWithoutRecordKeepsBoundary(t *testing.T) {
	row := ClassifyRow([]byte("@  some prose"))
	if row.Kind != RowPassthrough {
		t.Fatalf("kind = %v, want RowPassthrough", row.Kind)
	}
	if row.Parsed == nil {
		t.Fatal("a graph-prefixed line must keep the boundary find_boundary located")
	}
	if row.Parsed.GraphEnd != 1 {
		t.Errorf("graph end = %d, want 1", row.Parsed.GraphEnd)
	}
}
