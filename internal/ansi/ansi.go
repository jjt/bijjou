// Package ansi holds the byte-level ANSI helpers the render path needs: CSI
// skipping, UTF-8 decoding, and SGR filtering. Every function works on raw
// bytes, because bijjou forwards jj's own escape sequences unchanged.
package ansi

import (
	"bytes"
	"strconv"
	"strings"
	"unicode/utf8"
)

// FGReset ends a foreground color run.
var FGReset = []byte("\x1b[39m")

// SkipCSI gives the index just past the CSI sequence that starts at i. ok is
// false when the bytes at i do not open a CSI sequence. A sequence with no
// final byte runs to the end of the buffer.
func SkipCSI(b []byte, i int) (end int, ok bool) {
	if i+1 >= len(b) || b[i] != 0x1b || b[i+1] != '[' {
		return 0, false
	}
	j := i + 2
	for j < len(b) {
		c := b[j]
		j++
		if c >= 0x40 && c <= 0x7e {
			return j, true
		}
	}
	return j, true
}

// DecodeUTF8 gives the codepoint at i and the count of bytes it occupies. A
// truncated or invalid sequence gives the lead byte itself and a length of 1,
// so the caller always makes progress.
func DecodeUTF8(b []byte, i int) (cp uint32, size int) {
	c := b[i]
	switch {
	case c < 0x80:
		return uint32(c), 1
	case c < 0xc0:
		return uint32(c), 1
	case c < 0xe0 && i+1 < len(b):
		return uint32(c&0x1f)<<6 | uint32(b[i+1]&0x3f), 2
	case c < 0xf0 && i+2 < len(b):
		return uint32(c&0x0f)<<12 | uint32(b[i+1]&0x3f)<<6 | uint32(b[i+2]&0x3f), 3
	case i+3 < len(b):
		return uint32(c&0x07)<<18 | uint32(b[i+1]&0x3f)<<12 | uint32(b[i+2]&0x3f)<<6 | uint32(b[i+3]&0x3f), 4
	}
	return uint32(c), 1
}

// IsFGColorSGR reports whether the parameter bytes of an SGR sequence set a
// foreground color: the 30-37 and 90-97 ranges, the 39 reset, and the extended
// 38;5 and 38;2 forms. Background codes and attributes are not colors.
func IsFGColorSGR(params string) bool {
	parts := strings.Split(params, ";")
	code := uint64(999)
	if n, err := strconv.ParseUint(parts[0], 10, 16); err == nil {
		code = n
	}
	switch {
	case code >= 30 && code <= 37, code == 39, code >= 90 && code <= 97:
		return true
	case code == 38:
		return len(parts) > 1 && (parts[1] == "5" || parts[1] == "2")
	}
	return false
}

// SGRParams gives the parameter bytes of an SGR sequence (CSI ... m). ok is
// false for every other CSI sequence, whose parameters must not be read.
func SGRParams(seq []byte) (params string, ok bool) {
	if len(seq) < 3 || seq[0] != 0x1b || seq[1] != '[' || seq[len(seq)-1] != 'm' {
		return "", false
	}
	inner := seq[2 : len(seq)-1]
	if !utf8.Valid(inner) {
		return "", true
	}
	return string(inner), true
}

// StripSGR removes every SGR (CSI ... m) sequence from b. Other CSI sequences
// and plain bytes pass through unchanged.
func StripSGR(b []byte) []byte {
	var out bytes.Buffer
	out.Grow(len(b))
	EmitFilteredANSI(b, &out, func(string) bool { return true })
	return out.Bytes()
}

// EmitFilteredANSI copies src into out. The filter reads the parameters of
// each SGR sequence and returns true for the ones to remove. Every other byte,
// and every non-SGR CSI sequence, is copied verbatim.
func EmitFilteredANSI(src []byte, out *bytes.Buffer, filter func(params string) bool) {
	i := 0
	for i < len(src) {
		end, ok := SkipCSI(src, i)
		if !ok {
			out.WriteByte(src[i])
			i++
			continue
		}
		if params, isSGR := SGRParams(src[i:end]); isSGR && filter(params) {
			i = end
			continue
		}
		out.Write(src[i:end])
		i = end
	}
}
