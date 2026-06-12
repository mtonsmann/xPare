# Agent task: dependency / CI change

Prompt template for crate/dependency ranges, lints, CI, structural checks, and
automation.

## Files to read

- [`docs/agent-workflow.md`](../agent-workflow.md).
- [`docs/guardrails/dependency-posture.md`](../guardrails/dependency-posture.md).
- `Cargo.toml` / `Cargo.lock`, `deny.toml`, `xtask/src/main.rs`,
  `.github/workflows/*`, `rust-toolchain.toml`, `shells/macos/Package.swift`,
  `shells/macos/*.py`.

## Hard constraints

- Favor boring, API-stable, well-audited crates. Justify any new dependency,
  especially anything pulling in `unsafe` or network capability.
- A new **core** dependency must be pure-data (no OS/IO/net) and added to
  `CORE_DEP_ALLOWLIST` in `xtask` with justification. Never widen the allowlist to
  admit a crate with OS/IO/network capability; never add a crate on `NETWORK_BANLIST`.
- Keep mechanical dependency/automation updates **separate** from behavior changes.
- The invariants (no-unsafe core, frozen ABI surface, no-network, core-has-no-OS-deps,
  determinism) stay enforced by `cargo xtask ci`. Pinned linter versions
  (`cargo-deny`, `zizmor`, `cargo-fuzz`) in `xtask` must move in lockstep with the CI
  install step.
- CodeQL is an additive GitHub code-scanning signal, not the required local gate.
  Keep its workflow SHA-pinned, least-privilege, on `security-extended`, keep
  repo-specific custom packs wired by language, and keep it out of branch
  protection until baseline triage.
- Fix the code to satisfy a check; do not weaken the check. A scoped `deny.toml`
  ignore/exception needs a documented risk decision and a reason.

## Implementation rules

- After any dependency/lockfile change, re-derive the core allowlist mentally from
  `cargo metadata` and update `CORE_DEP_ALLOWLIST` only for genuinely new pure-data
  transitive crates.
- When editing `xtask`, add unit tests in `xtask/src/main.rs` for new parsing/logic.

## Required tests / checks

- `cargo xtask check-core-deps`, `cargo xtask check-no-network`,
  `cargo xtask check-supply-chain` for any dependency/lockfile change.
- `cargo xtask check-swift-package-deps` for SwiftPM changes.
- `cargo xtask check-python-tooling-posture` for Python helper changes.
- `cargo xtask check-workflows` and `cargo xtask check-codeql-workflow-posture` for
  any workflow change.
- `cargo test -p xtask` / `cargo clippy -p xtask --all-targets -- -D warnings` when
  editing `xtask`.

## Required evidence

- The full `cargo xtask ci` run (it is a superset of CI).
- For a new dependency: the justification, its capability surface, and confirmation
  it is on the right allowlist (and not on a banlist).

## Proof gaps to report

- Supply-chain auditing is advisory-DB + license + ban + source policy at a point in
  time, not a guarantee of crate correctness or absence of latent vulnerabilities.
