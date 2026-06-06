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

- ~~**In-menu sort-flag submenu.**~~ Done — `Sort lines`' *descending* /
  *case-insensitive* flags are now a "Sort options" submenu in the menu (and moved
  out of the Settings window so each control has one home).
- ~~**Drag-to-reorder pipeline.**~~ Delivered by exec-plan 0005 (canonical pipeline
  ordering): the pipeline runs in a correct/efficient canonical order by default, and
  the Settings window's "Manual order" mode provides drag-to-reorder for exact control.
- ~~**Measured throughput for `defang` / `clean_urls`.**~~ Done — the throughput
  harness (`make perf`) now measures the new ops alongside the existing pipeline.
