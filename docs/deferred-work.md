# Deferred work

Feature- and task-level work that was consciously deferred from **completed** exec
plans, collected here so it survives the plan being archived to
[`exec-plans/completed/`](exec-plans/completed/). Each item links back to the plan
that deferred it.

Scope split:

- **This file** is the tactical backlog — concrete features/improvements punted from
  a specific plan.
- **Strategic / architectural** deferrals (extra platform shells, a streaming
  transform path, signing/notarization automation, telemetry, doc-GC agents) live in
  [`DESIGN.md` → "Adopt if the project grows"](../DESIGN.md#adopt-if-the-project-grows).
- **In-progress** work and its not-yet-done items stay in the active plans under
  [`exec-plans/active/`](exec-plans/active/).

Nothing here is committed scope; it's a memory aid for the next maintainer.

## From exec-plan 0001 — initial re-architecture

- **Block-level embedded-HTML stripping in `strip_markdown`.** Accumulate consecutive
  raw-HTML *block* events and strip them as one unit, dropping block-level
  `<script>`/`<style>` in embedded HTML. Deferred because `StripHtml` already covers
  the security need and is the path the shell runs. (0001 → Follow-ups)
- **Nightly fuzz campaigns.** CI runs only a smoke `cargo fuzz` pass today; run longer
  campaigns as a scheduled job. (0001 → Follow-ups)
- **Package + run the real menu-bar `.app`.** Build and launch the signed/`LSUIElement`
  app on a full Xcode toolchain (this environment is Command-Line-Tools only, so the
  app is built but never launched). (0001 → Follow-ups)

## From exec-plan 0012 — AI-native, evidence-first workflow + reference semantics

- ~~**Bounded proof harness (Kani) over the resource envelope.**~~ Delivered: the
  saturating growth product is factored into `config::saturating_growth_product`, with
  `#[cfg(kani)]` harnesses that prove the gate accepts a pipeline iff its true
  worst-case growth is within `MAX_PIPELINE_GROWTH_FACTOR` (no saturation wrap can
  falsely accept). Run via `cargo xtask check-kani`; advisory CI cadence in
  `.github/workflows/proofs.yml`. The operation-count bound and canonical-rank
  total-order are covered by plain unit tests in `config.rs` (Kani adds nothing for
  constant data). The full text transformer is intentionally **not** Kani-proved.
  (0012 → Phase 3)

## From exec-plan 0004 — extraction, defang/refang, URL cleaning

- ~~**In-menu sort-flag submenu.**~~ Done — sort is a single "Sort lines: <mode>"
  submenu: one entry whose title shows the active mode, with the modes (Off / A→Z /
  Z→A / ±ignore-case) as an inline `Picker` so the active one gets the system ✓ (the
  native Finder "Sort By" idiom). Moved out of the Settings window so each control has
  one home.
- ~~**Drag-to-reorder pipeline.**~~ Delivered by exec-plan 0005 (canonical pipeline
  ordering): the pipeline runs in a correct/efficient canonical order by default, and
  the Settings window's "Manual order" mode provides drag-to-reorder for exact control.
- ~~**Measured throughput for `defang` / `clean_urls`.**~~ Done — the throughput
  harness (`make perf`) now measures the new ops alongside the existing pipeline.

## From exec-plan 0013 — anti-slop code & test hygiene

- **`cargo-public-api` snapshot gate.** Freeze `core`'s public API surface and diff it in
  CI (like the frozen C header). Deferred because `cargo-public-api` needs a pinned
  *nightly* rustdoc and its output drifts across toolchains — a brittle, nightly-dependent
  required gate fights the repo's determinism ethos, and the concern (dangling `pub`
  surface) is already covered by `unreachable_pub` + `dead_code` + the frozen FFI
  `check-abi` + mutation testing. (0013 → D-2)
- **`lychee` markdown link-checking.** Catch dead links in `docs/`. Deferred because
  external URLs flake (network-dependent, non-deterministic); only internal/relative-link
  checking would be worth a gate, and its value is low next to `check-docs` (which already
  catches broken intra-doc links in Rust). (0013 → D-4)
- **Mutation-testing parallelism tuning.** `check-mutants` runs local at `-j <cores>`
  ("hammer the box"); the first full-tree run at `-j 10` produced contention-spurious
  timeouts. Mitigated by the `timeout_multiplier = 5` / `minimum_test_timeout = 60` in
  `.cargo/mutants.toml` (subsequent `-j 6` runs were clean). If `-j <cores>` ever
  spurious-times-out again, cap per-job test threads (so jobs × test-threads ≈ cores)
  rather than lowering `-j`. (0013 → D-5)
- **Hold the enforcement code (`xtask`) to the product's test bar.** The tier-2 review
  (D-6) found that the `xtask` checks ship with far less test coverage than the product —
  `xtask/**` is excluded from both `check-coverage` and `check-mutants` ("verified by being
  run in CI"), but "run in CI" exercises only the happy path, not the failure/parsing
  branches where a false-green hides. The highest-risk parser (`classify_ignore_line`) now
  has a unit test; the broader gap remains. Consider a scoped mutation/coverage pass over
  `xtask` (or at least unit tests for each check's failure branch). (0013 → D-6 review finding)
- ~~**Anti-slop parity for the Swift macOS shell.**~~ Largely delivered via
  `cargo xtask check-swift` (`make swift`): a **best-effort, macOS-only** tier that fronts
  `swift format lint --strict` (config in `shells/macos/.swift-format`), a `cargo build -p
  xpare-ffi --release` + `swift test`, and a Sources-only line-coverage floor via
  `llvm-cov` (`SWIFT_COVERAGE_FLOOR_PCT`, 95% — matching the Rust product floor; measured
  baseline ~96.0%). It runs in the `continue-on-error` `macos-shell` CI job (replacing the
  old `swift build` smoke), so the shell's tests now run in CI rather than only locally, and
  skips cleanly where the Swift toolchain is absent. The Swift sources were normalized once
  with `swift format` so the strict lint passes; the coverage floor is a ratchet (raise,
  never lower). The OS-facing layers are tested headlessly — `SystemPasteboard` against an
  app-private `NSPasteboard(name:)`, the Carbon hot-key trampoline via a synthesized
  `kEventHotKeyPressed` event — leaving only the `XPareApp` SwiftUI executable
  unmeasured (it isn't linked into the test bundle; the analog of the Rust binary crates the
  workspace floor doesn't gate).
  SwiftLint (style/complexity, config in `shells/macos/.swiftlint.yml`) is wired as a
  **run-if-present** phase (non-`--strict`: warnings advise, `error`-severity fails) and CI
  installs it **SHA-pinned + checksum-verified** (the `portable_swiftlint.zip` release asset,
  hashed exactly like actionlint), so it runs in CI too — not just locally. **Still
  deferred:** `periphery` (dead code) — its binary is equally pinnable, but `periphery scan`
  needs a compiler **index store** (it drives a `swift build` first) and a curated retain-list
  config to avoid false positives on the SwiftUI/`@main` surface, neither of which is worth
  wiring until the Swift surface grows; add it (also run-if-present) then. The cross-language
  *security* posture was already enforced (`check-no-content-logging` /
  `check-clipboard-safety` scan `.swift`; `check-c-ffi-surface`; entitlements).
  (0013 → cross-language follow-up)
