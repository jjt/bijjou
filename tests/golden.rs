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

// Single golden: pipe tests/fixtures/local.txt through bijjou under bijjou-config.toml and
// snapshot the result. ui.color is forced to "always" so the SGR sequences are
// stable regardless of whether the test runner is a TTY. out.local.txt holds a
// human-readable copy of the same rendered output (raw ANSI bytes).
#[test]
fn local() {
    let root = root_dir();
    let input = std::fs::read(root.join("tests/fixtures/local.txt")).expect("tests/fixtures/local.txt");

    let output = Command::cargo_bin("bijjou")
        .expect("binary built")
        .env("BIJJOU_CONFIG", root.join("bijjou-config.toml"))
        .env("BIJJOU__UI__COLOR", "always")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    insta::with_settings!({description => "tests/fixtures/local.txt rendered under bijjou-config.toml (matches out.local.txt)."}, {
        insta::assert_snapshot!("local", visualize(&output));
    });
}
