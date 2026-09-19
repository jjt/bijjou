package stream

import (
	"bufio"
	"bytes"
	"testing"
)

func TestReadBatchReturnsNLines(t *testing.T) {
	r := bufio.NewReader(bytes.NewReader([]byte("a\nb\nc\nd\n")))
	batch, err := readBatch(r, 3)
	if err != nil {
		t.Fatalf("readBatch: %v", err)
	}
	if len(batch) != 3 {
		t.Fatalf("len = %d, want 3", len(batch))
	}
	for i, want := range []string{"a\n", "b\n", "c\n"} {
		if string(batch[i]) != want {
			t.Errorf("line %d = %q, want %q", i, batch[i], want)
		}
	}
}

func TestReadBatchStopsAtEOF(t *testing.T) {
	r := bufio.NewReader(bytes.NewReader([]byte("a\nb\n")))
	batch, err := readBatch(r, 10)
	if err != nil {
		t.Fatalf("readBatch: %v", err)
	}
	if len(batch) != 2 {
		t.Errorf("len = %d, want 2", len(batch))
	}
}

func TestReadBatchHandlesTrailingNoNewline(t *testing.T) {
	r := bufio.NewReader(bytes.NewReader([]byte("a\nb")))
	batch, err := readBatch(r, 10)
	if err != nil {
		t.Fatalf("readBatch: %v", err)
	}
	if len(batch) != 2 {
		t.Fatalf("len = %d, want 2", len(batch))
	}
	if string(batch[0]) != "a\n" {
		t.Errorf("line 0 = %q, want %q", batch[0], "a\n")
	}
	if string(batch[1]) != "b" {
		t.Errorf("line 1 = %q, want %q", batch[1], "b")
	}
}

func TestReadBatchEmptyInputReturnsEmpty(t *testing.T) {
	r := bufio.NewReader(bytes.NewReader(nil))
	batch, err := readBatch(r, 5)
	if err != nil {
		t.Fatalf("readBatch: %v", err)
	}
	if len(batch) != 0 {
		t.Errorf("len = %d, want 0", len(batch))
	}
}
