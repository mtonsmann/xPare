# Agentic Security Finding Triage

## Goal

Make security-finding triage agent-neutral so findings from Codex Security,
Claude Code Security, cloud reviewers, scanners, fuzzing, or manual audit enter
the same evidence-first closure workflow before any agent writes a patch.

## Change class

Documentation/process and agent workflow packaging. No product behavior,
privacy posture, ABI, dependency, or release boundary change.

## Plan

1. Add one canonical, repo-owned triage guardrail that routes security findings
   into the existing review-finding closure policy.
2. Add thin Codex and Claude skill wrappers named `security-finding-triage` that
   load the canonical guardrail instead of duplicating it.
3. Add `CLAUDE.md` importing `AGENTS.md`, with a Claude-specific pointer to the
   skill wrapper.
4. Add short routing pointers from `AGENTS.md`, `docs/agent-workflow.md`, and
   `docs/agent-tasks/review-finding-closure.md`.
5. Validate markdown/skill structure and inspect the final diff.

## Decision log

- Use one canonical guardrail under `docs/guardrails/` because AGENTS.md says
  durable detailed rules belong there, not in tool-specific wrappers.
- Keep skills instruction-only. The task is judgment-heavy and should rely on
  repo guardrails plus existing tests/checks, not bundled scripts.
- Use the neutral name `security-finding-triage` so the workflow applies to
  Codex, Claude, scanners, CI security signals, manual audits, or any future
  agentic security finding source.
- Add `CLAUDE.md` with an `@AGENTS.md` import so Claude reads the same repo
  guidance as Codex, plus a small Claude-specific skill invocation pointer.

## Outcome

- Added `docs/guardrails/agentic-security-finding-triage.md` as the canonical
  intake guardrail for security findings.
- Added Codex wrapper `.agents/skills/security-finding-triage/`.
- Added Claude wrapper `.claude/skills/security-finding-triage/`.
- Added `CLAUDE.md` importing `AGENTS.md`.
- Added `.gitignore` entries so Claude local settings and worktrees stay
  untracked while `.claude/skills/` remains commit-ready.
- Wired pointers into `AGENTS.md`, `docs/agent-workflow.md`,
  `docs/agent-tasks/review-finding-closure.md`, and
  `docs/guardrails/review-finding-closure.md`.
- Routed existing security/posture entry points (`SECURITY.md`, `DESIGN.md`,
  `ARCHITECTURE.md`, `docs/guardrails/code-and-test-hygiene.md`) through the
  triage guardrail before review-finding closure.
- Added a `Security finding triage` section to the PR template so finding fixes
  prompt for status, issue class, source/sink/control, owning boundary, sibling
  search, and proof gaps.
- Extended `check-agent-workflow` so CI verifies the Codex and Claude
  `security-finding-triage` wrappers keep their required headings and
  repo-root guardrail links.

## Validation

- `python3 - <<'PY' ... PY` structural skill check: superseded by the committed
  `check-agent-workflow` skill-wrapper guard.
- `cargo fmt --all --check`: passed.
- `cargo clippy -p xtask --all-targets -- -D warnings`: passed.
- `cargo test -p xtask`: passed.
- `cargo run -p xtask -- check-agent-workflow`: passed.
- `cargo run -p xtask -- check-docs`: passed.
- `git diff --check`: passed.
