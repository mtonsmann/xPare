# Performance

SafetyStrip treats performance as a product requirement **after** safety and
privacy. Clipboard transforms should feel instant for ordinary text, and large log
pastes should stay bounded, predictable, and measurable — never at the cost of a
guardrail. The full method (ceiling model, optimization waves, acceptance rules)
lives in
[`docs/exec-plans/active/0002-performance-ceiling-and-optimization-loop.md`](exec-plans/active/0002-performance-ceiling-and-optimization-loop.md).

## What we measure (three layers)

| Layer | Command | What it tells you |
|-------|---------|-------------------|
| Complexity gate | `cargo test -p safetystrip-core --test perf_guard` (runs in `make ci`) | Linear-time behavior holds; an O(n²)/DoS regression is caught. Always-on, cannot flake. |
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
`collapse_whitespace` path, W1c marker-free HTML text path, W4 ASCII Upper/Lower
fast paths, and W5b IOC marker dispatch plus W5c pre-sized line dedupe containers
and W5d defang allocation/marker guard cleanup, W2 output pre-sizing for shared line
joins, and W4b streaming sentence-case scanning, W2b borrowed-slice trailing trim,
and W1b Markdown output bookkeeping/normalization cleanup, plus W3
`TrimTrailingWhitespace` → `RemoveBlankLines` fusion and W3b `CollapseWhitespace` →
`TrimTrailingWhitespace` → `RemoveBlankLines` fusion with boundary-zeroized scratch
(see the cost section below).
Re-measure on each machine; do not assume another machine's numbers. Read each
transform row relative to this machine's own roofline controls (byte-copy ≈ 44 GiB/s
in this run is the practical memory-traffic anchor, though it is noisy at this size;
byte-scan is lower because the shipped release profile is `opt-level = "s"` —
size-optimized — leaving the scalar scan loop unvectorized).

| Scenario | Median | Throughput |
|----------|-------:|-----------:|
| roofline-byte-scan | 0.032s | 4027.1 MiB/s |
| roofline-byte-copy | 0.003s | 44687.6 MiB/s |
| strip-html-plain (no `<`/`&`) | 0.133s | 964.9 MiB/s |
| strip-html-heavy | 0.373s | 343.0 MiB/s |
| strip-html-sparse-log | 0.147s | 872.9 MiB/s |
| strip-markdown-heavy | 0.895s | 143.0 MiB/s |
| strip-markdown-sparse-log | 0.213s | 601.7 MiB/s |
| collapse-whitespace | 0.193s | 664.2 MiB/s |
| trim-trailing | 0.219s | 584.0 MiB/s |
| remove-blank-lines | 0.145s | 884.0 MiB/s |
| unwrap-lines | 0.179s | 714.8 MiB/s |
| case-lower-ascii | 0.106s | 1208.0 MiB/s |
| case-sentence-unicode | 0.528s | 242.5 MiB/s |
| dedupe-lines-repeated | 0.166s | 770.1 MiB/s |
| dedupe-lines-unique | 0.171s | 749.1 MiB/s |
| sort-lines | 0.212s | 602.5 MiB/s |
| defang-iocs (URLs/emails/IPs/domains; output grows ~15%) | 0.975s | 131.3 MiB/s |
| refang-iocs (input is the defanged buffer) | 0.388s | 381.2 MiB/s |
| clean-urls-trackers | 0.465s | 275.5 MiB/s |
| html-markdown-trim-log | 0.567s | 225.9 MiB/s |
| full-menu-without-markdown | 0.558s | 229.2 MiB/s |
| full-menu-without-collapse | 0.715s | 179.1 MiB/s |
| full-menu-without-dedupe | 0.883s | 144.9 MiB/s |
| full-menu-without-case | 0.794s | 161.2 MiB/s |
| **default-log** (html+md+collapse+trim+blank) | 0.646s | **198.0 MiB/s** |
| **full-menu-log** (+dedupe+unwrap+lowercase) | 0.799s | **160.2 MiB/s** |
| **lossy-utf8-log** (invalid UTF-8, default pipeline) | 0.655s | **195.8 MiB/s** |

Slow lanes (optimization targets): the remaining slow single-op cluster is heavy
Markdown stripping and defang. Marker-free HTML is no longer in that slow cluster
after W1c's guarded plain-text path, and sparse/log-like Markdown is no longer there
after W1b's suffix-based newline bookkeeping and in-place edge trim, but heavy
Markdown still pays parser/event cost. Defang still emits multi-character bracket
markers around every indicator character and grows output ~15%, but W5d removed
much of the avoidable token-level overhead. Unicode sentence-case is no longer in
the same slow cluster after W4b's streaming scanner, which avoids the temporary
lowercase buffer and per-character uppercase allocations while preserving Unicode
expansion. End-to-end clipboard pipelines (which don't include the IOC ops) sit at
~160–198 MiB/s in this run. The W3 fusions remove the trim/remove-blank
intermediate and the common collapse/trim/remove suffix from the default path. The
W3b fused collapse scratch is transform-local `Zeroizing` storage: it is wiped
before capacity growth can release old bytes and on drop, but allocation-preserving
reuse does not zeroize on every line. That preserves the wipe-before-release
posture without giving back the raw W3b speedup, so the decomposition rows now
point to Markdown parsing and the full-menu dedupe/unwrap/lowercase tail before
marker-free HTML or basic line cleanup.
(For reference,
the upstream FormatStripper track reported ~177/131 MiB/s default/full-menu on an
Apple M4 — a different machine, codebase, and zeroization posture, so not a
like-for-like comparison.)

## Cost of intermediate zeroization

The core holds every pipeline intermediate in a `Zeroizing` buffer so clipboard
secrets are wiped from the heap after use (see `core/src/pipeline.rs` and
[`SECURITY.md`](../SECURITY.md)). The wipes cost memory bandwidth. Measured on the
same machine, 128 MiB, before vs. after enabling it (the final output is returned
without an extra copy, so single-op scenarios are cheaper than the worst case):

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
additional ASCII fast paths with Unicode fallbacks where semantics allow (W4). A
speed-tuned `opt-level = 3` profile
would likely lift the scalar-bound rows (scan, case, markdown) at a binary-size cost
— evaluate per the acceptance rules. Each change must clear ≥ 5% median gain with no
> 3% regression and all guardrails green.
