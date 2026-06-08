# Execution Plan: Growth Envelope Review Closure

## Scope

Change class: FFI/config schema, macOS settings, review-finding closure.

Close the review finding class where a stricter current config schema can reject
older settings without clear version signaling or migration.

## Decisions

- Keep config schema v3 because the accepted config resource envelope intentionally
  tightened.
- Treat v3 as a semver-major event by bumping the workspace package version to 2.0.0.
- Normalize settings-derived free-text parameters before emitting current-schema JSON.
- Add mechanical protection at both layers: Rust package-major/schema consistency and
  Swift settings migration tests.

## Evidence

- Review finding class: config schema break without matching semver signal and shell
  migration at the settings-to-wire boundary.
- Regression protection: `config_schema_breaks_are_semver_major_events` in core tests
  plus `SettingsTests` coverage for persisted free-text normalization.
- Docs lesson: FFI guardrail, shell contract, release model, architecture table, and
  changelog all call out the version/migration invariant.
