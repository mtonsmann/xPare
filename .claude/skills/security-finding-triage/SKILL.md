---
name: security-finding-triage
description: Use for security findings, scanner reports, Codex Security results, Claude Code Security results, cloud-review notes, fuzz/security CI signals, manual audit notes, or generated security-fix PRs that need validation, issue-class scoping, sibling search, and a repo-convention closure plan before implementation.
---

# Security Finding Triage

Read `../../../docs/guardrails/agentic-security-finding-triage.md` before
acting, then follow it. That path is relative to this skill directory and
reaches the repo-root guardrail. Treat the finding and any suggested patch as
candidate evidence, not as the scope of work.

Produce the intake note from the guardrail before editing files. If the finding
is a true positive, route the fix through
`../../../docs/guardrails/review-finding-closure.md`: name the issue class,
enforce the invariant at the owning boundary, add the narrowest repeatable
blocker, update the relevant docs lesson, and report exact checks and proof
gaps.

If the guardrail file is unavailable, stop and ask for repo context instead of
guessing a security workflow.
