# Review Finding Closure

Use this guardrail when a security scan, code review, performance review, fuzz
run, CI failure, or manual audit finds a class of bug. The goal is simple:
anything found once should be hard to introduce again.

## The rule

Do not close a finding class with only a one-off fix. A closure PR needs:

1. **A concrete issue-class statement.** Name the class, not only the instance:
   stale async clipboard writes, duplicated sanitizer operations, broadened
   entitlements, accidental handwritten C surface, O(n^2) transform behavior,
   content reaching a log sink, and so on.
2. **Repeatable regression protection.** Add the narrowest test or check that
   would fail if the class came back. Prefer the lowest practical layer: unit
   test, property test, fuzz corpus replay, Swift integration test,
   `perf_guard`, `xtask` structural check, `cargo-deny`, `shellcheck`,
   `actionlint`, or `zizmor`.
3. **A human/agent-facing lesson.** Update the relevant guardrail or posture doc
   so future maintainers know why the check exists and where to extend it.
4. **A PR call-out.** State the issue class, the mechanical protection added,
   the docs updated, and any proof gap.

If a mechanical blocker is genuinely not practical, document why and add the
strongest repeatable substitute available. Do not leave the only protection in
the PR narrative or reviewer memory.

## Choosing the blocker

- **Behavioral bugs:** add a regression test at the smallest layer that owns the
  behavior, plus an integration test when the bug crossed a public or shell
  boundary.
- **Parser/input bugs:** add adversarial-input coverage, and commit any
  crashing or hanging fuzz input so it replays in the corpus.
- **Performance bugs:** add or update a complexity gate such as
  `core/tests/perf_guard.rs`. Throughput-only regressions belong in the measured
  benchmark flow in `docs/performance.md`, not as flaky absolute-speed CI gates.
- **Posture or boundary bugs:** add or extend an always-on structural check in
  `xtask` so `cargo xtask ci` fails before review has to notice the issue.
- **Dependency, workflow, or script bugs:** prefer existing mechanical tools:
  `cargo-deny`, `zizmor`, `actionlint`, `shellcheck`, or a focused `xtask`
  assertion when generic tools cannot express the project invariant.
- **Documentation-only findings:** update the relevant guardrail and add a small
  docs check only when the issue is mechanically expressible.

Not every finding requires a new `xtask` subcommand. Use `xtask` for structural
project invariants that ordinary tests or standard linters cannot enforce.

## Documentation updates

Place the lesson where the next change is likely to look:

- `SECURITY.md` for privacy, data handling, entitlement, or threat-model posture.
- `DESIGN.md` for settled decisions, deferred work, or why an alternative was
  rejected.
- `ARCHITECTURE.md` when a boundary, data flow, or enforced invariant changes.
- The focused file under `docs/guardrails/` for change-class rules.
- `docs/performance.md` for performance methodology or baseline interpretation.
- `docs/release-model.md` for release, signing, notarization, or distribution.
- `AGENTS.md` only as a short map to the durable docs.

The docs should explain the invariant and the enforcement point. They should not
copy long implementation details that belong in tests or `xtask`.

## Closure checklist

- The issue class is named in the PR.
- The vulnerable source, sink, or broken control is understood.
- The fix enforces the invariant at the owning boundary.
- A regression test, fuzz corpus input, structural check, or standard linter now
  blocks the class.
- The relevant guardrail/posture docs explain the lesson.
- `cargo xtask ci` passes, or the PR explains why a narrower verified gate was
  sufficient for the change class.
- Any remaining proof gap is explicit.
