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

## From exec-plan 0004 — extraction, defang/refang, URL cleaning

- **In-menu sort-flag submenu.** Surface `Sort lines`' *descending* / *case-insensitive*
  flags as a menu submenu; today they live only in the Settings window. (0004 → Phase 3)
- **Drag-to-reorder pipeline.** Let the user reorder the operation pipeline in the
  Settings window; today order follows menu/insertion order. (0004 → Phase 3)
- **Measured throughput for `defang` / `clean_urls`.** Add real `make perf` figures for
  the two new ops to [`docs/performance.md`](performance.md). Linearity is already
  guarded by `core/tests/perf_guard.rs`; only the reported numbers are missing.
  (0004 → Phase 4)
