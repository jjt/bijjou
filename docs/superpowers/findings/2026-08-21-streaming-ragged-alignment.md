# Streaming renders ragged columns on any log that fans out

Date: 2026-08-21
Status: Open (bug, unassigned)
Area: `src/stream.rs`, `src/main.rs::accumulate_metrics`

## Problem

In streaming mode (the default), content right of the graph does not line up.
The alignment column steps rightward each time a deeper graph row arrives, so a
single log renders at several different columns.

Observed on a real repo, `jj log -T log_oneline -r 'all()' | bijjou`
(3758 lines, `batch-size = "half-pager"`). Column index of `change_id` after
stripping SGR:

| first line at column | column |
| -------------------- | ------ |
| 0                    | 10     |
| 89                   | 14     |
| 100                  | 18     |
| later                | 22     |

Distribution across the log: `{22: 1079, 18: 581, 10: 65, 14: 8, 19: 3, 23: 1}`.

Same input with `--stream=false`: `{22: 3032}`, i.e. one column throughout.
(The handful of rows that measure one wider are 7-char change_ids from
`format_short_change_id_with_change_offset` disambiguation, not misalignment.)

This is not confined to `graph_col`. The elastic-tab anchors in
`TemplateMetrics::anchors` widen on the same schedule, so inter-field gaps
shift too.

## Repro

No jj repo needed. Four shallow rows, then four rows at graph depth 4, with the
batch boundary between them:

```sh
python3 - <<'PY' > /tmp/fanout.txt
def row(depth, ch, cid, d):
    return ("\u2502 " * depth + "\u25cb  bijjou_template_name\x00log_oneline"
            f"\x00change_id\x00{ch}\x00commit_id\x00{cid}\x00author\x00ME"
            f"\x00timestamp\x00260821\u00b70900\x00description\x00{d}\x1e")
rows  = [row(0, "aaaaaa", "111111", "shallow") for _ in range(4)]
rows += [row(4, "bbbbbb", "222222", "deep")    for _ in range(4)]
print("\n".join(rows))
PY

BIJJOU_CONFIG=bijjou-config.toml bijjou --activate=always --color=never --stream__batch-size=4 < /tmp/fanout.txt
BIJJOU_CONFIG=bijjou-config.toml bijjou --activate=always --color=never --stream=false            < /tmp/fanout.txt
```

Streaming — first four rows pinned at column 2, no dash run:

```
○ aaaaaa 111111 ME 260821·0900 shallow
○ aaaaaa 111111 ME 260821·0900 shallow
○ aaaaaa 111111 ME 260821·0900 shallow
○ aaaaaa 111111 ME 260821·0900 shallow
𜸩 𜸩 𜸩 𜸩 ○ bbbbbb 222222 ME 260821·0900 deep
```

Buffered — every row at column 10:

```
○╶───────╴aaaaaa 111111 ME 260821·0900 shallow
○╶───────╴aaaaaa 111111 ME 260821·0900 shallow
○╶───────╴aaaaaa 111111 ME 260821·0900 shallow
○╶───────╴aaaaaa 111111 ME 260821·0900 shallow
𜸩 𜸩 𜸩 𜸩 ○ bbbbbb 222222 ME 260821·0900 deep
```

## Root cause

`src/stream.rs::run` reads one batch, then loops. `process_batch`
(`stream.rs:136`) calls `accumulate_metrics` on the batch it is about to emit
and immediately emits it, so metrics only ever reflect rows seen so far.
`accumulate_metrics` (`main.rs:116`) widens `max_graph_col` and `anchors`
monotonically — correct, since rows already flushed cannot be repainted — but
that means the target column is a running maximum, not a global one.

The structural defect is in `resolve_batch_sizes` (`stream.rs:55`) returning
`(first_size, rest_size)`, where `first_size` is overloaded with three
unrelated jobs:

1. the activation-scan window (`stream.rs:28-42`),
2. the metrics prescan window,
3. the first flush unit.

Flush cadence is a latency knob. Prescan depth is a correctness knob. They are
tied together, so asking for low-latency first paint (`half-pager`) silently
buys a tiny prescan window and therefore ragged output. With
`batch-size = "half-pager"` on a 50-row terminal the prescan is 49 lines — the
fan-out in a real jj log is essentially always past that.

`ARCHITECTURE.md:22-27` and the `[stream]` comment block in
`bijjou-config.toml` document the divergence as intentional. It is defensible
as a description of the mechanism, but the resulting output is wrong on the one
thing bijjou exists to do, and there is no way to get correct columns *and*
streaming today.

## Fix

Split the two knobs. Add `[stream].prescan` (line count, default something like
2048, or `"all"`):

- Read up to `prescan` lines up front. Run `accumulate_metrics` over all of
  them before emitting anything.
- Then flush in `batch-size` chunks, from the already-read prescan buffer
  first, then straight from the reader.
- Beyond `prescan`, fall back to today's monotonic widening.

Properties:

- Any log ≤ `prescan` rows is byte-identical to buffered output. That covers
  effectively every interactive `jj log`, including `-r 'all()'` on a
  3758-line repo.
- Latency cost is bounded by `prescan` lines of read, not by EOF.
- Memory cost is bounded: ~2048 rows × ~200 B ≈ 400 KB.
- Bonus: the `Activate::Auto` scan at `stream.rs:28-42` currently gives up
  after the first batch. Widening its window to `prescan` makes auto-detection
  correspondingly more reliable.

Also reconsider the default. `[stream].enabled = true` optimizes for unbounded
input, but the overwhelmingly common case is a finite `jj log` that renders in
milliseconds. Flipping the default to `false` (streaming opt-in for genuinely
unbounded producers) is the one-line version of this fix, and is what the
reporter has applied locally as a workaround.

### Rejected alternatives

- **Repaint already-flushed rows with cursor-up.** Impossible once output has
  gone into a pager, which is the primary use case.
- **`reserve-graph-col = N` floor config.** Guesswork; wrong whenever the guess
  is wrong, and the user cannot know the right value in advance.
- **Shrink alignment retroactively.** Violates the monotonic-widening invariant
  that keeps already-emitted rows valid.

## Acceptance

- Golden test with a fan-out fixture (shallow rows, then deep rows, boundary
  inside the fixture) asserting streaming and buffered output are byte-identical
  when the fixture fits in `prescan`.
- Golden test with `prescan` forced below the fan-out point, asserting the
  documented monotonic-widening fallback still holds (columns grow, never
  shrink).
- `ARCHITECTURE.md:22-27` and the `[stream]` comment block in
  `bijjou-config.toml` updated: the modes no longer differ for input within
  `prescan`.
