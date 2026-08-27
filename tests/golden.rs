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
    let root = root_dir();
    let path = root.join("tests/fixtures").join(fixture);
    let input = std::fs::read(&path).unwrap_or_else(|_| panic!("{}", path.display()));

    let mut cmd = Command::cargo_bin("bijjou").expect("binary built");
    cmd.env("BIJJOU_CONFIG", root.join("bijjou-config.toml"))
        .env("BIJJOU__UI__COLOR", "always");
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
