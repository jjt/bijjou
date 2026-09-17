use assert_cmd::Command;
use std::path::PathBuf;

fn root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// Render bytes for snapshots. CSI escapes appear as \e[...X so the params and
// final byte are visible and stable. Control chars become \xNN. Valid UTF-8
// passes through. Newlines stay literal so multiline output renders naturally.
fn visualize(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                let mut j = i + 2;
                while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() {
                    j += 1;
                    let params = std::str::from_utf8(&bytes[i + 2..j - 1]).unwrap_or("?");
                    let final_byte = bytes[j - 1] as char;
                    out.push_str(&format!("\\e[{}{}", params, final_byte));
                    i = j;
                    continue;
                }
                out.push_str("\\e[");
                i += 2;
                continue;
            }
            out.push_str("\\e");
            i += 1;
            continue;
        }
        if b == b'\n' {
            out.push('\n');
            i += 1;
            continue;
        }
        if b < 0x20 || b == 0x7f {
            out.push_str(&format!("\\x{:02x}", b));
            i += 1;
            continue;
        }
        if b < 0x80 {
            out.push(b as char);
            i += 1;
            continue;
        }
        let len = match b {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xff => 4,
            _ => 1,
        };
        let end = (i + len).min(bytes.len());
        match std::str::from_utf8(&bytes[i..end]) {
            Ok(s) => {
                out.push_str(s);
                i = end;
            }
            Err(_) => {
                out.push_str(&format!("\\x{:02x}", b));
                i += 1;
            }
        }
    }
    out
}

// Pipe a fixture through bijjou under bijjou-config.toml and snapshot the
// result. ui.color is forced to "always" so the SGR sequences are stable
// regardless of whether the test runner is a TTY. `env` adds further
// BIJJOU__ overrides.
fn render_fixture_with(fixture: &str, env: &[(&str, &str)]) -> String {
    render_fixture_under(fixture, &root_dir().join("bijjou-config.toml"), env)
}

// The same, under a config file of the test's own — a template the stock
// config does not carry, say.
fn render_fixture_under(fixture: &str, config: &std::path::Path, env: &[(&str, &str)]) -> String {
    let root = root_dir();
    let path = root.join("tests/fixtures").join(fixture);
    let input = std::fs::read(&path).unwrap_or_else(|_| panic!("{}", path.display()));

    let mut cmd = Command::cargo_bin("bijjou").expect("binary built");
    cmd.env("BIJJOU_CONFIG", config)
        .env("BIJJOU__UI__COLOR", "always")
        // Hydra markup is opt-in per test: without this the result would
        // depend on whether the checkout bijjou is built in happens to be a
        // hydra, and on whether `hydra` is on PATH at all.
        .env("BIJJOU__HYDRA__ENABLE", "false");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    visualize(&output)
}

fn render_fixture(fixture: &str) -> String {
    render_fixture_with(fixture, &[])
}

// local.txt rendered under bijjou-config.toml (matches out.local.txt).
#[test]
fn local() {
    insta::with_settings!({description => "tests/fixtures/local.txt rendered under bijjou-config.toml (matches out.local.txt)."}, {
        insta::assert_snapshot!("local", render_fixture("local.txt"));
    });
}

// Regression: rows whose graph node is a custom `log_node` glyph (■ U+25A0,
// Nerd-Font PUA U+F28D) must render, not pass through raw. Built-in nodes (●)
// and edges share the fixture for contrast. Guards the structural node
// detection in render.rs::is_node_at.
#[test]
fn custom_nodes() {
    insta::with_settings!({description => "tests/fixtures/custom_nodes.txt: custom log_node glyphs (■, PUA) render via structural node detection."}, {
        insta::assert_snapshot!("custom_nodes", render_fixture("custom_nodes.txt"));
    });
}

// `graph.collapse = true`: jj's inter-column pad cells are dropped, so every
// graph column sits one cell from the last and the graph→content gap shrinks
// with it. Guards render.rs::is_pad_cell plus the collapsed graph_col /
// last_is_edge pair that feeds the dash run.
#[test]
fn local_collapsed() {
    insta::with_settings!({description => "tests/fixtures/local.txt rendered with graph.collapse = true."}, {
        insta::assert_snapshot!("local_collapsed", render_fixture_with("local.txt", &[("BIJJOU__GRAPH__COLLAPSE", "true")]));
    });
}

// A hydra log: `hydra.top-stack-padding` draws the separator row jj skips
// under the log's top stack (delta here), `hydra.colors` colours each
// stack's nodes — the hashed default, so this also pins the name → colour
// mapping, and each `HYWC-*` row up top matching its own stack — and
// `hydra.color-bookmarks` puts that same colour on the `HYS-*` / `HYWC-*`
// names themselves. The bookmark naming comes from `hydra.prefixes`, so no
// `hydra` call and no live hydra are involved. The row above `HYB` is an
// extra head off the base, in no stack and in its own graph column, so it
// keeps jj's colours.
#[test]
fn hydra() {
    insta::with_settings!({description => "tests/fixtures/hydra.txt: top-stack padding row plus hashed per-stack node and bookmark colors."}, {
        insta::assert_snapshot!("hydra", render_fixture_with("hydra.txt", &HYDRA_ENV));
    });
}

// `hydra.colors = [...]`: the palette is indexed by the order the log first
// names each stack — the working-copy rows up top here (delta → 1, gamma → 2,
// beta → 3, alpha → 4), and each stack matches its own working copy.
#[test]
fn hydra_palette() {
    let env = hydra_env(&[("BIJJOU__HYDRA__COLORS", "1,2,3,4")]);
    insta::with_settings!({description => "tests/fixtures/hydra.txt: hydra.colors as an explicit palette indexed by first sighting."}, {
        insta::assert_snapshot!("hydra_palette", render_fixture_with("hydra.txt", &env));
    });
}

// Both hydra knobs off: byte-for-byte the pre-hydra rendering, so a repo with
// no hydra (or `hydra.enable = false`) is unaffected.
#[test]
fn hydra_disabled() {
    let env = hydra_env(&[
        ("BIJJOU__HYDRA__TOP_STACK_PADDING", "false"),
        ("BIJJOU__HYDRA__COLORS", "false"),
    ]);
    assert_eq!(
        render_fixture_with("hydra.txt", &env),
        render_fixture_with("hydra.txt", &[("BIJJOU__GRAPH__COLLAPSE", "true")]),
    );
}

// `hydra.prefixes`: rename the prefix and the fixture's `HY*` bookmarks stop
// being hydra bookmarks, so the markup drops out entirely.
#[test]
fn hydra_prefixes_rename() {
    let env = hydra_env(&[("BIJJOU__HYDRA__PREFIXES__PREFIX", "ZZ")]);
    assert_eq!(
        render_fixture_with("hydra.txt", &env),
        render_fixture_with("hydra.txt", &[("BIJJOU__GRAPH__COLLAPSE", "true")]),
    );
}

// `hydra.color-bookmarks = false`: the stacks keep their node colours, but
// every bookmark name is left in jj's own colour (magenta here).
#[test]
fn hydra_plain_bookmarks() {
    let env = hydra_env(&[("BIJJOU__HYDRA__COLOR_BOOKMARKS", "false")]);
    let off = render_fixture_with("hydra.txt", &env);
    let on = render_fixture_with("hydra.txt", &HYDRA_ENV);
    // delta's hashed colour, which its node carries in both renderings.
    let delta = "\\e[38;2;156;224;92m";
    assert!(off.contains(&format!("{}", delta)), "{}", off);
    assert!(off.contains("\\e[38;5;5mHYS-delta"), "{}", off);
    assert!(!off.contains(&format!("{}HYS-delta", delta)), "{}", off);
    assert!(on.contains(&format!("{}HYS-delta", delta)), "{}", on);
    assert!(on.contains(&format!("{}HYWC-delta", delta)), "{}", on);
    // The anchors are nobody's stack, so they keep jj's colour either way.
    assert!(off.contains("\\e[38;5;5mHYH"), "{}", off);
    assert!(on.contains("\\e[38;5;5mHYH"), "{}", on);
}

// `hydra.prefixes-replace`: `stack-head` and `stack-working-copy` stand in
// for the whole leader, dash included, so `HYS-delta` reads `Ψdelta` and
// `HYWC-delta` reads `ψdelta`; `head` replaces the whole `HYH` name. `base`
// and `conflict-resolution` are unset, so `HYB main` is left as jj printed
// it. The stack colours are unchanged — the names are renamed, not
// reclassified.
#[test]
fn hydra_prefixes_replace() {
    let env = hydra_env(&[
        ("BIJJOU__HYDRA__PREFIXES_REPLACE__STACK_HEAD", "Ψ"),
        ("BIJJOU__HYDRA__PREFIXES_REPLACE__STACK_WORKING_COPY", "ψ"),
        ("BIJJOU__HYDRA__PREFIXES_REPLACE__HEAD", "◆"),
    ]);
    insta::with_settings!({description => "tests/fixtures/hydra.txt: hydra.prefixes-replace stand-ins for the stack, working-copy and head leaders."}, {
        insta::assert_snapshot!("hydra_prefixes_replace", render_fixture_with("hydra.txt", &env));
    });
}

// A stand-in is not the width of the name it replaces, so pass 1 has to
// measure the elastic-tab anchors on the replaced field. With a tab behind
// `%{bookmarks}` and a stand-in wider than the leader, every row's tab must
// still land in one column.
#[test]
fn hydra_prefixes_replace_keeps_elastic_tabs_aligned() {
    let config =
        std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tab-after-bookmarks.toml");
    std::fs::write(
        &config,
        "[templates]\nlog_oneline = ''' %{elastic_tab(change_id)} %{bookmarks} %{elastic_tab()}|%{description}'''\n",
    )
    .unwrap();
    let env = hydra_env(&[("BIJJOU__HYDRA__PREFIXES_REPLACE__STACK_HEAD", "stack/")]);
    let out = render_fixture_under("hydra.txt", &config, &env);
    assert!(out.contains("stack/delta"), "{}", out);

    // Rule 2 collapses a cell on the rows whose `bookmarks` is empty, so the
    // comparison is over the rows that carry a bookmark: a replaced name and
    // an untouched one have to land in the same column.
    let columns: Vec<usize> = out
        .lines()
        .map(plain)
        .filter(|line| line.contains("|hydra ") || line.contains("|Update A"))
        .filter_map(|line| line.chars().position(|c| c == '|'))
        .collect();
    // Four working copies, the head, four stack markers and the base.
    assert_eq!(columns.len(), 10, "{}", out);
    assert!(
        columns.iter().all(|col| *col == columns[0]),
        "{:?}\n{}",
        columns,
        out
    );
}

// One visualized line with its `\e[...X` sequences taken back out, so a
// column count is a column count.
fn plain(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("\\e[") {
        out.push_str(&rest[..at]);
        rest = &rest[at + 3..];
        match rest.find(|c: char| ('@'..='~').contains(&c)) {
            Some(end) => rest = &rest[end + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

// Hydra markup on, plus `graph.collapse`, which is how the hydra logs this is
// modelled on are actually read.
const HYDRA_ENV: [(&str, &str); 2] = [
    ("BIJJOU__HYDRA__ENABLE", "true"),
    ("BIJJOU__GRAPH__COLLAPSE", "true"),
];

fn hydra_env<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    HYDRA_ENV
        .iter()
        .copied()
        .chain(extra.iter().copied())
        .collect()
}
