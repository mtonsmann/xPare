# Performance

xPare treats performance as a product requirement **after** safety and
privacy. Clipboard transforms should feel instant for ordinary text, and large log
pastes should stay bounded, predictable, and measurable — never at the cost of a
guardrail. The full method (ceiling model, optimization waves, acceptance rules)
lives in
[`docs/exec-plans/active/0002-performance-ceiling-and-optimization-loop.md`](exec-plans/active/0002-performance-ceiling-and-optimization-loop.md).

## What we measure (three layers)

| Layer | Command | What it tells you |
|-------|---------|-------------------|
| Complexity gate | `cargo test -p xpare-core --test perf_guard` (runs in `make ci`) | Linear-time behavior holds; an O(n²)/DoS regression is caught. Always-on, cannot flake. |
| Statistical benches | `make bench`, `make bench-large` | criterion MiB/s with confidence intervals + outlier detection — the authoritative measurement. |
| Throughput baseline | `make perf PERF_MIB=128 PERF_SAMPLES=7` | A quick roofline-calibrated MiB/s table for a same-machine regression read; optional hard floor. |

All three use **synthetic, generated buffers**. None reads, logs, or persists real
clipboard content.

## How to run

```sh
# Quick same-machine baseline (release build, ~a minute or two):
make perf PERF_MIB=128 PERF_SAMPLES=7

# Optional hard floor on a calibrated machine (fails if an end-to-end scenario
# drops below the floor):
make perf PERF_MIB=128 PERF_SAMPLES=7 PERF_MIN_MIB_PER_SEC=90

# Statistical detail:
make bench           # clipboard-scale
make bench-large     # heavy log files up to 256 MB
```

## Local baseline

Measured 2026-06-06 on **Apple M5 Pro, 18 cores, 48 GB, arm64**, via
`make perf PERF_MIB=128 PERF_SAMPLES=5` (median of 5), on the current code **with
pipeline intermediate zeroization** and the W1 byte-oriented
`collapse_whitespace` path, W1c marker-free HTML text path, W1e guarded ASCII
plain/log Markdown fast path, W4 ASCII Upper/Lower fast paths, and W5b IOC marker
dispatch plus W5c pre-sized line dedupe containers and W5d/W5f/W5g defang
allocation/marker guard cleanup, streaming token reconstruction, and no-op token
prefiltering, W5j refang literal-span copying, W2 output pre-sizing for shared line
joins, and W4b streaming sentence-case scanning, W2b borrowed-slice trailing trim,
W1b Markdown output bookkeeping/normalization cleanup, and W5e streaming URL
cleaner token reconstruction plus W5h/W5i URL no-op token prefiltering and
tracker-key dispatch, W7 speed-tuned release optimization, W1d borrowed first-pass
pipeline input, W2c streaming `unwrap_lines`, plus W3
`TrimTrailingWhitespace` → `RemoveBlankLines` fusion and W3b `CollapseWhitespace` →
`TrimTrailingWhitespace` → `RemoveBlankLines` fusion with boundary-zeroized scratch
and W3c borrowed-line fast path for already-collapse-normalized lines (see the cost
section below), plus W3d `TrimTrailingWhitespace` → `RemoveBlankLines` →
`DedupeLines` fusion, W3e guarded `CollapseWhitespace` →
`TrimTrailingWhitespace` → `RemoveBlankLines` → `DedupeLines` fusion, and W3f
guarded ASCII plain/log `StripHtml` → `StripMarkdown` boundary fusion.
Re-measure on each machine; do not assume another machine's numbers. Read each
transform row relative to this machine's own roofline controls (byte-copy is noisy at
this size and was ≈ 36 GiB/s in this run; byte-scan is vectorized under the
speed-tuned release profile and can exceed the copy control because it does less
write traffic).

| Scenario | Median | Throughput |
|----------|-------:|-----------:|
| roofline-byte-scan | 0.003s | 45522.6 MiB/s |
| roofline-byte-copy | 0.003s | 36605.9 MiB/s |
| strip-html-plain (no `<`/`&`) | 0.042s | 3045.1 MiB/s |
| strip-html-heavy | 0.235s | 543.8 MiB/s |
| strip-html-sparse-log | 0.049s | 2596.7 MiB/s |
| strip-markdown-heavy | 0.823s | 155.6 MiB/s |
| strip-markdown-sparse-log | 0.115s | 1114.2 MiB/s |
| collapse-whitespace | 0.125s | 1025.3 MiB/s |
| trim-trailing | 0.100s | 1285.9 MiB/s |
| remove-blank-lines | 0.050s | 2555.9 MiB/s |
| unwrap-lines | 0.046s | 2775.0 MiB/s |
| case-lower-ascii | 0.010s | 12959.7 MiB/s |
| case-sentence-unicode | 0.410s | 312.1 MiB/s |
| dedupe-lines-repeated | 0.078s | 1637.9 MiB/s |
| dedupe-lines-unique | 0.082s | 1570.5 MiB/s |
| sort-lines | 0.131s | 977.7 MiB/s |
| defang-iocs (URLs/emails/IPs/domains; output grows ~15%) | 0.503s | 254.6 MiB/s |
| refang-iocs (input is the defanged buffer) | 0.112s | 1315.6 MiB/s |
| clean-urls-trackers | 0.234s | 546.5 MiB/s |
| html-markdown-trim-log | 0.240s | 533.5 MiB/s |
| full-menu-without-markdown | 0.281s | 456.0 MiB/s |
| full-menu-without-collapse | 0.307s | 417.2 MiB/s |
| full-menu-without-dedupe | 0.502s | 254.9 MiB/s |
| full-menu-without-case | 0.350s | 365.8 MiB/s |
| **default-log** (html+md+collapse+trim+blank) | 0.323s | **396.7 MiB/s** |
| **full-menu-log** (+dedupe+unwrap+lowercase) | 0.349s | **366.4 MiB/s** |
| **lossy-utf8-log** (invalid UTF-8, default pipeline) | 0.510s | **251.3 MiB/s** |

Slow lanes (optimization targets): the remaining slow single-op cluster is heavy
Markdown stripping and defang. Marker-free HTML is no longer in that slow cluster
after W1c's guarded plain-text path, and sparse/log-like Markdown is no longer there
after W1b's suffix-based newline bookkeeping/in-place edge trim and W1e's guarded
ASCII plain/log fast path, but heavy Markdown still pays parser/event cost. Defang still
emits multi-character bracket
markers around every indicator character and grows output ~15%, but W5d/W5f/W5g
removed avoidable token-level allocation and no-op classification overhead. Refang
now copies literal spans between marker-trigger bytes instead of paying the fallback
path one character at a time. Unicode sentence-case is no longer in the same slow
cluster after W4b's streaming scanner, which avoids the temporary lowercase buffer
and per-character uppercase allocations while preserving Unicode expansion. Clean URL
stripping now streams URL token
reconstruction directly into
the final output, so it avoids per-token temporary strings and an intermediate
survivor list, and skips trim/prefix work for prose tokens that cannot expose a
URL prefix after punctuation trimming. Tracker-key checks dispatch by first byte so
kept functional query keys do not scan the full tracker table. End-to-end clipboard
pipelines (which don't include the IOC ops) sit at ~251–397 MiB/s in this run. The
release profile is now speed-tuned (`opt-level = 3`), which materially improves the
parser, byte-scanner, line-op, and end-to-end rows at the cost of no longer choosing
the smallest release artifacts by default. The pipeline now borrows caller-owned
input for the first pass and only wraps operation outputs that feed later passes in
`Zeroizing`, eliminating one unnecessary xPare-owned full-buffer duplicate
while preserving intermediate wiping. The W3 fusions remove the trim/remove-blank
intermediate and the common collapse/trim/remove suffix from the default path. The
W3b fused collapse scratch is transform-local `Zeroizing` storage: it is wiped before
capacity growth can release old bytes and on drop, but allocation-preserving reuse
does not zeroize on every line. W3f skips the marker-free HTML intermediate before
Markdown for the same strict ASCII plain/log subset. That preserves the
wipe-before-release posture without giving back the raw W3b speedup, so the
decomposition rows now point to heavy Markdown parsing, IOC transforms, and the
full-menu dedupe/unwrap/lowercase tail before marker-free HTML or basic line cleanup.
(For reference,
the upstream FormatStripper track reported ~177/131 MiB/s default/full-menu on an
Apple M4 — a different machine, codebase, and zeroization posture, so not a
like-for-like comparison.)

## Cost of intermediate zeroization

The core holds every operation output that becomes a pipeline intermediate in a
`Zeroizing` buffer so clipboard secrets are wiped from the heap after use (see
`core/src/pipeline.rs` and [`SECURITY.md`](../SECURITY.md)). The caller-owned input is
borrowed for the first pass, and the final output is returned without an extra copy,
so single-op scenarios avoid pipeline-intermediate wipes entirely. For multi-op
pipelines, intermediate wipes cost memory bandwidth. Measured on the same machine,
128 MiB, before vs. after enabling intermediate zeroization:

| Scenario | Before (no zeroize) | After (shipped) | Δ |
|----------|--------------------:|----------------:|---:|
| default-log | 182.3 MiB/s | 125.0 MiB/s | −31% |
| full-menu-log | 162.7 MiB/s | 108.3 MiB/s | −33% |
| lossy-utf8-log | 175.9 MiB/s | 122.1 MiB/s | −31% |

**This cost is material only on very large pastes (100+ MiB).** At clipboard scale
(sub-MiB, the overwhelmingly common case) the absolute transform time is
microseconds either way, so the wipe is imperceptible. The trade — a third of
throughput on huge log pastes in exchange for not leaving plaintext clipboard
content in freed heap pages — is deliberate. If a deployment is throughput-bound on
large inputs and accepts the weaker hygiene, reverting `pipeline.rs` to a plain
`String` fold (keeping only the FFI-output zeroization) recovers it.

Transform-local scratch storage is handled by a narrower rule. A scratch allocation
that never leaves the transform is wiped before capacity growth could free old
storage and again on drop; it is not wiped on every logical reuse. This keeps the
security property aligned with heap lifetime rather than adding hot-path wipes that
do not improve the stated threat model.

## Interpreting the numbers

- For ordinary clipboard content (< 1 MiB) every scenario is far below the point a
  user notices. Large multi-MiB log pastes are bounded and predictable.
- The core pipeline is multi-pass: line-ending handling and each operation allocate
  and copy; intermediates are then wiped. The practical ceiling is well under memory
  bandwidth because of scalar parsing, UTF-8 decoding, branching, and allocation.
- Read each transform row relative to the machine's own `roofline-byte-scan` /
  `roofline-byte-copy` controls, not against the raw bandwidth number.

## Product targets

- Text under 1 MiB: feels instant.
- Normal multi-MiB document/log copies: well under perceptual friction.
- The macOS shell should move large transforms off the main actor so the menu-bar
  UI stays responsive regardless of transform time (Wave 6 in the exec-plan).

## Optimization backlog

See the exec-plan's wave list. Highest-confidence remaining items: stream the
remaining `collect`→`join` line ops (W2), fuse compatible adjacent passes (W3), and
additional ASCII fast paths with Unicode fallbacks where semantics allow (W4). The
speed-tuned `opt-level = 3` release profile and first-pass pipeline borrowing are
already banked. Each change must clear ≥ 5% median gain with no > 3% regression and
all guardrails green.
