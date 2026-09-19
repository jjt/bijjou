package ansi

import (
	"bytes"
	"strconv"
	"testing"
)

func TestSkipCSIBasicSGR(t *testing.T) {
	end, ok := SkipCSI([]byte("\x1b[31mfoo"), 0)
	if !ok || end != 5 {
		t.Errorf("got (%d, %v), want (5, true)", end, ok)
	}
}

func TestSkipCSINoEscapeReturnsNone(t *testing.T) {
	if _, ok := SkipCSI([]byte("foo"), 0); ok {
		t.Error("plain bytes must not open a CSI sequence")
	}
}

func TestSkipCSIWithMultiParamSGR(t *testing.T) {
	end, ok := SkipCSI([]byte("\x1b[38;5;245mX"), 0)
	if !ok || end != 11 {
		t.Errorf("got (%d, %v), want (11, true)", end, ok)
	}
}

func TestSkipCSIUnterminatedReturnsBufferEnd(t *testing.T) {
	end, ok := SkipCSI([]byte("\x1b[38;5"), 0)
	if !ok || end != 6 {
		t.Errorf("got (%d, %v), want (6, true)", end, ok)
	}
}

func TestSkipCSIAtOffset(t *testing.T) {
	end, ok := SkipCSI([]byte("X\x1b[1m"), 1)
	if !ok || end != 5 {
		t.Errorf("got (%d, %v), want (5, true)", end, ok)
	}
}

func TestDecodeUTF8(t *testing.T) {
	cases := []struct {
		name string
		in   []byte
		cp   uint32
		size int
	}{
		{"ascii", []byte("A"), 0x41, 1},
		{"two byte", []byte("\xc2\xa3"), 0xa3, 2},
		{"three byte circle", []byte("\xe2\x97\x8b"), 0x25CB, 3},
		{"three byte diamond", []byte("\xe2\x97\x86"), 0x25C6, 3},
		{"four byte supplementary", []byte("\xf0\x9f\x98\x80"), 0x1F600, 4},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			cp, size := DecodeUTF8(c.in, 0)
			if cp != c.cp || size != c.size {
				t.Errorf("got (%#x, %d), want (%#x, %d)", cp, size, c.cp, c.size)
			}
		})
	}
}

func TestFGColorBasic30s(t *testing.T) {
	for code := 30; code <= 37; code++ {
		if !IsFGColorSGR(strconv.Itoa(code)) {
			t.Errorf("%d must be a foreground color", code)
		}
	}
	if !IsFGColorSGR("39") {
		t.Error("39 must be a foreground color")
	}
}

func TestFGColorBright90s(t *testing.T) {
	for code := 90; code <= 97; code++ {
		if !IsFGColorSGR(strconv.Itoa(code)) {
			t.Errorf("%d must be a foreground color", code)
		}
	}
}

func TestFGColorExtended256AndTruecolor(t *testing.T) {
	for _, params := range []string{"38;5;245", "38;2;255;199;83"} {
		if !IsFGColorSGR(params) {
			t.Errorf("%q must be a foreground color", params)
		}
	}
}

func TestFGColorRejectsBGAndAttrs(t *testing.T) {
	for _, params := range []string{"0", "1", "3", "40", "49", "48;5;1"} {
		if IsFGColorSGR(params) {
			t.Errorf("%q must not be a foreground color", params)
		}
	}
}

func TestFGColorHandlesEmptyAndGarbage(t *testing.T) {
	for _, params := range []string{"", "xyz"} {
		if IsFGColorSGR(params) {
			t.Errorf("%q must not be a foreground color", params)
		}
	}
}

func TestFilteredANSIDropsFGKeepsAttrsAndText(t *testing.T) {
	var out bytes.Buffer
	EmitFilteredANSI([]byte("\x1b[1m\x1b[31mhello\x1b[39m\x1b[0m"), &out, IsFGColorSGR)
	if got, want := out.String(), "\x1b[1mhello\x1b[0m"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestFilteredANSIPassthroughPlainText(t *testing.T) {
	var out bytes.Buffer
	EmitFilteredANSI([]byte("plain"), &out, IsFGColorSGR)
	if got := out.String(); got != "plain" {
		t.Errorf("got %q, want %q", got, "plain")
	}
}

func TestFilteredANSIDropsTruecolorFG(t *testing.T) {
	var out bytes.Buffer
	EmitFilteredANSI([]byte("\x1b[38;2;255;199;83mtext\x1b[39m"), &out, IsFGColorSGR)
	if got := out.String(); got != "text" {
		t.Errorf("got %q, want %q", got, "text")
	}
}

func TestStripSGRRemovesAllSGRKeepsText(t *testing.T) {
	got := StripSGR([]byte("\x1b[1m\x1b[31mfoo\x1b[39m\x1b[0m bar"))
	if !bytes.Equal(got, []byte("foo bar")) {
		t.Errorf("got %q, want %q", got, "foo bar")
	}
}

func TestStripSGRPreservesNonSGRCSI(t *testing.T) {
	got := StripSGR([]byte("\x1b[Hhello\x1b[31m!\x1b[0m"))
	if !bytes.Equal(got, []byte("\x1b[Hhello!")) {
		t.Errorf("got %q, want %q", got, "\x1b[Hhello!")
	}
}
