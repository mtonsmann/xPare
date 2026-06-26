# Exec Plan 0020 - Codex Dependabot review guidance

Status: **completed** - Started: 2026-06-26 - Completed: 2026-06-26

## Goal

Make Codex's GitHub PR review behavior line up with xPare's dependency-update
policy: a Dependabot or dependency-update PR should not receive a useful
"thumbs up" unless the review either records the dependency-posture
recommendation evidence or flags that the evidence is missing.

## Change Class

Dependency / CI and docs. No product behavior, transform output, ABI,
entitlement, clipboard, persistence, or privacy-posture change.

## Issue Class

Automated PR review can approve a dependency-update PR from the local workflow
diff alone, without the required supply-chain recommendation (`merge`, `hold`,
or `close/defer`) and without calling out failed checks.

## Decision Log

- 2026-06-26: Keep Codex code review advisory. The deterministic gate remains
  `cargo xtask ci`; the bot's job is to produce or demand evidence, not to
  become branch protection.
- 2026-06-26: Put the durable reviewer-facing rule in `AGENTS.md` under a
  `## Review guidelines` heading because Codex GitHub review uses that section
  as repository-specific review guidance.
- 2026-06-26: Use the existing `check-agent-workflow` structural gate rather
  than adding a new subcommand; this is an agent-workflow wiring invariant.

## Completed Changes

- Added `AGENTS.md` review guidelines that route Dependabot and dependency-update
  PRs to the dependency-posture rubric, require a `merge` / `hold` /
  `close/defer` recommendation, and classify missing evidence or failed-check
  approval as a P1 review finding.
- Updated `docs/guardrails/dependency-posture.md`,
  `docs/agent-tasks/dependency-ci.md`, and
  `docs/guardrails/code-and-test-hygiene.md` so the lesson lives in the durable
  change-class docs, not only in the top-level agent map.
- Extended `check-agent-workflow` to require `AGENTS.md` and the dependency-review
  markers that Codex GitHub review needs.
- Added `agent_workflow_detects_missing_dependency_review_guidance` so the guard
  has focused unit coverage.

## Evidence Packet

- `cargo fmt --all --check` passed.
- `cargo test -p xtask agent_workflow` passed: 7 tests.
- `cargo run -p xtask -- check-agent-workflow` passed.
- `cargo clippy -p xtask --all-targets -- -D warnings` passed.
- `cargo test -p xtask` passed: 122 tests.
- `git diff --check` passed.

## Proof Gaps

- This cannot force the external Codex GitHub service to emit a review on every
  trigger; it makes the repository instructions and structural checks explicit so
  reviews that read repo guidance know the required dependency-review evidence.
- Codex review remains advisory. The deterministic merge gate is still
  `cargo xtask ci` plus branch protection.
