# Security Regression Guardrails

## Scope

Add mechanical checks and focused tests that make the recent security-finding
classes harder to reintroduce:

- release signing must keep official entitlements minimal and reject alternate
  entitlement paths;
- the C/SwiftPM interop surface must stay tiny and header-only, with no
  handwritten C logic;
- HTML pasteboard snapshots must force `strip_html` before Markdown or reduction
  operations in both saved and transient pipelines;
- rich pasteboard representations must be size-checked before materialization;
- stale transform completions and continuous self-writes must stay covered across
  the shared controller path.

## Out Of Scope

- No ABI redesign.
- No replacement of the current C ABI with a generated Swift binding layer.
- No new dependency or new release-signing credential path.
- No real `.general` clipboard exercise in default tests.

## Decision Log

- 2026-06-06: Keep this on `codex/security-scan-fixes` because the guard tests
  enforce behavior introduced in that PR.
- 2026-06-06: Implement project-specific `xtask` checks rather than broad SAST;
  these bugs were invariant regressions, so exact mechanical assertions give
  better signal.
- 2026-06-06: Keep the C ABI but forbid growth of handwritten C surface. SwiftPM
  still needs a C target/header bridge, while the actual unsafe boundary remains
  Rust `core-ffi`.
- 2026-06-06: Completed with `check-release-posture`, `check-c-ffi-surface`,
  stricter exact-minimal source entitlement validation, Swift controller/pasteboard
  regression tests, and docs updates. Verification passed with `./build.sh test`
  and `cargo xtask ci`.

## Acceptance

- `cargo xtask check-release-posture` passes and fails closed if the release
  script permits alternate entitlement paths or skips minimal signed-entitlement
  verification.
- `cargo xtask check-c-ffi-surface` passes and fails if unexpected C/C++ source
  appears, if `dummy.c` gains code, or if the SwiftPM shim stops including the
  generated header as the single source of truth.
- Relevant Swift controller/pasteboard tests pass.
- `cargo xtask ci` passes.
