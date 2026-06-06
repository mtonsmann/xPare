# Exec Plan 0009 — HTML-to-Markdown Clipboard Conversion

Status: **completed** · Started: 2026-06-06 · Completed: 2026-06-06

## Goal

Add a focused one-shot command that converts copied rich/web HTML into readable
Markdown while preserving SafetyStrip's core invariants: no ABI change, no new
dependency, no network/IO/logging in the core, deterministic output, and no
clipboard-content persistence.

This is a conversion command, not a continuous cleanup toggle. The shell should
run it only when the user explicitly chooses the command, and it should write the
Markdown result back as plain text.

## Scope

- Add a new config operation, `html_to_markdown`, implemented in the Rust core.
- Convert common copied-web structures: headings, paragraphs, links, lists,
  blockquotes, inline emphasis/code, preformatted code blocks, line breaks, and
  simple table rows.
- Keep active content inert: drop comments, declarations, processing instructions,
  and `<script>`/`<style>` raw-text bodies; do not emit unsafe `javascript:` /
  `data:` / `vbscript:` / `file:` links.
- Add macOS menu wiring as a one-shot command under "Extract / convert".
- Do not add a parser/converter dependency in this pass.
- Do not change the C ABI or write rich `public.html` pasteboard flavors.

## Implementation Plan

1. **Core schema and operation**
   - Add `Operation::HtmlToMarkdown`, capability JSON, pipeline dispatch, and
     round-trip coverage.
   - Implement the converter as a new pure operation module so the existing
     `strip_html` security workhorse remains behaviorally unchanged.
   - Reuse the existing curated entity decoder through a small core-internal helper.

2. **Core behavior and tests**
   - Pin documented converter rules in the operation doc comment.
   - Add focused regression tests for headings/paragraphs, links, lists, code,
     entities, dropped scripts/styles, unsafe links, malformed tags, and pipeline
     dispatch.
   - Add property coverage for panic freedom and determinism over arbitrary strings.

3. **macOS shell**
   - Add Swift enum/coding support for `html_to_markdown`.
   - Add a menu command that calls `runOnce(operations: [.htmlToMarkdown])`.
   - Ensure transient HTML-to-Markdown runs on raw HTML input rather than having
     `strip_html` injected ahead of it; other transient commands keep the existing
     HTML-neutralization behavior.

4. **Docs and verification**
   - Update `ARCHITECTURE.md`, `DESIGN.md`, `SECURITY.md`, and the transform/shell
     guardrails for the supported transform and its limitations.
   - Run focused Rust and Swift tests first, then the relevant full checks. If a
     long-running or environment-sensitive check is skipped, call that out.

## Acceptance Criteria

- A copied HTML fragment with headings, links, and lists converts to readable
  Markdown as a one-shot command.
- Existing `StripHtml` / `StripMarkdown` outputs remain unchanged.
- No new dependency, no ABI change, no new entitlement, and no clipboard-content
  logging or persistence.
- Core tests include adversarial malformed input and prove deterministic output.
- Shell tests prove the command is transient and does not inject `strip_html` before
  `html_to_markdown`.

## Decision Log

- 2026-06-06: Choose a worktree branch from `origin/main` after performance wave 10
  merged. Main checkout has unrelated local edits, so the feature lives in
  `/private/tmp/SafetyStrip-html-to-markdown` on `codex/html-to-markdown-convert`.
- 2026-06-06: Choose no new parser/converter dependency for v1. A small converter is
  enough for common copied-web content and avoids expanding the core dependency and
  audit surface.
- 2026-06-06: Treat the feature as a user-triggered command, not a persistent
  rewrite toggle or continuous-mode operation.
