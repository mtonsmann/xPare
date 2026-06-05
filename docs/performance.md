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

Measured 2026-06-05 on **Apple M5 Pro, 18 cores, 48 GB, arm64**, via
`make perf PERF_MIB=128 PERF_SAMPLES=5` (median of 5), on the current code **with
pipeline intermediate zeroization** and the W1 byte-oriented
`collapse_whitespace` path (see the cost section below). Re-measure on each machine;
do not assume another machine's numbers. Read each transform row relative to this
machine's own roofline controls (byte-copy ≈ 25 GiB/s in this run is the practical
memory-traffic anchor; byte-scan is lower because the shipped release profile is
`opt-level = "s"` — size-optimized — leaving the scalar scan loop unvectorized).

| Scenario | Median | Throughput |
|----------|-------:|-----------:|
| roofline-byte-scan | 0.036s | 3567.2 MiB/s |
| roofline-byte-copy | 0.005s | 25687.4 MiB/s |
| strip-html-plain (no `<`/`&`) | 0.310s | 413.5 MiB/s |
| strip-html-heavy | 0.390s | 328.1 MiB/s |
| strip-html-sparse-log | 0.313s | 408.3 MiB/s |
| strip-markdown-heavy | 1.164s | 109.9 MiB/s |
| strip-markdown-sparse-log | 0.254s | 503.6 MiB/s |
| collapse-whitespace | 0.187s | 685.0 MiB/s |
| trim-trailing | 0.276s | 464.2 MiB/s |
| remove-blank-lines | 0.175s | 733.4 MiB/s |
| unwrap-lines | 0.191s | 669.4 MiB/s |
| case-lower-ascii | 0.671s | 190.8 MiB/s |
| case-sentence-unicode | 1.074s | 119.2 MiB/s |
| dedupe-lines-repeated | 0.180s | 710.1 MiB/s |
| dedupe-lines-unique | 0.282s | 453.9 MiB/s |
| sort-lines | 0.244s | 523.7 MiB/s |
| html-markdown-trim-log | 0.770s | 166.2 MiB/s |
| full-menu-without-markdown | 1.063s | 120.4 MiB/s |
| full-menu-without-collapse | 1.074s | 119.2 MiB/s |
| full-menu-without-dedupe | 1.912s | 66.9 MiB/s |
| full-menu-without-case | 1.263s | 101.4 MiB/s |
| **default-log** (html+md+collapse+trim+blank) | 1.098s | **116.6 MiB/s** |
| **full-menu-log** (+dedupe+unwrap+lowercase) | 1.251s | **102.3 MiB/s** |
| **lossy-utf8-log** (invalid UTF-8, default pipeline) | 1.101s | **116.5 MiB/s** |

Slow lanes (optimization targets): Markdown stripping, Unicode sentence-case, and
unique-line dedupe are the slowest single-op rows. End-to-end pipelines sit at
~102–117 MiB/s. The decomposition rows show the lowercase/dedupe tail dominates the
full-menu log path on this generated corpus. (For reference,
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
ASCII fast paths with a Unicode fallback (W4). A speed-tuned `opt-level = 3` profile
would likely lift the scalar-bound rows (scan, case, markdown) at a binary-size cost
— evaluate per the acceptance rules. Each change must clear ≥ 5% median gain with no
> 3% regression and all guardrails green.
