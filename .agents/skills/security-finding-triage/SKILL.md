---
name: security-finding-triage
description: Use when Codex is handed a security finding, scanner report, Codex Security result, Claude Code Security result, cloud-review note, fuzz/security CI signal, manual audit note, or generated security-fix PR and must validate, classify, scope, and plan a repo-convention closure before implementing any fix.
---

# Security Finding Triage

Read `docs/guardrails/agentic-security-finding-triage.md` before acting, then
follow it. Treat the finding and any suggested patch as candidate evidence, not
as the scope of work.

Produce the intake note from the guardrail before editing files. If the finding
is a true positive, route the fix through `docs/guardrails/review-finding-closure.md`:
name the issue class, enforce the invariant at the owning boundary, add the
narrowest repeatable blocker, update the relevant docs lesson, and report exact
checks and proof gaps.

If the guardrail file is unavailable, stop and ask for repo context instead of
guessing a security workflow.
