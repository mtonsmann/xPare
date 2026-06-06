# Exec Plan 0011 - release fuzz gate

Status: **completed**. Started: 2026-06-06. Completed: 2026-06-06.

## Goal

Make in-depth fuzzing a mechanical release prerequisite on the exact release
candidate SHA, while keeping pull-request fuzzing honest but lightweight.

## Change class

Dependencies, CI, and release automation. This plan does not change the C ABI,
core transform behavior, shell clipboard handling, entitlements, or data
retention/privacy posture.

## Scope

- Add a manually dispatched GitHub Actions workflow that runs `cargo xtask
  check-fuzz` with an explicit per-target time budget and uploads fuzz artifacts.
- Add a release workflow gate that fails a tagged release unless the same commit
  SHA has a successful in-depth fuzz workflow run.
- Remove the duplicate standalone PR `zizmor` job; keep workflow security linting
  inside the canonical `cargo xtask ci` path.
- Update the release/dependency docs so the required release-fuzz evidence is
  visible to future maintainers.

## Out of scope

- Weekly or nightly fuzz campaigns.
- A new fuzz engine, corpus minimization service, or paid/larger GitHub runner.
- Branch-protection changes outside the repository.

## Work plan

1. Add the active exec plan. **Done.**
2. Remove the duplicate CI `zizmor` job and adjust comments/docs that mention it.
   **Done.**
3. Add a manual `Release Fuzz` workflow on `ubuntu-latest`. **Done.**
4. Gate `.github/workflows/release.yml` on a successful `Release Fuzz` run for
   `${{ github.sha }}`. **Done.**
5. Update `CONTRIBUTING.md`, `docs/release-model.md`, and the dependency guardrail.
   **Done.**
6. Run workflow linting, xtask tests/lints, and the full local gate where feasible.
   **Done.**

## Decision log

- 2026-06-06: Keep PR fuzz smoke as best-effort harness health. In-depth fuzzing is
  most valuable as a pre-release gate because releases are sparse and should carry
  stronger evidence than ordinary PRs.
- 2026-06-06: Require evidence by commit SHA, not by tag name. If the final release
  tag points at a different commit than the RC run, the release must fail and fuzz
  must be rerun on the new SHA.
- 2026-06-06: Use standard `ubuntu-latest` runners and manual dispatch to avoid
  recurring free-plan churn and macOS runner cost/noise.
- 2026-06-06: Keep `zizmor` in `cargo xtask ci` as the required PR signal. The
  standalone `zizmor` job duplicates that gate and makes PR status noisier.

## Acceptance criteria

- PR CI has one workflow-security gate path: `xtask ci` -> `check-workflows`.
- A maintainer can run release fuzz on an RC ref with a configurable budget.
- A tagged release fails unless a successful release-fuzz run exists for the exact
  tag commit SHA.
- `cargo run -p xtask -- check-workflows` passes.
- `cargo test -p xtask` and `cargo clippy -p xtask --all-targets -- -D warnings`
  pass when `xtask` changes.

## Verification

- `cargo run -p xtask -- check-workflows`
- `cargo fmt --all --check`
- `cargo test -p xtask`
- `cargo clippy -p xtask --all-targets -- -D warnings`
- `make ci`
- `make zizmor`
