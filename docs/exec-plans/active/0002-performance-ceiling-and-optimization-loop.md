# Exec Plan 0002 — Performance ceiling & optimization loop

Status: **active** · Started: 2026-06-05

## Goal

Drive SafetyStrip's transform performance toward a *calibrated practical maximum*
for its current semantics, or to documented diminishing returns, **without
weakening any safety/privacy invariant**: the frozen C ABI, the pure
`#![forbid(unsafe_code)]` core, no OS/IO/network in the core, no-network-anywhere,
no clipboard-content logging/persistence, deterministic output, and (in the shell)
the pasteboard stale-write protections.

This is the *methodology* track. It defines how we measure, what "fast enough"
means, and the rules an optimization must satisfy to land. It deliberately does
**not** mandate an absolute MiB/s threshold in CI — see [§ CI posture](#ci-posture).

## Measurement layers (already in the tree + this plan)

| Layer | File | Role |
|-------|------|------|
| **Complexity gate** (always-on) | [`core/tests/perf_guard.rs`](../../../core/tests/perf_guard.rs) | CI-safe linear-time budget (orders of magnitude headroom; cannot flake). Catches O(n²)/DoS regressions, **not** speed. |
| **Statistical benches** | [`core/benches/transform.rs`](../../../core/benches/transform.rs), [`transform_large.rs`](../../../core/benches/transform_large.rs) | criterion micro/macro benchmarks with confidence intervals — the authoritative *measurement* tool. `make bench`, `make bench-large`. |
| **Throughput baseline** (opt-in) | [`core/tests/throughput.rs`](../../../core/tests/throughput.rs) | `make perf` — one quick roofline-calibrated MiB/s table for same-machine regression reads, with an optional hard floor. Synthetic input only. |

"Benchmarks measure; the guard fails." The throughput harness is the same-machine
*reporting* arm; it is `#[ignore]`d so noisy absolute numbers never gate `cargo test`.

## Ceiling model

The hardware roofline is memory bandwidth — an *unreachable* upper bound, not a
target. For a machine with bandwidth `B` GB/s, peak traffic is `B * 1e9 / 1024² ≈
B * 953.7` MiB/s. A perfect transform reads each input byte once and writes an
equal-sized output once — ≥ 2 bytes of traffic per input byte — so its absolute
input-throughput ceiling is roughly `B * 477` MiB/s. Treat that as a sanity bound
only; the practical ceiling is far lower because SafetyStrip decodes UTF-8, parses
syntax, branches per character, allocates output strings, hashes lines for dedupe,
and zeroizes buffers.

Calibrate against the harness's own `roofline-byte-scan` / `roofline-byte-copy`
controls (measured on *this* machine each run), and define each semantic class as
"within a small multiple of the relevant control":

- Streaming ASCII transforms: within **2×** of the scan/copy roofline.
- Multi-operation default pipeline: within **3×**.
- Unicode / dedupe / sort paths: within **4×**, unless profiling proves the
  semantic work (Unicode case expansion, secure line hashing, comparison sorts)
  dominates.

Record the machine, command, and medians in [`docs/performance.md`](../../performance.md)
after every accepted change. Do **not** copy another machine's numbers in as if
they were local — always re-measure.

## Optimization waves

Ordered by safety/confidence. Several early wins are **already banked** in the
current tree (see the decision log); they are listed for continuity.

- **W0 — Measure first.** Roofline controls + per-op + end-to-end scenarios in the
  throughput harness; criterion benches for stats. Profile before broad changes;
  prefer built-in tools, add a profiling dependency only after dependency-posture
  review. *(Done — `core/tests/throughput.rs` + criterion benches.)*
- **W1 — Remove copy amplification.** No-op fast paths when an op's trigger bytes
  are absent (HTML with no `<`/`&`, whitespace-collapse with no tab/double-space),
  guarded by golden tests so output stays byte-for-byte identical. *(Partially
  banked: `collapse_whitespace` has a byte-oriented fast path; `strip_html` has a
  guarded marker-free text path that still preserves newline-collapsing and
  document-trim semantics; `strip_markdown` suffix-scans newline bookkeeping and
  trims final output in place; `transform` borrows caller-owned input for the first
  pass and only zeroizes operation outputs that feed later passes.)*
- **W2 — Stream line ops.** Rewrite `trim_trailing_whitespace`, `remove_blank_lines`,
  `unwrap_lines`, and the line-list ops to stream into one output buffer instead of
  `collect`→`join`. *(Partially banked: `sort_lines` no longer allocates a per-line
  key on the case-sensitive path, and shared line joining pre-sizes its output from
  known borrowed line lengths; `trim_trailing_whitespace` trims borrowed input
  slices instead of buffering each line.)*
- **W3 — Fuse compatible passes.** A planner that fuses adjacent ops (e.g. line-ending
  normalization + a line op, or collapse + trim where ordering permits) without
  changing visible semantics or the public config. Golden-tested fused-vs-unfused.
  *(Partially banked: adjacent `TrimTrailingWhitespace` → `RemoveBlankLines` is
  fused inside the pipeline executor, and the common `CollapseWhitespace` →
  `TrimTrailingWhitespace` → `RemoveBlankLines` suffix is fused when adjacent.)*
- **W4 — Byte-oriented fast paths.** ASCII-specialized loops falling back to the
  Unicode-safe path on non-ASCII; byte scans where char boundaries are irrelevant.
  Consider `memchr` only if local benches show a clear gain **and** dependency
  guardrails approve it. *(Partially banked: whole-text Upper/Lower use standard
  ASCII fast paths with full Unicode fallback; sentence-case streams lowercase
  mappings directly and uses an ASCII scanner on ASCII chars.)*
- **W5 — Dedupe-specific.** Bench repeated/unique/long/adversarial lines separately;
  preserve exact first-occurrence semantics; consider pre-sizing the `HashSet`. Do
  **not** switch to a weaker hasher (adversarial-input risk). *(Partially banked:
  `dedupe_lines` pre-sizes the membership set and kept-line vector from the known
  line count.)*
- **W5b — IOC-specific.** Defang/refang and URL cleaning are measured as separate
  synthetic rows. Keep the documented token/marker heuristics exact, avoid new parser
  dependencies, and favor bounded byte dispatch over repeated replacement or
  per-position table scans. *(Partially banked: `refang` dispatches by first marker
  byte instead of checking every marker at every byte and copies literal spans up to
  the next marker-trigger byte; `defang` avoids an extra transformed-token wrapper
  allocation and prefilters marker families before expensive idempotence checks,
  streams transformed cores directly into the final output, and skips no-op tokens
  that contain none of the bytes any handled indicator needs; `clean_urls` streams
  URL token reconstruction into the final output instead of allocating per-token
  rebuilt strings and skips no-op prose tokens that cannot expose a URL prefix after
  punctuation trimming; tracker-key checks dispatch by first byte.)*
- **W6 — Shell responsiveness** (macOS): measure Swift↔Rust copies separately; move
  large transforms off the main actor while keeping pasteboard reads/writes on it;
  re-check `changeCount` before commit; keep `NSPasteboard.general` opt-in. Land the
  off-main-actor transform together with the ABI-v3 shell-integration pass (below) so
  the shell's transform path is touched once. Off-thread transform is a per-shell
  requirement — see the [shell-contract guardrail](../../guardrails/shell-contract.md).
- **W7 — Release profile & docs.** Update `docs/performance.md` each wave; add
  `PERF_MIN_MIB_PER_SEC` guidance only for calibrated same-machine checks. Release
  artifacts are speed-tuned when local measurements justify the binary-size tradeoff.

## Acceptance rules (per attempt)

An optimization lands only if **all** hold:

1. Deterministic output preserved for every existing test and new targeted tests
   (`cargo xtask ci` green).
2. `core/` stays `#![forbid(unsafe_code)]` with no OS/IO/network deps
   (`check-unsafe-forbid`, `check-core-deps`, `check-no-network`).
3. No C ABI change unless a separate ABI plan is opened (`check-abi`).
4. No clipboard content logged, persisted, hashed for diagnostics, or transmitted.
5. It improves at least one targeted **median** by ≥ 5% with no other targeted
   median regressing > 3% — unless the change is pure measurement infrastructure or
   unlocks a documented later wave.

## Diminishing-returns rule

Declare done only when all hold, and record it in the decision log:

1. Two consecutive waves fail to produce ≥ 5% median improvement in `default-log`
   or `full-menu-log` at the standard sizes.
2. Profiles show the remaining dominant cost is required semantic work, Unicode
   behavior, exact dedupe hashing, platform pasteboard APIs, or unavoidable
   Swift/Rust ownership copies.
3. No backlog item has a plausible ≥ 10% win at acceptable safety/dependency/
   correctness/maintenance risk.

## CI posture

Absolute MiB/s thresholds are **not** mandatory in general CI — shared runners are
too noisy. The always-on gate is `perf_guard.rs` (complexity, not speed). A floor
via `SS_PERF_MIN_MIB_PER_SEC` is meaningful only on a calibrated, dedicated runner
or in a local same-machine regression check.

## Deferred zeroization — the ABI-v3 arena design (planned; coordinate with the FFI/ABI owner)

Synchronous intermediate zeroization costs ~31% on 128 MiB end-to-end pipelines (see
`docs/performance.md`). The cost is **not** memory bandwidth — at ~2 GiB/s of traffic
against a ~55 GiB/s copy roofline the transform is ~96% compute-bound — it is the
deliberately *volatile* wipe running **serially on the transform thread**. Moving the
wipe off that path recovers most of it: a background wiper clears a 128 MiB buffer in
~40 ms while the foreground takes ~200 ms to produce the next, so it keeps pace at
queue depth ~1 with only a small memory bump.

This **cannot** live in the pure core — a wiper thread or deferred side effect would
break the core's no-OS / no-threads / deterministic / side-effect-free contract, which
is the foundation of the security argument. The clean design is at the **FFI layer**:

- The core writes intermediates into a **caller-provided arena** (a growable pool)
  instead of freeing each one; `transform` returns the final output as before.
- The FFI/shell **zeroizes and frees the arena on a background thread** once the output
  has been consumed — "hold the buffers, return now, wipe before close."
- Net: the headless/large-input path (the CLI cleaning a big log that may contain a
  secret) regains throughput while every intermediate is still wiped; one contiguous
  arena can also use a faster wipe than scattered per-op volatile drops.

Costs/caveats: peak memory rises (intermediates held until the deferred wipe — bounded
while the wiper keeps up, which it does here), and it is an **ABI change** → **ABI v3**,
to be **coordinated with the FFI/ABI owner**, not landed unilaterally. Until then the
shipped core zeroizes synchronously (correct by default).

Do **not** "fix" the cost by making the C ABI asynchronous (callback/handle-based): it
bloats the deliberately tiny, auditable FFI surface and still would not remove the
per-shell UI-thread hop (every UI framework requires its updates on its own main
thread). Keep the ABI synchronous; the shell owns concurrency (Wave 6 + the
shell-contract guardrail).

## Verification loop

Measurement-only change:

```sh
make perf PERF_MIB=128 PERF_SAMPLES=5
```

Core transform change:

```sh
cargo run -p xtask -- ci        # fmt + clippy -D warnings + tests + invariants
make perf PERF_MIB=128 PERF_SAMPLES=7
make bench                      # criterion, for statistical confirmation
```

## Running a campaign with subagents / a workflow

A multi-wave optimization campaign is a good fit for an orchestrated, evidence-first
loop: independent **profiling**, **correctness-review**, and **shell/FFI** slices
feeding a single editing owner that applies one change at a time and re-measures.
Keep subagents read-only unless assigned a disjoint edit; security-sensitive,
dependency, ABI, or clipboard-path changes always require main-agent review. Never
let two agents edit the same file family at once.

## Decision log

- 2026-06-05: Establish the methodology — roofline-calibrated, synthetic-only,
  median-reported throughput; criterion for statistics; `perf_guard` as the only
  always-on gate; explicit accept / diminishing-returns rules. (Ported and adapted
  from the upstream FormatStripper performance track onto the SafetyStrip tree:
  `make perf` now drives `core/tests/throughput.rs` instead of an FFI-coupled test,
  and CI gates complexity, not absolute speed.)
- 2026-06-05: Already-banked wins recorded for continuity — the O(n²) `strip_html`
  newline-collapse fix (now pinned by `perf_guard.rs`), `sort_lines` case-sensitive
  no-key-allocation, and the HTML/whitespace no-op fast paths.
- 2026-06-05: Captured the first local baseline (Apple M5 Pro) in `docs/performance.md`.
- 2026-06-05: Measured the cost of the in-core intermediate zeroization — a SECURITY
  posture change, **not** a perf optimization, so outside the ≥5%/-3% accept rule. At
  128 MiB: default-log 182.3 → 125.0 MiB/s (−31%), full-menu 162.7 → 108.3 (−33%),
  lossy 175.9 → 122.1 (−31%). Mitigated by returning the final output without an extra
  copy; negligible at clipboard scale. The shipped baseline above now reflects it.
- 2026-06-05: Ported the useful measurement-only salvage from the obsolete performance
  branch into `core/tests/throughput.rs`: sparse stripper rows, unique-line dedupe,
  and pipeline-decomposition rows (`html-markdown-trim-log` and
  `full-menu-without-*`). No production behavior change.
- 2026-06-05: W1 accepted for `collapse_whitespace`: rewrite the ASCII space/tab
  collapse as a safe byte-oriented loop with lazy output allocation. Same-session
  128 MiB / 5-sample comparison against a temporary `main` worktree on common rows:
  `collapse-whitespace` 533.2 → 685.0 MiB/s (+28%), `default-log` 111.0 → 116.6
  (+5%), `full-menu-log` 97.8 → 102.3 (+4.6%), `lossy-utf8-log` 110.7 → 116.5
  (+5.2%).
- 2026-06-06: Wave 0 re-run after the canonical-ordering / IOC updates used
  read-only profiling, correctness-risk, and shell/FFI subagents. W4 accepted for
  whole-text ASCII Upper/Lower: `to_upper`/`to_lower` now use the standard library's
  ASCII case path when `input.is_ascii()`, falling back to full Unicode mappings for
  any non-ASCII input. Same-worktree 128 MiB / 5-sample comparison:
  `case-lower-ascii` 179.6 → 1142.9 MiB/s (+536%), `full-menu-without-dedupe`
  62.4 → 85.3 (+37%), `full-menu-log` 89.9 → 93.0 (+3.4%), `default-log`
  106.0 → 108.1 (+2%). No ABI, dependency, zeroization, ordering, or privacy
  posture change.
- 2026-06-06: W5b accepted for `refang`: replace the per-position nine-marker table
  scan with first-byte dispatch for `[`, `(`, and `h`, preserving longest-marker
  semantics and both bracket styles. Also removed a tiny per-URL scheme allocation in
  `defang_url`. Same-branch 128 MiB / 5-sample comparison after W4:
  `refang-iocs` 44.6 → 378.7 MiB/s (+749%), `defang-iocs` 58.6 → 62.6 (+6.8%),
  `clean-urls-trackers` 259.6 → 276.0 (+6.3%), `full-menu-log` 93.0 → 97.0
  (+4.3%), `default-log` 108.1 → 109.0 (+0.8%). No ABI, dependency, zeroization,
  ordering, or privacy posture change.
- 2026-06-06: W5 accepted for unique-line dedupe allocation: `dedupe_lines` now
  pre-sizes the `HashSet` and kept-line `Vec` from the already-known line count,
  preserving first-occurrence output order and deterministic visible output.
  Same-branch 128 MiB / 5-sample comparison after W5b: `dedupe-lines-unique`
  490.1 → 680.3 MiB/s (+39%), `dedupe-lines-repeated` 761.1 → 763.6 (+0.3%),
  `default-log` 109.0 → 109.9 (+0.8%), `full-menu-log` 97.0 → 96.7 (−0.3%).
  No ABI, dependency, zeroization, ordering, or privacy posture change.
- 2026-06-06: W5d accepted for `defang`: transformed tokens with no trimmed
  punctuation now reuse the transformed core directly, and the already-defanged
  guard first scans for possible marker-family bytes before running substring
  checks. Same-branch 128 MiB / 5-sample comparison after W5c: `defang-iocs`
  62.2 → 122.1 MiB/s (+96%), `refang-iocs` 375.7 → 368.2 (−2.0%),
  `default-log` 109.9 → 108.8 (−1.0%), `full-menu-log` 96.7 → 96.2 (−0.5%).
  No ABI, dependency, zeroization, ordering, or privacy posture change.
- 2026-06-06: W2 accepted for shared line-join allocation: `join_lines` now
  pre-sizes its output buffer from the kept line lengths, separator count, and
  trailing-newline flag, leaving the existing push order and visible output
  unchanged. Two same-size runs were taken because the first had a noisy
  `dedupe-lines-repeated` median; the confirming 128 MiB / 5-sample run after W5d
  showed `remove-blank-lines` 758.4 → 819.5 MiB/s (+8.1%),
  `sort-lines` 546.8 → 575.9 (+5.3%), `dedupe-lines-unique` 708.6 → 724.6
  (+2.3%), `dedupe-lines-repeated` 752.3 → 754.9 (+0.3%), `default-log`
  108.8 → 109.4 (+0.6%), and `full-menu-log` 96.2 → 96.4 (+0.2%). No ABI,
  dependency, zeroization, ordering, or privacy posture change.
- 2026-06-06: W4b accepted for sentence-case scanning: `to_sentence` now streams
  lowercase mappings directly into the sentence scanner, uses an ASCII fast path for
  ASCII chars, and streams Unicode uppercase expansions without per-character
  temporary `String` allocation. Exact-output tests pin Unicode lowercase expansion
  (`İ` → `i\u{307}` before sentence capitalization) and uppercase expansion
  (`ß` → `SS`). Same-branch 128 MiB / 5-sample comparison after the W2 line-join
  wave: `case-sentence-unicode` 122.0 → 240.6 MiB/s (+97%), `default-log`
  109.4 → 112.2 (+2.6%), `full-menu-log` 96.4 → 98.7 (+2.4%), `lossy-utf8-log`
  109.9 → 111.8 (+1.7%). No ABI, dependency, zeroization, ordering, or privacy
  posture change.
- 2026-06-06: W2b accepted for `trim_trailing_whitespace`: the op now slices the
  original input at newline boundaries and trims borrowed line slices, removing the
  per-line scratch `String` while preserving Unicode trailing trim, CRLF-to-LF
  behavior, multi-CR trimming, final-CR trimming, and all-blank newline preservation.
  Two same-size runs were taken because the first had unrelated row noise; the
  confirming 128 MiB / 5-sample run after W4b showed `trim-trailing`
  486.9 → 557.9 MiB/s (+14.6%), `html-markdown-trim-log` 152.6 → 159.0 (+4.2%),
  `default-log` 112.2 → 115.7 (+3.1%), `full-menu-log` 98.7 → 101.6 (+2.9%),
  and `lossy-utf8-log` 111.8 → 115.4 (+3.2%). No ABI, dependency, zeroization,
  ordering, or privacy posture change.
- 2026-06-06: W1b accepted for Markdown output bookkeeping: `MarkdownOutput::push_str`
  now derives the newline-collapse state from the emitted text suffix instead of
  scanning every byte of ordinary text, leading structural newlines are skipped when
  the output is still empty, and final edge trimming mutates the output `String`
  in place while preserving the exact ASCII trim set. Focused regressions pin
  structural edge trimming and NBSP edge preservation. Same-branch 128 MiB /
  5-sample comparison after W2b: `strip-markdown-sparse-log` 361.4 → 591.1 MiB/s
  (+64%), `strip-markdown-heavy` 133.1 → 133.9 (+0.6%),
  `html-markdown-trim-log` 159.0 → 174.9 (+10.0%), `default-log`
  115.7 → 124.7 (+7.8%), `full-menu-log` 101.6 → 108.8 (+7.1%), and
  `lossy-utf8-log` 115.4 → 123.8 (+7.3%). No ABI, dependency, zeroization,
  ordering, or privacy posture change.
- 2026-06-06: W3 accepted for `TrimTrailingWhitespace` → `RemoveBlankLines` fusion:
  the pipeline executor now consumes that adjacent pair as one private fused pass
  when canonical ordering places them together or `as_given` specifies them together.
  A proptest equivalence check compares fused transform output with the public
  unfused ops, including canonical reordering, and caught/fixed the final-blank-line
  trailing-newline edge. Same-branch 128 MiB / 5-sample comparison after W1b:
  `default-log` 124.7 → 142.2 MiB/s (+14%), `full-menu-log` 108.8 → 122.7
  (+13%), `lossy-utf8-log` 123.8 → 142.2 (+15%), `full-menu-without-markdown`
  136.5 → 160.4 (+18%), and `full-menu-without-collapse` 126.7 → 145.8 (+15%).
  No ABI, dependency, zeroization, ordering, or privacy posture change.
- 2026-06-06: W3b accepted for `CollapseWhitespace` → `TrimTrailingWhitespace` →
  `RemoveBlankLines` fusion: the pipeline executor now consumes the common adjacent
  default-pipeline suffix as one private pass, reusing a transform-local
  boundary-zeroized collapse scratch buffer instead of materializing and zeroizing
  the full collapse output before line cleanup. A proptest equivalence check compares
  fused transform output with the public unfused ops for both `as_given` and
  canonical ordering. Same-branch
  128 MiB / 5-sample comparison after W3: `default-log` 142.2 → 157.8 MiB/s
  (+11%), `full-menu-log` 122.7 → 133.4 (+8.7%), `lossy-utf8-log`
  142.2 → 158.2 (+11%), `full-menu-without-markdown` 160.4 → 178.2 (+11%),
  and `full-menu-without-dedupe` 112.4 → 122.2 (+8.7%). No ABI, dependency,
  ordering, or privacy posture change.
- 2026-06-06: Security review closure for the W3b fused-scratch zeroization finding:
  the issue class is fused core scratch buffers holding clipboard-derived bytes
  outside the pipeline's wipe posture. `collapse_trim_then_remove_blank_lines` now
  wraps the collapsed-line scratch in `Zeroizing<Vec<u8>>`, zeroizes it before
  capacity growth can release old storage, and relies on drop-time zeroization for
  allocation-preserving reuse. `cargo xtask check-pipeline-zeroization` blocks a
  regression to plain `Vec::new()` or growth without `scratch.zeroize()`.
  Threat-model calibration: zeroization is a best-effort persistence control for
  SafetyStrip-owned heap storage after use, not a live-memory exfiltration control;
  private scratch that remains owned by one transform does not need a hot-path wipe
  on every line. Same-worktree 128 MiB / 5-sample conservative per-line-wipe →
  boundary-wipe comparison: `default-log` 160.6 → 198.0 MiB/s (+23%),
  `full-menu-log` 136.0 → 160.2 (+18%), `lossy-utf8-log` 140.7 → 195.8
  (+39%), `full-menu-without-markdown` 191.5 → 229.2 (+20%),
  `full-menu-without-dedupe` 124.2 → 144.9 (+17%), and
  `full-menu-without-case` 135.9 → 161.2 (+19%). `docs/performance.md` now records
  the revised baseline and scratch-zeroization policy.
- 2026-06-06: Rejected a private `DedupeLines` → `UnwrapLines` →
  `ChangeCase(Lower)` full-menu-tail fusion after equivalence proptests passed but
  the 128 MiB / 5-sample throughput run regressed `full-menu-log`
  133.4 → 129.7 MiB/s. The synthetic full-menu log dedupes down to a tiny tail, so
  this does not look like a promising near-term wave without a different workload.
- 2026-06-06: W1c accepted for marker-free HTML text: `strip_html` now takes a
  guarded fast path when the input has no `<` or `&`, still routing literal newlines
  through the existing newline-collapser and trimming only the documented ASCII edge
  bytes in place. Exact-output tests pin marker-free Unicode/prose, `>` literals,
  blank-line collapsing, all-edge trimming, and NBSP edge preservation. To avoid
  thermal/noise drift, the same 128 MiB / 5-sample comparison was run against a
  temporary clean worktree at the previous checkpoint under current machine
  conditions: `strip-html-plain` 423.3 → 954.1 MiB/s (+125%),
  `strip-html-sparse-log` 417.9 → 861.1 (+106%), `strip-html-heavy`
  332.4 → 329.8 (−0.8%), `html-markdown-trim-log` 173.1 → 219.8 (+27%),
  `default-log` 156.3 → 194.9 (+25%), `full-menu-log` 132.0 → 158.7 (+20%),
  and `lossy-utf8-log` 154.6 → 193.5 (+25%). No ABI, dependency, zeroization,
  ordering, or privacy posture change.
- 2026-06-06: W5e accepted for `clean_urls`: URL candidates now stream their
  cleaned token reconstruction directly into the transform output, and surviving
  query pairs are emitted as they are scanned instead of first collecting a
  `Vec<&str>`. This removes temporary rebuilt URL strings and the survivor list
  while preserving the documented token, punctuation, query, fragment, tracker-key,
  and idempotence rules. Same-worktree 128 MiB / 5-sample comparison against fresh
  `origin/main`: `clean-urls-trackers` 278.2 → 310.4 MiB/s (+11.6%),
  `default-log` 195.9 → 195.0 (−0.5%), `full-menu-log` 156.0 → 158.6 (+1.7%),
  and `lossy-utf8-log` 191.1 → 193.4 (+1.2%). The unrelated IOC rows stayed within
  the −3% rule (`defang-iocs` 129.9 → 128.1, `refang-iocs` 379.3 → 369.9). No ABI,
  dependency, zeroization, ordering, or privacy posture change; the change removes
  short-lived allocations and adds no transform-local scratch.
- 2026-06-06: W5f accepted for `defang`: transformed URL/email/IP/domain cores now
  stream directly into the transform output after classification, instead of
  allocating a temporary transformed core `String` for every indicator token and
  copying it into the outer output. Classifier order, already-defanged detection,
  punctuation reattachment, bracket style, and URL scheme handling are unchanged.
  Same-worktree 128 MiB / 5-sample comparison after W5e:
  `defang-iocs` 128.1 → 140.6 MiB/s (+9.8%), `refang-iocs` 369.9 → 369.9 (+0%),
  `clean-urls-trackers` 310.4 → 312.7 (+0.7%), `default-log` 195.0 → 195.8
  (+0.4%), `full-menu-log` 158.6 → 158.2 (−0.3%), and `lossy-utf8-log`
  193.4 → 193.1 (−0.2%). No ABI, dependency, zeroization, ordering, or privacy
  posture change; the change removes short-lived allocations and adds no
  transform-local scratch.
- 2026-06-06: W5g accepted for `defang`: token cores that contain none of `.`, `@`,
  `:`, `[`, or `(` now skip already-defanged marker checks and the URL/email/IP/domain
  classifier chain. This is an exact no-op prefilter: every live indicator class
  handled by `defang` needs `.`, `@`, or `:`, and every already-defanged bracket marker
  needs `[` or `(`. Same-worktree 128 MiB / 5-sample comparison after W5f confirmed
  the target row twice despite broader machine noise: `defang-iocs` 140.6 → 183.0
  MiB/s (+31%) and then 140.6 → 187.1 (+33%). In the confirmation run, adjacent IOC
  rows stayed within the −3% rule (`refang-iocs` 369.9 → 359.1, `clean-urls-trackers`
  312.7 → 304.0). No ABI, dependency, zeroization, ordering, or privacy posture
  change; the change removes classifier work on ordinary prose tokens and adds no
  transform-local scratch.
- 2026-06-06: W5h accepted for `clean_urls`: non-whitespace tokens now skip
  punctuation trimming and URL-prefix checks when the first byte after the fixed
  ASCII trim-punctuation set is not lowercase `h` or `w`. This is an exact no-op
  prefilter because the only recognized URL prefixes are `http://`, `https://`, and
  `www.`. Same-worktree 128 MiB / 5-sample comparison after W5g:
  `clean-urls-trackers` 312.7 → 331.9 MiB/s (+6.1%). The op is not part of
  `default-log` or `full-menu-log`; nearby IOC rows stayed within the −3% rule
  (`defang-iocs` 187.1 → 188.9, `refang-iocs` 369.9 → 369.3). No ABI, dependency,
  zeroization, ordering, or privacy posture change; the change removes trim/prefix
  work on ordinary prose tokens and adds no transform-local scratch.
- 2026-06-06: W5i accepted for `clean_urls`: tracker-key matching now dispatches by
  the ASCII-lowercased first key byte instead of scanning the full tracker table for
  every query pair. Exact matching remains case-insensitive, and `utm_*` / `oly_*`
  remain prefix rules. A focused test iterates `TRACKING_PARAMS` and verifies each
  configured entry drops through the public transform, plus a regression keeps
  non-wildcard stems like `utm` / `oly` / `utm-source` as functional params.
  Same-worktree 128 MiB / 5-sample comparison after W5h:
  `clean-urls-trackers` 331.9 → 392.0 MiB/s (+18%). Nearby IOC rows stayed within
  the −3% rule (`defang-iocs` 188.9 → 186.9, `refang-iocs` 369.3 → 369.1). No ABI,
  dependency, zeroization, ordering, or privacy posture change; the change removes
  unnecessary tracker-name comparisons and adds no transform-local scratch.
- 2026-06-06: W7 accepted for release profile speed tuning: `[profile.release]`
  now uses `opt-level = 3` instead of the former size-tuned `opt-level = "s"`.
  This is a broad build-profile tradeoff, not a semantic transform change: output,
  ABI, dependencies, zeroization, ordering, and privacy posture are unchanged, but
  release artifacts no longer optimize for smallest size by default. Same-worktree
  128 MiB / 5-sample comparison after W5i: `default-log` 193.4 → 221.6 MiB/s
  (+11%), `full-menu-log` 157.8 → 182.8 (+16%), `lossy-utf8-log`
  191.2 → 218.6 (+9.5%), `strip-markdown-heavy` 141.5 → 153.0 (+8.1%),
  `strip-markdown-sparse-log` 589.3 → 687.1 (+17%), `strip-html-heavy`
  334.0 → 421.8 (+25%), `defang-iocs` 186.9 → 237.5 (+27%), and
  `clean-urls-trackers` 392.0 → 430.0 (+9.7%). The only targeted IOC row that moved
  down was `refang-iocs` 369.1 → 361.4 (−2.1%), inside the −3% rule.
- 2026-06-06: W5j accepted for `refang`: when no marker matches at the current byte,
  the fallback path now copies the whole literal span up to the next marker-trigger
  byte (`[`, `(`, or `h`) instead of copying one UTF-8 character at a time. The
  existing marker matcher still owns all substitutions, so bracket style handling,
  longest-marker precedence, `hxxp` semantics, and Unicode literal preservation are
  unchanged. A focused regression covers long literal spans, Unicode, and near-miss
  marker text. Same-worktree 128 MiB / 5-sample comparison after W7:
  `refang-iocs` 365.6 → 790.5 MiB/s (+116%). Adjacent rows stayed within the −3%
  rule (`defang-iocs` 242.0 → 238.0, `clean-urls-trackers` 432.8 → 429.5). No ABI,
  dependency, zeroization, ordering, or privacy posture change; the change adds no
  transform-local scratch and only reduces fallback scanner work.
- 2026-06-06: W1d accepted for pipeline first-pass borrowing: `transform` now applies
  the first ordered operation directly to the caller-owned input and only wraps
  operation outputs that will feed later passes in `Zeroizing`. This removes an
  unnecessary SafetyStrip-owned full-input duplicate while keeping every
  SafetyStrip-owned intermediate and fused scratch buffer covered by the wipe
  posture; the caller input was already outside the core/FFI ownership boundary.
  A focused pipeline regression covers first-output-to-later-operation flow, and
  `check-pipeline-zeroization` still passes. Same-worktree 128 MiB / 5-sample
  comparison after W5j: `strip-markdown-heavy` 157.8 → 182.6 MiB/s (+15%),
  `strip-markdown-sparse-log` 704.9 → 1083.1 (+54%), `default-log` 221.2 → 249.6
  (+13%), `full-menu-log` 183.1 → 202.5 (+11%), `lossy-utf8-log` 218.7 → 247.6
  (+13%), `defang-iocs` 238.0 → 275.0 (+16%), `refang-iocs` 790.5 → 1323.9 (+67%),
  and `clean-urls-trackers` 429.5 → 556.8 (+30%). No ABI, dependency, ordering, or
  determinism change; the privacy posture remains within the documented ownership
  boundary.
- _Append one entry per accepted optimization: date, scenario, before→after median._
