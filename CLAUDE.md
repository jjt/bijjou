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

### Do not run `mise run install` during the refactor

User pinned an older bijjou binary at rev `z` on their system so they
can keep using stable bijjou while the content-rendering refactor is
in flight. Don't install over it.

**Why:** auto-installing the half-finished working copy would replace
their stable tool with a build that can't yet render commit content.

**How to apply:** build/test locally (`cargo build`, `cargo test`)
during this refactor work, but skip any `install` task. Re-enable only
when the user signals shippable.

### Never auto-accept snapshot changes — present the diff for review

When golden/insta snapshots differ, show the user the diff and let them
accept. Do NOT run `INSTA_UPDATE=always`, `cargo insta accept`, or delete
+ regenerate `.snap` files to silence a mismatch.

**Why:** the snapshots are the source of truth for rendered output;
silently overwriting them hides regressions the user needs to eyeball.

**How to apply:** run the tests, and on a mismatch surface the pending
diff (`cargo insta test` then `cargo insta review`, or show the failing
test's diff / `mise run show-golden`). Let the user decide before any
snapshot is written.
