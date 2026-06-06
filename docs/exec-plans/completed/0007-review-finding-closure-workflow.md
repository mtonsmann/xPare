# Review Finding Closure Workflow

## Scope

Document the rule that security, correctness, and performance findings are not
closed by a one-off fix alone. Each issue class needs repeatable regression
protection plus a short human/agent-facing lesson in the relevant docs.

## Out of scope

- Adding new runtime checks beyond the guardrails already implemented in this PR.
- Reclassifying existing security findings.
- Changing the FFI ABI, entitlement posture, dependency posture, or transform
  behavior.

## Plan

1. Add a focused guardrail that defines finding-class closure.
2. Route AGENTS, CONTRIBUTING, SECURITY, DESIGN, and ARCHITECTURE to the new
   workflow without duplicating policy.
3. Run the relevant docs/check gate and move this plan to completed.

## Decision log

- 2026-06-06: Treat this as a reusable review workflow, not a security-only rule,
  because performance and ordinary correctness reviews can also uncover classes of
  regressions that need permanent mechanical blockers.
- 2026-06-06: Added `docs/guardrails/review-finding-closure.md` and routed the
  agent/contributor/security/design/architecture docs to it.
