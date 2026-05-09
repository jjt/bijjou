use assert_cmd::Command;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_fixture(name: &str) -> Vec<u8> {
    let path = fixture_dir().join(format!("{}.txt", name));
    std::fs::read(&path).unwrap_or_else(|_| panic!("fixture missing: {}", path.display()))
}

fn run_bijjou(input: &[u8]) -> Vec<u8> {
    Command::cargo_bin("bijjou")
        .expect("binary built")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone()
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

fn snapshot(name: &str) {
    let input = read_fixture(name);
    let output = run_bijjou(&input);
    insta::assert_snapshot!(name, visualize(&output));
}

#[test]
fn empty_input() {
    snapshot("empty");
}

#[test]
fn single_working_copy() {
    snapshot("single_wc");
}

#[test]
fn single_working_copy_no_graph() {
    snapshot("single_wc_no_graph");
}

#[test]
fn linear_chain() {
    snapshot("linear_chain");
}

#[test]
fn root_immutable() {
    snapshot("root_immutable");
}

#[test]
fn mixed_with_elision() {
    snapshot("mixed_with_elision");
}

#[test]
fn plain_text_passthrough() {
    snapshot("plain_text");
}

#[test]
fn branching_graph() {
    snapshot("branching");
}

#[test]
fn merge_graph() {
    snapshot("merge_graph");
}

// --- Real-state fixtures: bookmarks, conflicts, hidden, divergent, workspaces,
// --- megamerges, and combinations.

#[test]
fn bookmarks() {
    snapshot("bookmarks");
}

#[test]
fn conflicted() {
    snapshot("conflicted");
}

#[test]
fn hidden_revisions() {
    snapshot("hidden");
}

#[test]
fn divergent() {
    snapshot("divergent");
}

#[test]
fn workspaces() {
    snapshot("workspaces");
}

#[test]
fn megamerge() {
    snapshot("megamerge");
}

#[test]
fn combo_conflicted_working_copy() {
    snapshot("combo_conflicted_wc");
}

#[test]
fn combo_megamerge_conflict() {
    snapshot("combo_megamerge_conflict");
}

#[test]
fn combo_kitchen_sink() {
    // Single capture exercising bookmarks, hidden, divergent, conflict on
    // working copy, multi-workspace markers, octopus megamerge, and immutable
    // root all in one log output.
    snapshot("combo_kitchen_sink");
}

// Synthetic fixtures: hand-crafted bytes that exercise specific paths
// without depending on a captured jj session.

#[test]
fn synthetic_only_marker_byte() {
    // Empty marker on its own line — bijjou should strip it and emit just \n.
    let input = b"\xf0\x9d\x99\x80\n";
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_only_marker_byte", visualize(&output));
}

#[test]
fn synthetic_immutable_diamond_no_color() {
    // Immutable ◆ with no surrounding color; bijjou should darken it and
    // replace glyph with the lock icon.
    let input = "\u{25C6}  zzzzz root() 000000\n".as_bytes();
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_immutable_diamond_no_color", visualize(&output));
}

#[test]
fn synthetic_mutable_circle_no_color() {
    // Mutable ○ with no color — gets darkened.
    let input = "\u{25CB}  abcde 12345 desc\n".as_bytes();
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_mutable_circle_no_color", visualize(&output));
}

#[test]
fn synthetic_wc_immutable_lock() {
    // @ on a line that carries the immutable marker: glyph becomes the lock,
    // node is darkened (lock wins over WC visuals).
    let input = "@  abc \u{1D644}desc\n".as_bytes();
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_wc_immutable_lock", visualize(&output));
}

#[test]
fn synthetic_wc_empty() {
    // @ on an empty line: WC_EMPTY_ICON.
    let input = "@  abc \u{1D640}desc\n".as_bytes();
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_wc_empty", visualize(&output));
}

#[test]
fn synthetic_immutable_empty_diamond() {
    // ◆ + empty marker: EMPTY_IMMUTABLE_ICON.
    let input = "\u{25C6}  abc \u{1D640}\u{1D644}desc\n".as_bytes();
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_immutable_empty_diamond", visualize(&output));
}

#[test]
fn synthetic_conflict_node() {
    // × → conflict icon, fg color preserved (no darken).
    let input = "\u{00D7}  abc desc\n".as_bytes();
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_conflict_node", visualize(&output));
}

#[test]
fn synthetic_box_drawing_only() {
    // Pure graph segment with no node — should still emit edge dim color.
    let input = "│ ├─╯\n".as_bytes();
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_box_drawing_only", visualize(&output));
}

#[test]
fn synthetic_alignment_dash_filler() {
    // When graph_col is short relative to max graph width, bijjou should
    // pad to align and (for short-side, change-id-looking content) emit
    // dash filler. Two lines: one tall graph, one short.
    let input = b"\xe2\x94\x82 \xe2\x94\x82 \xe2\x97\x8b  abcde 12345\n\xe2\x97\x8b  fghij 67890\n";
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_alignment_dash_filler", visualize(&output));
}

#[test]
fn synthetic_marker_inside_csi_segment() {
    // Marker bytes wrapped in jj's color envelope — bijjou should strip the
    // marker but the (now empty) color codes shouldn't introduce extra spacing.
    let input = b"@  abc \x1b[38;5;10m\xf0\x9d\x99\x80\x1b[39mdesc\n";
    let output = run_bijjou(input);
    insta::assert_snapshot!("synthetic_marker_inside_csi_segment", visualize(&output));
}
