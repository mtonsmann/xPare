# Settings Config Envelope Review Closure

## Change class

Native shell / FFI config boundary / review-finding closure.

## Issue class

Settings-derived macOS configs can still be rejected by the v3 core schema when
normalization is per-field only or when a strip path bypasses the settings
export helper. The boundary invariant is stronger: every settings-derived
`config_json` emitted by the shell must be normalized for the current schema and
kept inside the whole-pipeline growth envelope before it crosses the FFI.

## Decision log

- Use a single Swift helper for current-schema operation normalization so
  `Settings.transformConfig`, `StripController.effectiveConfig`, and transient
  config construction cannot drift.
- Strip CR and LF before byte truncation so CRLF grapheme clusters cannot
  survive normalization.
- Mirror the core's small growth-factor table in the Swift shell boundary and
  clamp text parameters only as much as needed. Drop only a non-clampable
  growth operation that would exceed `MAX_PIPELINE_GROWTH_FACTOR`.
- Mirror the core's operation-count cap and add a linked-core acceptance test so
  Swift/Rust constant drift is caught by the macOS test lane.
- Add Swift regression tests that exercise settings export, the live strip
  path, transient `runOnce` config construction, CRLF normalization, operation
  count, and the aggregate growth cap.

## Validation

- `swift format lint --strict --recursive Sources Tests` passed.
- `swift build` passed.
- `cargo fmt --check` passed.
- `cargo test -p xpare-core --lib` passed.
- `git diff --check` passed.
- Local `swift test` was attempted and is blocked on this host by
  `no such module 'Testing'`; the CI macOS shell lane is the authoritative
  Swift test run for this branch.
- Subagent review found one real blocker (`maxOperations` mirrored as 64 instead
  of 32) and one test gap (`runOnce` normalization not pinned). Both were fixed
  before push.
