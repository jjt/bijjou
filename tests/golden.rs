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
        .env("BIJJOU_CONFIG", "/dev/null")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone()
}

fn config_path(name: &str) -> PathBuf {
    fixture_dir().join("configs").join(format!("{}.toml", name))
}

fn run_bijjou_with_config(input: &[u8], config: &str) -> Vec<u8> {
    let path = config_path(config);
    Command::cargo_bin("bijjou")
        .expect("binary built")
        .env("BIJJOU_CONFIG", &path)
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone()
}

fn snapshot_with_config(input_name: &str, config: &str, snap_name: &str) {
    let input = read_fixture(input_name);
    let output = run_bijjou_with_config(&input, config);
    insta::assert_snapshot!(snap_name, visualize(&output));
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

#[test]
fn diff_authors() {
    // Mix of single-name, multi-word, CJK, and emoji authors — exercises how
    // jj's truncate/pad on the author cell shapes a variable-width column.
    snapshot("diff_authors");
}

#[test]
fn remote_bookmarks() {
    // Local main ahead of origin: shows `main*` (locally-modified marker) and
    // `main@origin` (remote-tracking bookmark) on different commits.
    snapshot("remote_bookmarks");
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

// --- Degenerate inputs --------------------------------------------------
// These exercise bijjou's robustness against malformed / unexpected input
// that real jj output usually wouldn't produce. Behavior should be
// deterministic and never panic.

fn snap_bytes(name: &str, input: &[u8]) {
    let output = run_bijjou(input);
    insta::assert_snapshot!(name, visualize(&output));
}

#[test]
fn degenerate_author_column_missing() {
    // Realistic format minus the author cell.
    snap_bytes(
        "degenerate_author_column_missing",
        b"@  abcde 12345 260509\xc2\xb70958 desc\n",
    );
}

#[test]
fn degenerate_timestamp_missing() {
    snap_bytes(
        "degenerate_timestamp_missing",
        b"@  abcde 12345 ME desc-without-timestamp\n",
    );
}

#[test]
fn degenerate_no_description() {
    // Bare graph + change/commit IDs only.
    snap_bytes("degenerate_no_description", b"\xe2\x97\x8b  abcde 12345\n");
}

#[test]
fn degenerate_only_newlines() {
    snap_bytes("degenerate_only_newlines", b"\n\n\n");
}

#[test]
fn degenerate_only_csi_no_text() {
    // Pure escape sequences with no visible characters.
    snap_bytes("degenerate_only_csi_no_text", b"\x1b[31m\x1b[39m\n");
}

#[test]
fn degenerate_unterminated_csi() {
    // CSI cut off without final byte (no terminator). bijjou should not panic;
    // it consumes to end-of-buffer.
    snap_bytes("degenerate_unterminated_csi", b"@  abc\x1b[38;5;");
}

#[test]
fn degenerate_no_trailing_newline() {
    snap_bytes(
        "degenerate_no_trailing_newline",
        b"\xe2\x97\x8b  abcde 12345 desc",
    );
}

#[test]
fn degenerate_many_consecutive_markers() {
    // Stack of markers (𝙀𝙄𝙀𝙄) — all should be stripped, no doubled spacing.
    let mut buf = b"@  abc ".to_vec();
    for _ in 0..4 {
        buf.extend_from_slice(b"\xf0\x9d\x99\x80"); // 𝙀
        buf.extend_from_slice(b"\xf0\x9d\x99\x84"); // 𝙄
    }
    buf.extend_from_slice(b"desc\n");
    snap_bytes("degenerate_many_consecutive_markers", &buf);
}

#[test]
fn degenerate_marker_at_line_start() {
    snap_bytes(
        "degenerate_marker_at_line_start",
        b"\xf0\x9d\x99\x80@  abcde desc\n",
    );
}

#[test]
fn degenerate_invalid_utf8_byte() {
    // Stray 0x80 (continuation byte) in the middle of content. bijjou's UTF-8
    // decoder treats it as a 1-byte char; output should pass through.
    snap_bytes(
        "degenerate_invalid_utf8_byte",
        b"@  abcde \x80 strange desc\n",
    );
}

#[test]
fn degenerate_tab_in_content() {
    snap_bytes(
        "degenerate_tab_in_content",
        b"@  abcde 12345 ME 260509 desc\twith\ttabs\n",
    );
}

#[test]
fn degenerate_blank_graph_line_between() {
    // Mid-graph blank line (jj doesn't normally emit this).
    snap_bytes(
        "degenerate_blank_graph_line_between",
        b"\xe2\x97\x8b  abcde 12345\n\n\xe2\x97\x8b  fghij 67890\n",
    );
}

#[test]
fn degenerate_graph_only_no_content() {
    // Lines with graph chars but no content after.
    snap_bytes(
        "degenerate_graph_only_no_content",
        b"\xe2\x94\x82\n\xe2\x94\x9c\xe2\x94\x80\xe2\x95\xaf\n",
    );
}

#[test]
fn degenerate_long_line_no_graph() {
    // 500-byte plain text line; emit_dim_graph wraps each non-space char
    // individually — make sure that scales.
    let body: Vec<u8> = std::iter::repeat(b'x').take(500).collect();
    let mut buf = body.clone();
    buf.push(b'\n');
    snap_bytes("degenerate_long_line_no_graph", &buf);
}

#[test]
fn degenerate_csi_then_newline() {
    // CSI with newline immediately after.
    snap_bytes("degenerate_csi_then_newline", b"\x1b[1m\n");
}

#[test]
fn degenerate_solo_marker_byte_no_newline() {
    // Empty marker as the only input, no trailing newline.
    snap_bytes("degenerate_solo_marker_byte_no_newline", b"\xf0\x9d\x99\x80");
}

#[test]
fn degenerate_marker_after_graph_only() {
    // Graph + marker but no content. The marker should still be stripped.
    snap_bytes(
        "degenerate_marker_after_graph_only",
        b"\xe2\x97\x8b  \xf0\x9d\x99\x80\n",
    );
}

// --- Custom-config fixtures --------------------------------------------
// Each pipes a stock input through bijjou with a non-default config from
// tests/fixtures/configs/*.toml. The snapshot captures the configured
// glyphs/colors so regressions in config plumbing show up here.

#[test]
fn config_ascii_linear_chain() {
    snapshot_with_config("linear_chain", "ascii", "config_ascii_linear_chain");
}

#[test]
fn config_ascii_branching() {
    snapshot_with_config("branching", "ascii", "config_ascii_branching");
}

#[test]
fn config_ascii_workspaces() {
    snapshot_with_config("workspaces", "ascii", "config_ascii_workspaces");
}

#[test]
fn config_hex_colors_workspaces() {
    snapshot_with_config(
        "workspaces",
        "hex_colors",
        "config_hex_colors_workspaces",
    );
}

#[test]
fn config_dash_only_workspaces() {
    snapshot_with_config(
        "workspaces",
        "dash_only",
        "config_dash_only_workspaces",
    );
}

#[test]
fn config_alt_nodes_kitchen_sink() {
    // Kitchen-sink fixture exercises every node type (@, ○, ◆, ×, ●,
    // empty, immutable @, empty-immutable). Renders them via a non-default
    // BMP-only icon set so the override applies across all branches.
    snapshot_with_config(
        "combo_kitchen_sink",
        "alt_nodes",
        "config_alt_nodes_kitchen_sink",
    );
}
