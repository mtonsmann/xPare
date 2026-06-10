# Security Finding Fixes

## Change Classes

- Core transform correctness and adversarial input.
- FFI memory hygiene without ABI changes.
- macOS shell clipboard safety and release posture.
- Security/privacy documentation.

## Scope

- Fix all six reportable findings from the Deep Codex Security scan:
  - release signing can omit App Sandbox entitlements,
  - Markdown newline coalescing can be superlinear,
  - HTML sources can run Markdown stripping before HTML stripping,
  - rich pasteboard data can be parsed before the shell size ceiling,
  - continuous mode can reprocess xPare self-writes,
  - stale asynchronous transforms can overwrite newer clipboard contents.
- Address the recommended documentation follow-ups, including local
  pasteboard-writer assumptions, release sandbox posture, and the FFI
  invalid-UTF-8 zeroization limitation.
- Add focused regression coverage and run the relevant guardrails.
- Commit the fixes, then run a Codex Security diff scan over the resulting
  change set.

## Out Of Scope

- Changing the FFI ABI.
- Adding new dependencies.
- Adding network, persistence, logging, telemetry, Accessibility, Input
  Monitoring, or broad file entitlements.
- Broad UI redesign or unrelated refactors.

## Plan

1. Patch core Markdown newline coalescing and add adversarial complexity coverage.
2. Patch FFI lossy UTF-8 temporary zeroization or document any residual limitation.
3. Patch macOS pasteboard pre-cap checks, forced HTML-first config ordering, and
   continuous-mode generation/backpressure behavior.
4. Patch release signing to require and verify the checked App Sandbox
   entitlements on official Developer ID releases.
5. Update SECURITY, DESIGN, guardrails, release docs, and architecture notes where
   the trust boundaries and limitations are clarified.
6. Run focused Rust/Swift tests, formatter/lints/guardrails, then the full
   project gate where feasible.
7. Commit the fix set.
8. Run Codex Security diff scan and continue until the scan no longer reports the
   addressed findings or new regressions.

## Decision Log

- 2026-06-06: Treat the release entitlement mismatch as a fix, not merely a doc
  clarification: official Developer ID distribution should embed the checked
  App Sandbox entitlement unless a future PR explicitly changes posture.
- 2026-06-06: Keep the C ABI unchanged; any FFI memory hygiene change must stay
  internal to `xp_transform`.
- 2026-06-06: Use an incremental Markdown output state instead of a bounded
  reverse scan, preserving newline coalescing semantics while making structural
  newline insertion constant-time.
- 2026-06-06: Verification before commit: `cargo run -p xtask -- ci` passed,
  `./build.sh test` passed for the macOS shell, and release gates fail closed for
  missing signing entitlements, alternate entitlement paths, or missing Developer ID
  credentials. The final Codex Security diff scan runs after the fix commit, per the
  requested workflow.
