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

- **Bounded proof harness (Kani) over the resource envelope.** A small `kani` proof
  track for the crisp arithmetic/resource parts only: the saturating growth
  multiplication cannot wrap to acceptance, the operation-count bound is enforced, and
  the canonical-rank ordering is a total order. Deferred so it does not destabilize
  normal stable development; the same properties are currently covered by the
  `reference_transform` growth-envelope property and `config_roundtrip` saturation
  tests. Do **not** attempt to Kani-prove the full text transformer. (0012 → Phase 3)

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
