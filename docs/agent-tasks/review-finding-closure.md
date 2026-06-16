# Agent task: review-finding closure

Prompt template for closing a class of issue surfaced by a security scan, code
review, fuzz run, CI failure, performance review, or a reference/production
mismatch. The rule: anything found once should be hard to introduce again.

## Files to read

- [`docs/agent-workflow.md`](../agent-workflow.md).
- [`docs/guardrails/agentic-security-finding-triage.md`](../guardrails/agentic-security-finding-triage.md)
  — read this first when the finding came from an agentic scanner, cloud reviewer,
  generated security-fix PR, or other agent-produced report.
- [`docs/guardrails/review-finding-closure.md`](../guardrails/review-finding-closure.md)
  — the authoritative closure rules; this file is the agent-facing checklist over it.
- The guardrail for the finding's change class, and the owning source/test file.

## Hard constraints

- Do **not** close a finding class with only a one-off fix.
- Do **not** treat an agent-generated finding title or suggested patch as the
  scope of work; validate the source, sink, control, boundary, and sibling class.
- The fix must enforce the invariant at the **owning boundary** (the lowest layer
  that owns the behavior), not in PR narrative or reviewer memory.
- Do not weaken an existing check to make the finding "go away".

## Implementation rules — a closure PR needs all four

1. **Name the issue class** (not just the instance): e.g. O(n²) transform behavior,
   resource-amplifying config, active content surviving sanitization, content
   reaching a log sink, broadened entitlement, ABI drift, fused-path divergence.
2. **Add the narrowest repeatable blocker** at the owning layer — pick the lowest
   practical one:
   - behavioral bug → regression test (+ integration test if it crossed a boundary);
   - parser/input bug → adversarial coverage + commit the crashing input under
     `fuzz/regressions/<target>/`;
   - performance bug → `core/tests/perf_guard.rs` complexity gate;
   - posture/boundary bug → an always-on `xtask` structural check;
   - dependency/workflow/script bug → `cargo-deny` / `zizmor` / `actionlint` /
     `shellcheck`, or a focused `xtask` assertion.
3. **Record the lesson** in the right doc: `SECURITY.md` (posture), `DESIGN.md`
   (settled decision/deferral), `ARCHITECTURE.md` (boundary/invariant), the focused
   `docs/guardrails/` file, `docs/performance.md`, or `docs/release-model.md`.
4. **Call it out in the PR**: the class, the mechanical blocker, the docs updated,
   and any residual proof gap.

## Required tests / checks

- Run the gate that proves the new blocker actually fails when the class returns
  (e.g. temporarily reintroduce the bug locally to confirm red, then revert), and
  `cargo xtask ci` green with the blocker in place.

## Required evidence

- The named class, the blocker layer + file, the doc updated, and the gate output.

## Proof gaps to report

- If a mechanical blocker is genuinely impractical, state why and add the strongest
  repeatable substitute (focused regression/corpus). Never leave the only protection
  in the PR text.
