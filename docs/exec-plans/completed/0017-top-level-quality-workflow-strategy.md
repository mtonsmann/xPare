# Exec Plan 0017 - Top-level quality workflow strategy

Status: **completed** - Started: 2026-06-15 - Completed: 2026-06-15

## Goal

Make the repository's GitHub Actions and docs express one cohesive verification
model:

- `CI` is the required deterministic merge gate.
- `Quality Hygiene` is the best-effort anti-slop lane across Rust and the macOS
  shell.
- Deep confidence/proof signals remain best-effort and outside the required gate.
- Scheduled workflows are only for external drift: advisories, ecosystem security
  posture, and analysis baselines that can change while the repo is quiet.

The workflow formerly named `Hygiene` was accurate mechanically but too narrow
semantically: it ran Rust coverage and mutation testing only. The implemented
shape makes the top-level model match the repo's Rust-core / native-shell
architecture without changing the required gate.

## Change Class

Dependency / CI and docs. Changed GitHub Actions and top-level/guardrail docs.

No transform-output change, FFI ABI change, entitlement change, product network
access, persistence, telemetry, or clipboard privacy-posture change.

## Scope

- Renamed the workflow display name from `Hygiene` to `Quality Hygiene`.
- Moved the best-effort macOS `check-swift` job from `CI` into
  `Quality Hygiene`.
- Expanded `Quality Hygiene` path filters so `xtask/**`, `Cargo.toml`, and
  `shells/macos/**` changes trigger the checks they can affect.
- Documented the Actions lane taxonomy in `CONTRIBUTING.md`.
- Updated `ARCHITECTURE.md` and guardrails so Rust coverage, Rust mutants, and
  Swift shell anti-slop are all described as best-effort `Quality Hygiene`
  signals outside `cargo xtask ci`.

## Out of Scope

- Making CodeQL, Swift shell anti-slop, Kani, Miri, fuzz, coverage, or mutation
  testing required branch-protection checks.
- Adding a cron to deterministic quality/proof jobs.
- Moving Miri or fuzz smoke out of `CI`. They remain best-effort immediate PR
  feedback; broadening `proofs.yml` path filters would make Kani run on unrelated
  code changes.
- Changing transform behavior, config schema, C ABI, entitlements, release
  signing, product privacy posture, or shell/core ownership.
- Adding dependencies or new GitHub permissions.

## Decision Log

- 2026-06-15: Keep `cargo xtask ci` as the single required portable gate. Quality
  evidence is advisory and best-effort.
- 2026-06-15: Prefer expanding `hygiene.yml` over merely renaming it, because the
  Swift shell needs visible anti-slop coverage under the same top-level quality
  strategy as Rust.
- 2026-06-15: Move `check-swift` from `CI` to `Quality Hygiene` instead of
  duplicating it. This keeps the Actions inventory clearer and avoids running the
  same macOS job twice on shell changes.
- 2026-06-15: Add `xtask/**` to the Quality Hygiene filters because `xtask` owns
  the coverage floor, mutation behavior, Swift coverage floor, and install/check
  plumbing.
- 2026-06-15: Leave Miri and fuzz smoke in `CI` for now. They are still
  best-effort, but moving them into `proofs.yml` would require broader workflow
  filters and would cause the heavy Kani job to run for changes it does not
  prove.
- 2026-06-15: Pre-PR subagent review found two documentation consistency issues:
  the `ARCHITECTURE.md` and hygiene guardrail intros still implied all listed
  checks fail `cargo xtask ci`, and the shell change-class row still pointed at
  the weaker `swift build` path. Fixed both before submission.

## Evidence Packet

- `git diff --check` passed.
- `cargo fmt --all --check` passed.
- `cargo run -p xtask -- check-workflows` passed: `actionlint` clean and
  `zizmor --offline .github/workflows` reported no findings.
- `cargo run -p xtask -- check-codeql-workflow-posture` passed.
- `cargo run --locked -p xtask -- ci` passed through metadata, fmt, clippy,
  workspace tests, docs, ABI, entitlements, release posture, shellcheck,
  actionlint, zizmor, and cargo-machete. It then failed only at `cargo-deny`
  because the sandbox could not lock `/Users/marcus/.cargo/advisory-dbs/db.lock`.
- Escalated `cargo run --locked -p xtask -- check-supply-chain` passed:
  advisories, bans, licenses, and sources ok. Existing informational warnings
  remained for unmatched license allowances and duplicate `wit-bindgen`.
- First sandboxed `cargo run -p xtask -- check-swift` passed swift-format and the
  Rust FFI release build, then failed at `swift test` because the sandbox could
  not write the Clang module cache.
- Escalated `cargo run -p xtask -- check-swift` passed: 126 Swift tests passed;
  Sources line coverage was 96.24% against the 95% floor; SwiftLint was not on
  PATH locally, matching the check's run-if-present behavior.
- Read-only review subagent reported two P3 doc consistency findings; both were
  fixed in this branch.
- After the subagent fixes, `git diff --check` and `cargo fmt --all --check`
  passed, and a stale-wording search found no remaining references to the old
  weaker shell check or "everything fails `cargo xtask ci`" wording.
- A post-fix `cargo run --locked -p xtask -- ci` rerun again passed through
  workspace tests, docs, structural checks, workflow linting, and cargo-machete,
  then hit the same sandbox-only advisory DB lock at `cargo-deny`; the escalated
  focused supply-chain check above is the authoritative result for that step.

## Outcome

The top-level strategy is now explicit and reflected in the workflow inventory:
`CI` remains the required gate, while `Quality Hygiene` owns cross-language
anti-slop evidence for Rust and the macOS shell. Required-vs-best-effort
boundaries and least-privilege workflow permissions are unchanged.
