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
    run_bijjou_with_config(input, "bypass_gate")
}

fn run_bijjou_default(input: &[u8]) -> Vec<u8> {
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

fn snapshot_with_config(input_name: &str, config: &str, snap_name: &str, desc: &str) {
    let input = read_fixture(input_name);
    let output = run_bijjou_with_config(&input, config);
    insta::with_settings!({description => desc}, {
        insta::assert_snapshot!(snap_name, visualize(&output));
    });
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

fn snapshot(name: &str, desc: &str) {
    let input = read_fixture(name);
    let output = run_bijjou(&input);
    insta::with_settings!({description => desc}, {
        insta::assert_snapshot!(name, visualize(&output));
    });
}

fn snap_bytes(name: &str, input: &[u8], desc: &str) {
    let output = run_bijjou(input);
    insta::with_settings!({description => desc}, {
        insta::assert_snapshot!(name, visualize(&output));
    });
}

#[test]
fn empty_input() {
    snapshot("empty", "Empty stdin → empty stdout.");
}

#[test]
fn single_working_copy() {
    snapshot("single_wc", "One working-copy commit; minimal graph.");
}

#[test]
fn single_working_copy_no_graph() {
    snapshot(
        "single_wc_no_graph",
        "Single line of input with no graph column at all.",
    );
}

#[test]
fn linear_chain() {
    snapshot(
        "linear_chain",
        "Straight chain of commits — baseline for vertical edge rendering.",
    );
}

#[test]
fn root_immutable() {
    snapshot(
        "root_immutable",
        "Root() commit — exercises immutable ◆ glyph and lock icon.",
    );
}

#[test]
fn mixed_with_elision() {
    snapshot(
        "mixed_with_elision",
        "Graph that includes a `~` elision marker for collapsed history.",
    );
}

#[test]
fn plain_text_passthrough() {
    snapshot(
        "plain_text",
        "Input with no graph chars — must passthrough unchanged.",
    );
}

#[test]
fn branching_graph() {
    snapshot(
        "branching",
        "Diverging branches — exercises tee + corner glyph mapping.",
    );
}

#[test]
fn merge_graph() {
    snapshot(
        "merge_graph",
        "Two-parent merge — covers tee_up and bottom-corner edges.",
    );
}

// --- Real-state fixtures: bookmarks, conflicts, hidden, divergent, workspaces,
// --- megamerges, and combinations.

#[test]
fn bookmarks() {
    snapshot("bookmarks", "jj log with bookmarks attached to commits.");
}

#[test]
fn conflicted() {
    snapshot(
        "conflicted",
        "Conflicted commit — × glyph with bold red color preserved.",
    );
}

#[test]
fn hidden_revisions() {
    snapshot("hidden", "Hidden (abandoned) revisions in the log.");
}

#[test]
fn divergent() {
    snapshot(
        "divergent",
        "Two visible commits sharing a change_id (divergent state).",
    );
}

#[test]
fn workspaces() {
    snapshot(
        "workspaces",
        "Multi-workspace log: multiple @ markers across workspaces.",
    );
}

#[test]
fn megamerge() {
    snapshot(
        "megamerge",
        "Octopus merge with many parents converging at one node.",
    );
}

#[test]
fn combo_conflicted_working_copy() {
    snapshot(
        "combo_conflicted_wc",
        "Working copy on a conflicted commit — × node and bold-red color combo.",
    );
}

#[test]
fn combo_megamerge_conflict() {
    snapshot(
        "combo_megamerge_conflict",
        "Megamerge where one parent is conflicted.",
    );
}

#[test]
fn combo_kitchen_sink() {
    snapshot(
        "combo_kitchen_sink",
        "Single capture exercising every node type: bookmarks, hidden, divergent, conflicted WC, multi-workspace, octopus megamerge, immutable root.",
    );
}

#[test]
fn diff_authors() {
    snapshot(
        "diff_authors",
        "Author column variety: single-name, multi-word, CJK, emoji — exercises jj's truncate/pad on a variable-width column.",
    );
}

#[test]
fn remote_bookmarks() {
    snapshot(
        "remote_bookmarks",
        "Local main ahead of origin: `main*` (locally-modified) and `main@origin` (remote-tracking) on different commits.",
    );
}

#[test]
fn compact_log() {
    snapshot(
        "compact_log",
        "Captured from `jj log -T builtin_log_compact` — two-line records per commit; continuation `│` lines have edges but no node, so dash filler must NOT appear on them.",
    );
}

#[test]
fn config_ascii_compact_log() {
    snapshot_with_config(
        "compact_log",
        "ascii",
        "config_ascii_compact_log",
        "compact_log with ascii.toml — ASCII fallbacks applied to the compact two-line layout.",
    );
}

// Synthetic fixtures: hand-crafted bytes that exercise specific paths
// without depending on a captured jj session.

#[test]
fn synthetic_only_marker_byte() {
    let input = b"\xf0\x9d\x99\xb4\n";
    snap_bytes(
        "synthetic_only_marker_byte",
        input,
        "Empty marker on its own line — non-graph line passes through verbatim.",
    );
}

#[test]
fn synthetic_immutable_diamond_no_color() {
    let input = "\u{25C6}  zzzzz root() 000000\n".as_bytes();
    snap_bytes(
        "synthetic_immutable_diamond_no_color",
        input,
        "◆ with no surrounding color — must darken and swap glyph to lock.",
    );
}

#[test]
fn synthetic_mutable_circle_no_color() {
    let input = "\u{25CB}  abcde 12345 desc\n".as_bytes();
    snap_bytes(
        "synthetic_mutable_circle_no_color",
        input,
        "○ alone, no color — must darken (override of jj's default fg).",
    );
}

#[test]
fn synthetic_wc_immutable_lock() {
    let input = "@  abc \u{1D678}desc\n".as_bytes();
    snap_bytes(
        "synthetic_wc_immutable_lock",
        input,
        "@ on a line carrying the immutable marker — lock icon wins over WC visuals.",
    );
}

#[test]
fn synthetic_wc_empty() {
    let input = "@  abc \u{1D674}desc\n".as_bytes();
    snap_bytes(
        "synthetic_wc_empty",
        input,
        "@ on an empty line — uses WC_EMPTY_ICON.",
    );
}

#[test]
fn synthetic_immutable_empty_diamond() {
    let input = "\u{25C6}  abc \u{1D674}\u{1D678}desc\n".as_bytes();
    snap_bytes(
        "synthetic_immutable_empty_diamond",
        input,
        "◆ + empty + immutable markers — uses EMPTY_IMMUTABLE_ICON.",
    );
}

#[test]
fn synthetic_conflict_node() {
    let input = "\u{00D7}  abc desc\n".as_bytes();
    snap_bytes(
        "synthetic_conflict_node",
        input,
        "× alone — conflict icon; fg color preserved (no darken).",
    );
}

#[test]
fn synthetic_box_drawing_only() {
    let input = "│ ├─╯\n".as_bytes();
    snap_bytes(
        "synthetic_box_drawing_only",
        input,
        "Pure graph segment with no node — must still emit edge-dim color.",
    );
}

#[test]
fn synthetic_alignment_dash_filler() {
    let input = b"\xe2\x94\x82 \xe2\x94\x82 \xe2\x97\x8b  abcde 12345\n\xe2\x97\x8b  fghij 67890\n";
    snap_bytes(
        "synthetic_alignment_dash_filler",
        input,
        "Tall + short graph rows — short row pads with dash filler when content begins with a change-id.",
    );
}

#[test]
fn synthetic_alignment_dash_filler_non_alphanumeric() {
    // Tall + short rows where the short row's content begins with `(` (an
    // annotation like `(elided revisions)`, `(empty)`, `(conflict)`). The
    // short row's graph segment is the elision `~` — no node char — so the
    // gap pads with plain spaces; dash filler only fires on rows with nodes.
    let input = "│ │ ○  abcde 12345\n~  (elided revisions)\n".as_bytes();
    snap_bytes(
        "synthetic_alignment_dash_filler_non_alphanumeric",
        input,
        "Tall + short rows; short row's graph is `~` (no node) — gap is plain spaces, not dashes.",
    );
}

#[test]
fn synthetic_marker_inside_csi_segment() {
    let input = b"@  abc \x1b[38;5;10m\xf0\x9d\x99\xb4\x1b[39mdesc\n";
    snap_bytes(
        "synthetic_marker_inside_csi_segment",
        input,
        "Marker bytes wrapped in jj's color envelope — marker stripped without extra spacing from the now-empty SGR pair.",
    );
}

// --- Degenerate inputs --------------------------------------------------
// These exercise bijjou's robustness against malformed / unexpected input
// that real jj output usually wouldn't produce. Behavior should be
// deterministic and never panic.

#[test]
fn degenerate_author_column_missing() {
    snap_bytes(
        "degenerate_author_column_missing",
        b"@  abcde 12345 260509\xc2\xb70958 desc\n",
        "Realistic format minus the author cell — boundary detection still works.",
    );
}

#[test]
fn degenerate_timestamp_missing() {
    snap_bytes(
        "degenerate_timestamp_missing",
        b"@  abcde 12345 ME desc-without-timestamp\n",
        "Header row without timestamp — no panic.",
    );
}

#[test]
fn degenerate_no_description() {
    snap_bytes(
        "degenerate_no_description",
        b"\xe2\x97\x8b  abcde 12345\n",
        "Bare graph + change/commit IDs only, no description text.",
    );
}

#[test]
fn degenerate_only_newlines() {
    snap_bytes(
        "degenerate_only_newlines",
        b"\n\n\n",
        "Three blank lines, no graph and no content.",
    );
}

#[test]
fn degenerate_only_csi_no_text() {
    snap_bytes(
        "degenerate_only_csi_no_text",
        b"\x1b[31m\x1b[39m\n",
        "Pure escape sequences, zero visible chars.",
    );
}

#[test]
fn degenerate_unterminated_csi() {
    snap_bytes(
        "degenerate_unterminated_csi",
        b"@  abc\x1b[38;5;",
        "CSI cut off without final byte — parser must consume to EOF without panic.",
    );
}

#[test]
fn degenerate_no_trailing_newline() {
    snap_bytes(
        "degenerate_no_trailing_newline",
        b"\xe2\x97\x8b  abcde 12345 desc",
        "Final line missing trailing \\n — output must still flush correctly.",
    );
}

#[test]
fn degenerate_many_consecutive_markers() {
    let mut buf = b"@  abc ".to_vec();
    for _ in 0..4 {
        buf.extend_from_slice(b"\xf0\x9d\x99\xb4"); // 𝙴
        buf.extend_from_slice(b"\xf0\x9d\x99\xb8"); // 𝙸
    }
    buf.extend_from_slice(b"desc\n");
    snap_bytes(
        "degenerate_many_consecutive_markers",
        &buf,
        "Stack of 𝙴𝙸 markers — all stripped, no doubled spacing.",
    );
}

#[test]
fn degenerate_marker_at_line_start() {
    snap_bytes(
        "degenerate_marker_at_line_start",
        b"\xf0\x9d\x99\xb4@  abcde desc\n",
        "Empty marker before the @ glyph — must strip cleanly.",
    );
}

#[test]
fn degenerate_invalid_utf8_byte() {
    snap_bytes(
        "degenerate_invalid_utf8_byte",
        b"@  abcde \x80 strange desc\n",
        "Stray 0x80 continuation byte — UTF-8 decoder treats as 1-byte char, no crash.",
    );
}

#[test]
fn degenerate_tab_in_content() {
    snap_bytes(
        "degenerate_tab_in_content",
        b"@  abcde 12345 ME 260509 desc\twith\ttabs\n",
        "Tabs inside the description column — pass through unchanged.",
    );
}

#[test]
fn degenerate_blank_graph_line_between() {
    snap_bytes(
        "degenerate_blank_graph_line_between",
        b"\xe2\x97\x8b  abcde 12345\n\n\xe2\x97\x8b  fghij 67890\n",
        "Mid-graph blank line — must not crash or merge into next row.",
    );
}

#[test]
fn degenerate_graph_only_no_content() {
    snap_bytes(
        "degenerate_graph_only_no_content",
        b"\xe2\x94\x82\n\xe2\x94\x9c\xe2\x94\x80\xe2\x95\xaf\n",
        "Lines with graph chars but no commit content after.",
    );
}

#[test]
fn degenerate_long_line_no_graph() {
    let body: Vec<u8> = std::iter::repeat(b'x').take(500).collect();
    let mut buf = body.clone();
    buf.push(b'\n');
    snap_bytes(
        "degenerate_long_line_no_graph",
        &buf,
        "500-byte plain text line — emit_dim_graph wraps each char individually; checks scaling.",
    );
}

#[test]
fn degenerate_csi_then_newline() {
    snap_bytes(
        "degenerate_csi_then_newline",
        b"\x1b[1m\n",
        "CSI immediately followed by newline; no visible content.",
    );
}

#[test]
fn degenerate_solo_marker_byte_no_newline() {
    snap_bytes(
        "degenerate_solo_marker_byte_no_newline",
        b"\xf0\x9d\x99\xb4",
        "Empty marker as the entire input, no newline — non-graph line passes through verbatim.",
    );
}

#[test]
fn degenerate_marker_after_graph_only() {
    snap_bytes(
        "degenerate_marker_after_graph_only",
        b"\xe2\x97\x8b  \xf0\x9d\x99\xb4\n",
        "Graph + marker but no commit content; marker still stripped.",
    );
}

// --- Activation gate ----------------------------------------------------
// Default config gates processing on presence of the activation marker
// (the ASCII sentinel `BIJJOU_ACTIVATE`). Absent → byte-identical passthrough.
// Present → process and strip every occurrence of the marker.

const ACTIVATION_MARKER: &[u8] = b"BIJJOU_ACTIVATE";

#[test]
fn gate_passthrough_when_marker_absent() {
    let input = b"\xe2\x97\x8b  abcde 12345 description\n";
    let output = run_bijjou_default(input);
    assert_eq!(
        output, input,
        "input without BIJJOU_ACTIVATE must passthrough unchanged"
    );
}

#[test]
fn gate_processes_when_marker_present() {
    // Marker on a graph line triggers processing; the graph line gets rewritten.
    let mut input = "\u{25CB}  abcde 12345 ".as_bytes().to_vec();
    input.extend_from_slice(ACTIVATION_MARKER);
    input.extend_from_slice(b"description\n");
    let output = run_bijjou_default(&input);
    assert_ne!(
        output, input,
        "input with BIJJOU_ACTIVATE on a graph line must be processed"
    );
}

#[test]
fn gate_passes_through_non_graph_lines_even_when_marker_present() {
    // Marker present in input → gate opens. But lines without graph chars
    // (plain text, status output, anything that isn't the log graph) must pass
    // through byte-for-byte unchanged, including any marker bytes on them.
    let mut input = ACTIVATION_MARKER.to_vec();
    input.extend_from_slice(b" plain text line\n");
    input.extend_from_slice(b"M src/main.rs\n");
    input.extend_from_slice(b"\xe2\x97\x8b  abcde 12345 description\n");
    let output = run_bijjou_default(&input);
    let marker_line = {
        let mut v = ACTIVATION_MARKER.to_vec();
        v.extend_from_slice(b" plain text line\n");
        v
    };
    assert!(
        output.windows(marker_line.len()).any(|w| w == marker_line),
        "non-graph line containing marker must passthrough verbatim"
    );
    assert!(
        output.windows(b"M src/main.rs\n".len()).any(|w| w == b"M src/main.rs\n"),
        "plain-text status line must passthrough verbatim"
    );
}

#[test]
fn gate_strips_inline_marker_in_content() {
    // Marker embedded inside a description should be stripped.
    let mut input = "\u{25CB}  abcde 12345 desc ".as_bytes().to_vec();
    input.extend_from_slice(ACTIVATION_MARKER);
    input.extend_from_slice(b" suffix\n");
    let output = run_bijjou_default(&input);
    assert!(
        !output
            .windows(ACTIVATION_MARKER.len())
            .any(|w| w == ACTIVATION_MARKER),
        "inline activation marker must be stripped"
    );
}

// --- Custom-config fixtures --------------------------------------------
// Each pipes a stock input through bijjou with a non-default config from
// tests/fixtures/configs/*.toml. The snapshot captures the configured
// glyphs/colors so regressions in config plumbing show up here.

#[test]
fn config_ascii_linear_chain() {
    snapshot_with_config(
        "linear_chain",
        "ascii",
        "config_ascii_linear_chain",
        "Linear chain rendered with ascii.toml — verifies icon and dash overrides on a simple shape.",
    );
}

#[test]
fn config_ascii_branching() {
    snapshot_with_config(
        "branching",
        "ascii",
        "config_ascii_branching",
        "Branching shape with ascii.toml — exercises ASCII fallback for graph chars and tees.",
    );
}

#[test]
fn config_ascii_workspaces() {
    snapshot_with_config(
        "workspaces",
        "ascii",
        "config_ascii_workspaces",
        "Workspaces (state-rich) input with ascii.toml — full ASCII fallback across all node types.",
    );
}

#[test]
fn config_hex_colors_workspaces() {
    snapshot_with_config(
        "workspaces",
        "hex_colors",
        "config_hex_colors_workspaces",
        "workspaces with hex_colors.toml — exercises the #rrggbb path emitting 24-bit truecolor SGR (38;2;r;g;b).",
    );
}

#[test]
fn config_dash_only_workspaces() {
    snapshot_with_config(
        "workspaces",
        "dash_only",
        "config_dash_only_workspaces",
        "workspaces with dash_only.toml — only the alignment dash glyph is overridden; other defaults remain.",
    );
}

#[test]
fn config_hide_vertical_only_single_wc() {
    snapshot_with_config(
        "single_wc",
        "hide_vertical",
        "config_hide_vertical_only_single_wc",
        "single_wc with hide_vertical.toml — the lone `│` filler line above the elision marker must be dropped.",
    );
}

#[test]
fn config_hide_vertical_only_megamerge_conflict() {
    snapshot_with_config(
        "combo_megamerge_conflict",
        "hide_vertical",
        "config_hide_vertical_only_megamerge_conflict",
        "combo_megamerge_conflict with hide_vertical.toml — vertical-only filler row between nodes is dropped; rows with corners/tees stay.",
    );
}

// --- Streaming mode -----------------------------------------------------
// Streaming flushes output in batches; graph-column width is updated
// per-line as wider graph columns are seen and only grows monotonically.
// The widening takes effect starting at the exact line whose graph_col
// exceeds the running max, not at the batch boundary. (This differs from
// batch mode, which applies the global max uniformly to every line.)

#[test]
fn stream_per_line_widening_within_single_batch() {
    // All 5 lines fit in one batch (batch-size = 1000000). Widening must
    // still happen line-by-line: lines 1-2 emit at the narrow target, line
    // 3 onward at the wider one — even though they share one batch and one
    // flush.
    let input = b"\
\xe2\x97\x8b  a\n\
\xe2\x97\x8b  b\n\
\xe2\x94\x82 \xe2\x97\x8b  c\n\
\xe2\x94\x82 \xe2\x97\x8b  d\n\
\xe2\x94\x82 \xe2\x97\x8b  e\n";
    let output = run_bijjou_with_config(input, "stream-huge");
    insta::with_settings!({description => "Widening within a single batch: lines 1-2 emit at narrow target, lines 3-5 at the wider one."}, {
        insta::assert_snapshot!("stream_per_line_widening_within_single_batch", visualize(&output));
    });
}

#[test]
fn stream_monotonic_widening() {
    // First batch (2 lines) has graph_col=1, second batch widens to graph_col=3.
    // Expected: first batch padded to col 3 (1+2), second to col 5 (3+2).
    let input = b"\
\xe2\x97\x8b  a\n\
\xe2\x97\x8b  b\n\
\xe2\x94\x82 \xe2\x97\x8b  c\n\
\xe2\x94\x82 \xe2\x97\x8b  d\n";
    let output = run_bijjou_with_config(input, "stream-batch-2-always");
    insta::with_settings!({description => "Two batches; second widens. First batch emitted at col=3, second at col=5."}, {
        insta::assert_snapshot!("stream_monotonic_widening", visualize(&output));
    });
}

#[test]
fn stream_monotonic_no_shrink() {
    // First batch (2 lines) has graph_col=3, second batch narrows to graph_col=1.
    // Running max sticks at 3, so second batch is still emitted at col=5.
    let input = b"\
\xe2\x94\x82 \xe2\x97\x8b  a\n\
\xe2\x94\x82 \xe2\x97\x8b  b\n\
\xe2\x97\x8b  c\n\
\xe2\x97\x8b  d\n";
    let output = run_bijjou_with_config(input, "stream-batch-2-always");
    insta::with_settings!({description => "Two batches; second narrows. Running max sticks at 3, so both batches emit at col=5."}, {
        insta::assert_snapshot!("stream_monotonic_no_shrink", visualize(&output));
    });
}

#[test]
fn stream_auto_marker_in_first_batch_activates() {
    // Marker is on line 2, within the first batch (size 2). Auto must activate
    // and process the input — graph chars get rewritten.
    let mut input = b"\xe2\x97\x8b  a\n".to_vec();
    input.extend_from_slice(b"\xe2\x97\x8b  b ");
    input.extend_from_slice(ACTIVATION_MARKER);
    input.extend_from_slice(b"\n\xe2\x97\x8b  c\n\xe2\x97\x8b  d\n");
    let output = run_bijjou_with_config(&input, "stream-batch-2-auto");
    assert_ne!(
        output, input,
        "marker in first batch must activate streaming auto mode"
    );
    assert!(
        !output.windows(ACTIVATION_MARKER.len()).any(|w| w == ACTIVATION_MARKER),
        "marker must be stripped from output once activated"
    );
}

#[test]
fn stream_auto_marker_after_first_batch_passes_through() {
    // Marker is on line 4 (third batch); first batch is 2 lines with no marker.
    // Streaming auto must give up and pass-through verbatim.
    let mut input = b"\xe2\x97\x8b  a\n".to_vec();
    input.extend_from_slice(b"\xe2\x97\x8b  b\n");
    input.extend_from_slice(b"\xe2\x97\x8b  c\n");
    input.extend_from_slice(b"\xe2\x97\x8b  d ");
    input.extend_from_slice(ACTIVATION_MARKER);
    input.extend_from_slice(b"\n");
    let output = run_bijjou_with_config(&input, "stream-batch-2-auto");
    assert_eq!(
        output, input,
        "marker missing from first batch must result in byte-identical passthrough"
    );
}

#[test]
fn stream_passthrough_when_activate_never() {
    // Even with streaming enabled, Activate::Never short-circuits to pure
    // byte-for-byte passthrough — no batching, no rendering.
    let input = b"\xe2\x97\x8b  abcde 12345 description\n";
    let toml = "activate = \"never\"\n[stream]\nenabled = true\nbatch-size = 2\n";
    let tmp = std::env::temp_dir().join("bijjou-stream-never.toml");
    std::fs::write(&tmp, toml).unwrap();
    let output = Command::cargo_bin("bijjou")
        .expect("binary built")
        .env("BIJJOU_CONFIG", &tmp)
        .write_stdin(&input[..])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let _ = std::fs::remove_file(&tmp);
    assert_eq!(output, input, "Activate::Never must passthrough regardless of streaming");
}

#[test]
fn config_alt_nodes_kitchen_sink() {
    snapshot_with_config(
        "combo_kitchen_sink",
        "alt_nodes",
        "config_alt_nodes_kitchen_sink",
        "combo_kitchen_sink with alt_nodes.toml swapping every node icon to BMP fallbacks (★ ◉ ⬢ ⊗ ✦ ○ ☆ ⬡).",
    );
}
