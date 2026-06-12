# Execution Plan: Main Dev Preview Artifact

## Change Class

- Dependency / CI

## Goal

Publish a developer-only macOS preview artifact for every protected `main` push
after the required portable gate succeeds, without creating a public release
channel or requiring Apple signing/notary secrets.

## Decision Log

- 2026-06-12: Use the existing `make preview` path so the artifact matches tag
  validation packaging and remains explicitly unsigned/ad-hoc.
- 2026-06-12: Put the job in `ci.yml` with `needs: xtask-ci` and an
  `if: push to refs/heads/main` guard. PRs do not upload runnable app artifacts.
- 2026-06-12: Upload as a short-retention Actions artifact named with the main
  commit SHA, not as a GitHub Release asset and not as `latest`.
- 2026-06-12: Keep official user downloads on tagged Developer ID releases.

## Must-Preserve Invariants

- No PR or fork path receives signing/notary credentials.
- No GitHub Release asset is produced from `main`.
- Official release signing remains gated by `release.yml` and the checked App
  Sandbox entitlements.
- The core ABI, transform behavior, privacy posture, and deterministic output are
  unchanged.

## Implementation Plan

1. Add a `main-preview` job to `.github/workflows/ci.yml`.
2. Gate it on successful `xtask-ci` and `push` events to `refs/heads/main`.
3. Build with `make preview VERSION=0.0.0-main.<short-sha>`.
4. Upload the zip and checksum with a 14-day retention.
5. Document the dev-preview channel in `docs/release-model.md`.

## Verification Plan

- `cargo fmt --all --check`
- `cargo run -p xtask -- check-workflows`
- `cargo run -p xtask -- check-release-posture`

## Performance Plan

Not applicable: this adds CI packaging/distribution automation only and does not
change runtime behavior.

## Evidence Packet

- `cargo fmt --all --check` -> pass.
- Initial `cargo run -p xtask -- check-release-posture` / `check-workflows`
  exposed a stale `target/debug/xtask` binary compiled with the old
  `/Users/marcus/Dev/SafetyStrip` checkout path.
- `cargo clean -p xtask` -> removed the stale `xtask` build artifact.
- `cargo run -p xtask -- check-release-posture` -> pass; official signing path
  still rejects alternate entitlements and verifies minimal signed payloads.
- `cargo run -p xtask -- check-workflows` -> pass; `actionlint` and offline
  `zizmor` reported no findings.
- After staging generated artifacts into a fixed upload directory,
  `cargo run -p xtask -- check-workflows` -> pass again.
- Final checks on clean branch `codex/main-dev-preview` based on `origin/main`:
  `cargo fmt --all --check` -> pass;
  `cargo run -p xtask -- check-release-posture` -> pass;
  `cargo run -p xtask -- check-workflows` -> pass.
- PR review follow-up: updated the staging globs and uploaded artifact name from
  stale `SafetyStrip-*` names to the current `xPare-*` release artifact names so
  the first protected-main preview run can find the generated zip and checksum.

## Proof Gaps

- The actual macOS packaging artifact is produced by GitHub-hosted macOS runners,
  not by Linux workflow linting.
