// Package hydra holds hydra awareness: it recognizes the stacks of a jj hydra
// in the log and marks them up. A hydra merges several linear stacks as
// siblings off one base, so `jj log` prints one graph column per stack. Two
// things follow from that:
//
//   - The log's top stack has no `├─╯` row under it. jj closes a graph column
//     only when the branch to its *left* ends. So every stack but the topmost
//     gets a closing row. The topmost runs straight into its neighbor.
//     `hydra.top-stack-padding` draws the separator jj skipped.
//   - Each stack owns a column. A color per stack makes the columns readable
//     at a glance (`hydra.colors`). If the stack's own bookmark names carry
//     the same color, they read as part of that column (`hydra.color-bookmarks`).
//
// The bookmark names come from `hydra.prefixes`. bijjou classifies a row from
// its `bookmarks` field plus the graph prefix jj already drew. No subprocess
// and no repo lookup are needed. A repo that renamed its hydra bookmarks says
// so in config. A repo with no hydra has no bookmark that matches. So it gets
// no markup at all.
//
// The column bounds a stack at the bottom. A marker opens its stack. The rows
// under it in the same column are its content. A commit drawn in another
// column is nobody's stack and keeps jj's own colors. This commit is an extra
// head off the base, between the bottom stack and `HYB`.
package hydra

import (
	"bytes"
	"math"
	"strconv"
	"strings"
	"unicode/utf8"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
	"tangled.org/jjt.io/bijjou/internal/render"
)

// BookmarksField names the row field a hydra classification reads.
const BookmarksField = "bookmarks"

// replacement is what one bookmark leader renders as. set is false for a
// leader whose `hydra.prefixes-replace` key is unset. That name passes through
// as jj printed it.
type replacement struct {
	bytes []byte
	set   bool
}

func replacementOfValue(r config.Replacement) replacement {
	return replacement{bytes: []byte(r.Value), set: r.Set}
}

// tokenReplace is the stand-in for one bookmark token's leader, plus the count
// of name bytes it stands in for. set is false when the token keeps the leader
// jj printed: no key for it, or no hydra bookmark at all.
type tokenReplace struct {
	standIn []byte
	drop    int
	set     bool
}

// topology holds the bookmark names a hydra uses here. bijjou expands them
// from `hydra.prefixes` once. Each name pairs with its stand-in under
// `hydra.prefixes-replace`.
type topology struct {
	// `HYS-` — the marker bookmark that opens a stack.
	stackPrefix string
	// `HYWC-` — a stack's working copy, which sits outside the stack.
	wcPrefix string
	// `HYB` / `HYH` / `HYCR`.
	anchors []string
	// What each of those leaders renders as. anchorReplace is index-aligned
	// with anchors.
	stackReplace  replacement
	wcReplace     replacement
	anchorReplace []replacement
}

func newTopology(p *config.HydraPrefixes, r *config.HydraPrefixReplace) *topology {
	anchorsReplace := p.AnchorsReplace(r)
	anchorReplace := make([]replacement, len(anchorsReplace))
	for i := range anchorsReplace {
		anchorReplace[i] = replacementOfValue(anchorsReplace[i])
	}
	return &topology{
		stackPrefix:   p.StackMarker(),
		wcPrefix:      p.WorkingCopy(),
		anchors:       p.Anchors(),
		stackReplace:  replacementOfValue(p.StackMarkerReplace(r)),
		wcReplace:     replacementOfValue(p.WorkingCopyReplace(r)),
		anchorReplace: anchorReplace,
	}
}

// replaces reports whether any name has a stand-in. When it does not, the
// rewrite only applies colors.
func (t *topology) replaces() bool {
	if t.stackReplace.set || t.wcReplace.set {
		return true
	}
	for i := range t.anchorReplace {
		if t.anchorReplace[i].set {
			return true
		}
	}
	return false
}

// replacementOf gives what one bookmark token renders as.
func (t *topology) replacementOf(token []byte) tokenReplace {
	// jj's out-of-sync `*` is not part of the name. A remote ref is not a
	// local hydra bookmark. markOf applies the same rules.
	name := trimFlag(token)
	if bytes.IndexByte(name, '@') >= 0 {
		return tokenReplace{}
	}
	for _, leader := range [2]struct {
		prefix  string
		replace replacement
	}{
		{t.stackPrefix, t.stackReplace},
		{t.wcPrefix, t.wcReplace},
	} {
		if rest, found := cutPrefix(name, leader.prefix); found && len(rest) > 0 {
			if !leader.replace.set {
				return tokenReplace{}
			}
			return tokenReplace{standIn: leader.replace.bytes, drop: len(leader.prefix), set: true}
		}
	}
	for i, anchor := range t.anchors {
		if string(name) != anchor {
			continue
		}
		if !t.anchorReplace[i].set {
			return tokenReplace{}
		}
		return tokenReplace{standIn: t.anchorReplace[i].bytes, drop: len(anchor), set: true}
	}
	return tokenReplace{}
}

// Markup is what one row's hydra classification changes about its rendering.
type Markup struct {
	// SGR for the row's graph node. nil leaves the node as jj drew it.
	Node []byte
	// The row's `bookmarks` field, with every hydra bookmark recolored to its
	// stack.
	Bookmarks []byte
	// False leaves the `bookmarks` field as jj printed it.
	RewroteBookmarks bool
}

// Walk is the per-row state, walked top-down with the log.
type Walk struct {
	// nil means `hydra.enable = false`. Then every row passes through untouched.
	topo *topology
	// Stack names in the order the log first named them, top first. When
	// `hydra.colors` is a palette, this order gives each stack its slot.
	seen []string
	// SGR for the stack the walk is inside. It is empty outside any stack.
	color []byte
	// The graph cell that stack's nodes sit in, so a row drawn in another
	// column is recognized as some other branch. columnSet is false outside
	// any stack.
	column    int
	columnSet bool
	// SGR for a single `HYWC-*` row. This row belongs to a stack but is not
	// in it. The color applies to that row and is not carried down.
	wcColor    []byte
	stacksSeen int
	// Reused scratch for the bookmarks field, ANSI removed.
	scratch []byte
	// Reused buffer for the rewritten `bookmarks` field.
	bookmarks []byte
}

// NewWalk starts a walk. It reads the topology in force for this run out of
// config. `hydra.prefixes` and `hydra.prefixes-replace` are config, so every
// pass reads the same names. A walk with `hydra.enable = false` classifies and
// renames nothing.
func NewWalk() *Walk {
	c := config.Get()
	w := &Walk{}
	if c.HydraEnable {
		w.topo = newTopology(&c.HydraPrefixes, &c.HydraPrefixesReplace)
	}
	return w
}

// Markup classifies one commit row and returns what its rendering takes from
// the hydra. prefix is the row's graph prefix as jj drew it. When this row
// opens the second stack, bijjou draws the top stack's missing separator from
// it into out first.
//
// Rows must arrive in log order. The walk carries a stack's color down from
// its marker through its content commits. These content commits are the rows
// below the marker in its own graph column.
func (w *Walk) Markup(fields map[string][]byte, prefix []byte, out *bytes.Buffer) Markup {
	raw := fields[BookmarksField]
	w.scratch = w.scratch[:0]
	stripANSIInto(raw, &w.scratch)
	// A nil topology means `hydra.enable = false`. Then bijjou classifies nothing.
	if w.topo == nil {
		return Markup{}
	}
	m := markOf(w.topo, w.scratch)

	// A working copy sits above the head, outside every stack. So it ends
	// whichever stack the walk was in. But the working copy is that stack's
	// row. It takes the stack's color and does not carry it down.
	fromWC := m.kind == markWorkingCopy
	switch m.kind {
	case markOutside:
		w.leave()
	// A stack owns one graph column, from its marker down to its last content
	// commit. So a row whose node sits in another column is another branch
	// entirely, for example an extra head off the base. The stack ends above
	// that row and does not lend it a color.
	case markInside:
		if len(w.color) > 0 {
			cell, ok := render.NodeCell(prefix)
			if ok != w.columnSet || (ok && cell != w.column) {
				w.leave()
			}
		}
	case markStack:
		if w.stacksSeen == 1 && config.Get().HydraTopStackPadding {
			emitPadding(prefix, out)
		}
		w.stacksSeen++
		w.color = stackColor(m.name, indexOf(&w.seen, m.name))
		w.column, w.columnSet = render.NodeCell(prefix)
	case markWorkingCopy:
		w.leave()
		w.wcColor = stackColor(m.name, indexOf(&w.seen, m.name))
	}

	rewritten := false
	switch {
	case config.Get().HydraColorBookmarks:
		rewritten = rewriteBookmarks(w.topo, &w.seen, &w.scratch, raw, &w.bookmarks)
	// Colors off, names still replaced. `hydra.prefixes-replace` is
	// independent of `hydra.color-bookmarks`.
	case w.topo.replaces():
		rewritten = rewriteBookmarks(w.topo, nil, &w.scratch, raw, &w.bookmarks)
	}

	node := w.color
	if fromWC {
		node = w.wcColor
	}
	markup := Markup{RewroteBookmarks: rewritten}
	if len(node) > 0 {
		markup.Node = node
	}
	if rewritten {
		markup.Bookmarks = w.bookmarks
	}
	return markup
}

// leave puts the walk out of every stack. The walk is between the head and the
// markers, or below the last one. The next marker row starts the state over.
func (w *Walk) leave() {
	w.color = nil
	w.column = 0
	w.columnSet = false
}

// Renamer runs the pass-1 substitution, which needs no walk state.
type Renamer struct {
	// nil when no name has a stand-in, or `hydra.enable = false`.
	topo    *topology
	scratch []byte
	names   []byte
}

// NewRenamer builds the renamer for this run out of config, on the same names
// a Walk reads.
func NewRenamer() *Renamer {
	c := config.Get()
	r := &Renamer{}
	if c.HydraEnable {
		topo := newTopology(&c.HydraPrefixes, &c.HydraPrefixesReplace)
		if topo.replaces() {
			r.topo = topo
		}
	}
	return r
}

// ReplaceNames gives the row's `bookmarks` field at its rendered width. Pass 1
// needs that width, and `hydra.prefixes-replace` changes it, so the same
// substitution runs there. It runs without the colors, which cost no width.
// ok is false when the row has no name to stand in for. Then the caller keeps
// jj's field.
func (r *Renamer) ReplaceNames(fields map[string][]byte) (names []byte, ok bool) {
	if r.topo == nil {
		return nil, false
	}
	if !rewriteBookmarks(r.topo, nil, &r.scratch, fields[BookmarksField], &r.names) {
		return nil, false
	}
	return r.names, true
}

// indexOf gives a stack's position in the graph, top first. The log itself is
// the order. So a stack keeps its slot for the whole run once met.
func indexOf(seen *[]string, name string) int {
	for i, n := range *seen {
		if n == name {
			return i
		}
	}
	*seen = append(*seen, name)
	return len(*seen) - 1
}

// markKind is where a row sits relative to the stacks. A row can open a stack,
// carry a stack's working copy, sit outside every stack, or continue in
// whichever stack the walk is already in. A stack's own content commits name no
// bookmark at all.
type markKind int

const (
	markStack markKind = iota
	markWorkingCopy
	markOutside
	markInside
)

// mark is one row's classification, with the stack it names.
type mark struct {
	kind markKind
	name string
}

func markOf(topo *topology, bookmarks []byte) mark {
	wc := ""
	wcSet := false
	outside := false
	for i := 0; i < len(bookmarks); {
		if isASCIIWhitespace(bookmarks[i]) {
			i++
			continue
		}
		start := i
		for i < len(bookmarks) && !isASCIIWhitespace(bookmarks[i]) {
			i++
		}
		// jj flags a bookmark out of sync with its remote with a trailing `*`.
		// jj prints remote refs as `name@remote`. Only local names count.
		token := trimFlag(bookmarks[start:i])
		if len(token) == 0 || bytes.IndexByte(token, '@') >= 0 {
			continue
		}
		if name, found := cutPrefix(token, topo.stackPrefix); found && len(name) > 0 {
			return mark{kind: markStack, name: lossyString(name)}
		}
		if name, found := cutPrefix(token, topo.wcPrefix); found && len(name) > 0 {
			wc, wcSet = lossyString(name), true
			continue
		}
		for _, anchor := range topo.anchors {
			if string(token) == anchor {
				outside = true
				break
			}
		}
	}
	switch {
	case wcSet:
		return mark{kind: markWorkingCopy, name: wc}
	case outside:
		return mark{kind: markOutside}
	default:
		return mark{kind: markInside}
	}
}

// rewriteBookmarks rewrites the row's `bookmarks` field into out. Every hydra
// bookmark on it carries its stack's color instead of jj's color. Each
// bookmark reads under the stand-in `hydra.prefixes-replace` gives its leader.
// Bookmarks are whitespace separated, and each comes wrapped in jj's own SGR.
// So a name that bijjou takes over loses its foreground codes, and the stack's
// codes go in front. Every other bookmark on the row is copied byte-for-byte.
// seen carries the palette order. seen is nil when bijjou replaces only the
// names: pass 1, or `hydra.color-bookmarks = false`. This returns false when
// the row has no name to take over. Then the caller keeps jj's field.
func rewriteBookmarks(topo *topology, seen *[]string, scratch *[]byte, raw []byte, out *[]byte) bool {
	*out = (*out)[:0]
	hit := false
	i := 0
	for i < len(raw) {
		// CSI sequences carry no whitespace. So a split on raw bytes keeps
		// each name together with the color codes around it.
		start := i
		ws := isASCIIWhitespace(raw[i])
		for i < len(raw) && isASCIIWhitespace(raw[i]) == ws {
			i++
		}
		token := raw[start:i]
		if ws {
			*out = append(*out, token...)
			continue
		}
		*scratch = (*scratch)[:0]
		stripANSIInto(token, scratch)
		var color []byte
		colored := false
		if seen != nil {
			if name, ok := stackOf(topo, *scratch); ok {
				color = stackColor(name, indexOf(seen, name))
				colored = true
			}
		}
		replace := topo.replacementOf(*scratch)
		switch {
		case colored && len(color) > 0:
			*out = append(*out, color...)
			emitToken(token, true, replace, out)
			*out = append(*out, ansi.FGReset...)
			hit = true
		// Nothing to recolor, but the name still reads as its stand-in, in
		// whichever color jj gave it.
		case replace.set:
			emitToken(token, false, replace, out)
			hit = true
		default:
			*out = append(*out, token...)
		}
	}
	return hit
}

// emitToken copies one bookmark token into out. dropFG drops jj's foreground
// SGRs and keeps the caller's color in force. replace is the stand-in for the
// token's leader, plus the byte count it stands in for. bijjou counts that over
// the name's own bytes. The CSI sequences jj wrapped it in are copied either
// way. So the color around the name survives the substitution.
func emitToken(token []byte, dropFG bool, replace tokenReplace, out *[]byte) {
	dropLeft := replace.drop
	pending := replace.set
	i := 0
	for i < len(token) {
		if end, ok := ansi.SkipCSI(token, i); ok {
			fg := false
			if params, isSGR := ansi.SGRParams(token[i:end]); isSGR {
				fg = ansi.IsFGColorSGR(params)
			}
			if !(dropFG && fg) {
				*out = append(*out, token[i:end]...)
			}
			i = end
			continue
		}
		if pending {
			*out = append(*out, replace.standIn...)
			pending = false
		}
		if dropLeft > 0 {
			dropLeft--
			i++
			continue
		}
		*out = append(*out, token[i])
		i++
	}
}

// stackOf gives the stack a single bookmark name belongs to. Both `HYS-<name>`
// and `HYWC-<name>` name their stack. The same flag and remote-ref rules as
// markOf apply.
func stackOf(topo *topology, token []byte) (string, bool) {
	token = trimFlag(token)
	if bytes.IndexByte(token, '@') >= 0 {
		return "", false
	}
	for _, prefix := range [2]string{topo.stackPrefix, topo.wcPrefix} {
		if name, found := cutPrefix(token, prefix); found && len(name) > 0 {
			return lossyString(name), true
		}
	}
	return "", false
}

// emitPadding draws the separator row jj skipped under the log's top stack.
// bijjou draws it from the next stack's marker row, so every column lands where
// it does above and below.
func emitPadding(prefix []byte, out *bytes.Buffer) {
	verticals := render.GraphNodesToVerticals(prefix)
	render.EmitDimGraph(verticals, config.Get().GraphCollapse, nil, out)
	out.WriteByte('\n')
}

func stackColor(name string, index int) []byte {
	colors := config.Get().HydraColors
	switch colors.Mode {
	case config.HydraColorsHash:
		return hashColor(name)
	case config.HydraColorsPalette:
		if len(colors.Palette) == 0 {
			return nil
		}
		return colors.Palette[index%len(colors.Palette)]
	default:
		return nil
	}
}

// reservedHues are the hues the hashed palette does not hand out: the hue of a
// color a stack must not read as, plus 10° either side. `#a6e3a1` sits at 115°
// and `#f5c2e7` at 316°. The bands are ascending and disjoint, which hueOf
// counts on.
var reservedHues = [2][2]uint32{{105, 125}, {306, 326}}

// hueSpace is the hues left to hand out, the reserved bands taken off the
// circle.
var hueSpace = func() uint64 {
	left := uint64(360)
	for _, band := range reservedHues {
		left -= uint64(band[1] - band[0] + 1)
	}
	return left
}()

// hueOf gives the index-th hue still on offer, with index in 0..hueSpace. This
// function skips reserved bands and does not clamp them. So no hue is handed
// out twice as often as another.
func hueOf(index uint32) uint32 {
	hue := index
	for _, band := range reservedHues {
		if hue >= band[0] {
			hue += band[1] - band[0] + 1
		}
	}
	return hue
}

// hashColor gives a stack's color, which must be stable across runs and
// distinct from its neighbors. FNV-1a over the name picks a hue out of
// hueSpace. Saturation and lightness are fixed, so every stack lands in the
// same legible band. A hash straight into rgb hands out near-blacks and
// near-whites instead.
func hashColor(name string) []byte {
	h := uint64(0xcbf2_9ce4_8422_2325)
	for i := 0; i < len(name); i++ {
		h ^= uint64(name[i])
		h *= 0x0000_0100_0000_01b3
	}
	hue := hueOf(uint32(h % hueSpace))
	r, g, b := hslToRGB(hue, 0.68, stackLightness(hue))
	sgr := make([]byte, 0, 20)
	sgr = append(sgr, "\x1b[38;2;"...)
	sgr = strconv.AppendUint(sgr, uint64(r), 10)
	sgr = append(sgr, ';')
	sgr = strconv.AppendUint(sgr, uint64(g), 10)
	sgr = append(sgr, ';')
	sgr = strconv.AppendUint(sgr, uint64(b), 10)
	return append(sgr, 'm')
}

// stackLightness lifts the lightness of the blues around 241°, which read dark
// against the terminal. The ramp is linear inside a 20° band either side of
// 241°. The lightness is 0.62 at the band edges, up to 0.70 at 241° itself.
// Outside the band, the fixed 0.62 stands.
func stackLightness(hue uint32) float64 {
	const (
		peakHue = 241.0
		band    = 20.0
		base    = 0.62
		peak    = 0.70
	)
	distance := math.Abs(float64(hue) - peakHue)
	if distance >= band {
		return base
	}
	return base + (peak-base)*(band-distance)/band
}

func hslToRGB(hue uint32, sat, light float64) (uint8, uint8, uint8) {
	chroma := (1.0 - math.Abs(2.0*light-1.0)) * sat
	sector := hue / 60
	second := chroma * (1.0 - math.Abs(math.Mod(float64(hue)/60.0, 2.0)-1.0))
	var r, g, b float64
	switch sector {
	case 0:
		r, g, b = chroma, second, 0.0
	case 1:
		r, g, b = second, chroma, 0.0
	case 2:
		r, g, b = 0.0, chroma, second
	case 3:
		r, g, b = 0.0, second, chroma
	case 4:
		r, g, b = second, 0.0, chroma
	default:
		r, g, b = chroma, 0.0, second
	}
	base := light - chroma/2.0
	return rgbByte(r, base), rgbByte(g, base), rgbByte(b, base)
}

func rgbByte(v, base float64) uint8 {
	n := math.Round((v + base) * 255.0)
	if n < 0.0 {
		n = 0.0
	} else if n > 255.0 {
		n = 255.0
	}
	return uint8(n)
}

// stripANSIInto appends bytes to out without its CSI sequences, so bookmark
// names split on real whitespace and not on the color codes jj wraps them in.
func stripANSIInto(b []byte, out *[]byte) {
	i := 0
	for i < len(b) {
		if after, ok := ansi.SkipCSI(b, i); ok {
			i = after
			continue
		}
		*out = append(*out, b[i])
		i++
	}
}

// trimFlag drops jj's out-of-sync `*`, which is not part of the name.
func trimFlag(token []byte) []byte {
	if n := len(token); n > 0 && token[n-1] == '*' {
		return token[:n-1]
	}
	return token
}

// cutPrefix gives the bytes behind prefix, when name leads with it.
func cutPrefix(name []byte, prefix string) ([]byte, bool) {
	if len(name) < len(prefix) || string(name[:len(prefix)]) != prefix {
		return nil, false
	}
	return name[len(prefix):], true
}

// isASCIIWhitespace reports whether b separates two bookmarks: space,
// horizontal tab, newline, form feed, or carriage return.
func isASCIIWhitespace(b byte) bool {
	return b == ' ' || b == '\t' || b == '\n' || b == '\f' || b == '\r'
}

// lossyString reads name bytes as a string, with every invalid UTF-8 sequence
// replaced by U+FFFD. One replacement stands for one maximal invalid subpart,
// so a stack whose name carries stray bytes still hashes to one color.
func lossyString(b []byte) string {
	if utf8.Valid(b) {
		return string(b)
	}
	var out strings.Builder
	out.Grow(len(b))
	for i := 0; i < len(b); {
		if r, size := utf8.DecodeRune(b[i:]); r != utf8.RuneError || size > 1 {
			out.Write(b[i : i+size])
			i += size
			continue
		}
		out.WriteRune(utf8.RuneError)
		i += invalidLen(b[i:])
	}
	return out.String()
}

// invalidLen gives the length of the invalid sequence that b leads with. A
// lead byte with the continuation bytes its width calls for counts as one
// sequence, up to the first byte that does not belong to it.
func invalidLen(b []byte) int {
	lead := b[0]
	switch {
	case lead < 0xc2 || lead > 0xf4:
		return 1
	case lead < 0xe0:
		// A two-byte lead fails on its continuation byte alone.
		return 1
	case lead < 0xf0:
		lo, hi := byte(0x80), byte(0xbf)
		if lead == 0xe0 {
			lo = 0xa0
		} else if lead == 0xed {
			hi = 0x9f
		}
		if len(b) < 2 || b[1] < lo || b[1] > hi {
			return 1
		}
		return 2
	default:
		lo, hi := byte(0x80), byte(0xbf)
		if lead == 0xf0 {
			lo = 0x90
		} else if lead == 0xf4 {
			hi = 0x8f
		}
		if len(b) < 2 || b[1] < lo || b[1] > hi {
			return 1
		}
		if len(b) < 3 || b[2] < 0x80 || b[2] > 0xbf {
			return 2
		}
		return 3
	}
}
