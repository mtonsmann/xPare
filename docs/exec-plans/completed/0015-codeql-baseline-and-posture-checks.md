# 0015 - CodeQL baseline and deterministic posture checks

**Status:** completed

## Goal

Enable built-in CodeQL as an additive security review layer and add the
low-noise deterministic `xtask` checks identified by the CodeQL research pass.

## Change class

Dependency / CI. This changes GitHub Actions, `xtask`, and docs only. No runtime
transform behavior, FFI ABI surface, entitlement, or clipboard privacy posture
change is intended.

## Scope

- Add a pinned CodeQL workflow using `security-extended`.
- Keep CodeQL separate from `cargo xtask ci` and avoid branch-protection changes.
- Add exact `xtask` checks for Swift no-network APIs, shipped command execution,
  SwiftPM dependency drift, Python helper posture, real clipboard test bans, and
  pasteboard write shape.
- Add a CodeQL workflow posture check once the workflow exists.
- Update architecture/contributing/guardrail docs to describe the new signals.

## Out of scope

- Custom CodeQL query packs or QL source.
- Making CodeQL a required branch-protection check.
- Replacing actionlint, zizmor, cargo-deny, Miri, Kani, fuzzing, or any existing
  structural check.
- Adding dependencies, entitlements, network access, persistence, telemetry, or
  logging.

## Decision log

- Use built-in CodeQL `security-extended`, not `security-and-quality`; this repo
  already has strong quality gates and low tolerance for noisy required checks.
- Analyze Rust with `build-mode: none`; current Rust source has no generated code
  that requires build capture.
- Analyze Swift on macOS and build the Rust FFI staticlib before SwiftPM build so
  CodeQL sees the shell compile path.
- Analyze Python and GitHub Actions workflow files in a separate no-build job.
- Pin `github/codeql-action@v4.36.2` by peeled release commit SHA
  `8aad20d150bbac5944a9f9d289da16a4b0d87c1e`. The earlier
  `411bbbe57033eedfc1a82d68c01345aa96c737d7` value was the annotated `v4` tag
  object, not the commit, and was closed in
  `0016-codeql-action-pin-comment-closure.md`.
- Keep repo-specific invariants in `xtask`, where failures are deterministic and
  remediated locally.
- Keep `security-events: write` at job scope, not workflow scope, so `zizmor`
  accepts the workflow as least-privilege.

## Evidence packet

- `cargo fmt --all --check` passed.
- `cargo test -p xtask` passed: 62 tests.
- `cargo run -p xtask -- check-swift-no-network-apis` passed: scanned 12 shipped
  Swift files.
- `cargo run -p xtask -- check-shipped-command-exec` passed: scanned 12 Swift and
  17 Rust shipped files.
- `cargo run -p xtask -- check-swift-package-deps` passed.
- `cargo run -p xtask -- check-python-tooling-posture` passed: scanned 1 Python
  helper.
- `cargo run -p xtask -- check-real-clipboard-tests` passed: scanned 13 Swift test
  files.
- `cargo run -p xtask -- check-pasteboard-write-shape` passed.
- `cargo run -p xtask -- check-codeql-workflow-posture` passed.
- `cargo run -p xtask -- check-workflows` passed: `actionlint` clean and
  `zizmor --offline .github/workflows` reported no findings.
- First sandboxed `cargo run --locked -p xtask -- ci` passed through tests, structural
  checks, shellcheck, actionlint, and offline zizmor, then failed only because
  `cargo-deny` could not lock `/Users/marcus/.cargo/advisory-dbs/db.lock` from the
  restricted filesystem.
- Escalated `cargo run --locked -p xtask -- ci` passed end-to-end. `cargo-deny` emitted the
  existing informational warnings about unmatched license allowances and duplicate
  `wit-bindgen`, then reported `advisories ok, bans ok, licenses ok, sources ok`.

## Outcome

CodeQL is now enabled as additive, SHA-pinned code scanning over Rust, Python,
and GitHub Actions workflows. Swift CodeQL is deferred until the extractor
completes reliably in CI; deterministic local `xtask` checks continue covering
the Swift privacy/posture invariants that should not wait for CodeQL alerts or
review.
