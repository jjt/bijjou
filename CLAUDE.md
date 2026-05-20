# Project memory

## Feedback

### Always use character literals in strings, even PUA codepoints

Prefer the actual UTF-8 glyph in source (e.g. `""`) over escape sequences
like `"\u{f040}"`. Applies to all string literals in this project,
including Private Use Area codepoints (Nerd Fonts, supplementary PUA).

**Why:** matches the existing style for the other icon/edge/node
constants in `src/config.rs`, which all hold the literal glyph. Keeping
the source consistent makes the constants scannable at a glance.

**How to apply:** when adding or editing string constants, if a
codepoint can be represented as a literal character, write the literal
character. Don't reach for `\u{...}` escapes for printability or
"safety" — the file is UTF-8 and the constants render fine.
