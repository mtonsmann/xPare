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
