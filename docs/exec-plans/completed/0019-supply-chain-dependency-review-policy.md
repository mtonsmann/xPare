# Exec Plan 0019 - Supply-chain dependency review policy

Status: **completed** - Started: 2026-06-19 - Completed: 2026-06-19

## Goal

Make Dependabot dependency PRs easy for automated reviewers to classify and
recommend for merge without weakening xPare's supply-chain posture.

The immediate example is PR #56, a GitHub Actions SHA bump for
`taiki-e/install-action`. The repo diff is small, but the security-relevant diff
is the upstream action code and metadata between the old and new SHAs.

## Change Class

Dependency / CI and docs.

No transform-output change, FFI ABI change, entitlement change, product network
access, persistence, telemetry, or clipboard privacy-posture change.

## Scope

- Refine Dependabot config metadata for dependency PR routing.
- Add a deterministic guard so Dependabot routing cannot drift silently.
- Document a supply-chain-first merge recommendation rubric for dependency PRs.
- Teach dependency/CI agent prompts to inspect upstream diffs, maintainer trust,
  package capability, and CVE applicability before recommending merge.
- Add PR-template space for an explicit dependency merge recommendation when a
  PR touches dependencies or workflows.

## Out of Scope

- Adding new runtime dependencies.
- Automatically merging Dependabot PRs in GitHub Actions.
- Making CodeQL or dependency review a new required branch-protection gate.
- Adding `cargo-vet` as a required gate before the existing dependency tree is
  deliberately audited.
- Changing workflow action versions in this branch.

## Decision Log

- 2026-06-19: Keep `cargo xtask ci` as the required gate. Supply-chain review is
  advisory evidence, except for the deterministic checks that already fail CI.
- 2026-06-19: Ordinary dependency version updates should remain slower than
  security updates. Dependabot cooldown is appropriate for version updates, while
  CVE/security updates should bypass the cooldown and get immediate triage.
- 2026-06-19: Do not recommend merge from the repository diff alone. For GitHub
  Actions, compare the old and new upstream SHAs and inspect changed files for
  new network, credential, artifact, shell, or release-write behavior.
- 2026-06-19: Add `check-dependabot-policy` to keep the one-action-per-PR and
  Cargo-security-only routing policy mechanical instead of comment-only.
- 2026-06-19: PR #56's upstream compare is 30 commits ahead and changes only
  `CHANGELOG.md` plus install-action `manifests/*.json` files; the file list and
  whether any changed manifest is on xPare's installed-tool path is the kind of
  evidence the reviewer should report before recommending merge.

## Must-Preserve Invariants

- GitHub Actions stay pinned to full commit SHAs with version comments.
- Workflow permissions stay least-privilege.
- No dependency update may add network/OS capability to the shipped workspace.
- The core dependency allowlist and workspace network banlist stay authoritative.
- RustSec/cargo-deny advisory failures are treated as security findings, not
  routine version churn.

## Verification Plan

- `cargo fmt --all --check`
- `cargo test -p xtask`
- `cargo clippy -p xtask --all-targets -- -D warnings`
- `cargo run -p xtask -- check-agent-workflow`
- `cargo run -p xtask -- check-dependabot-policy`
- `cargo run -p xtask -- check-workflows`
- `git diff --check`

## Evidence Packet

- Researched current guidance before editing:
  - GitHub recommends full-length SHA pins for Actions and source-code review of
    actions handling repository content or secrets.
  - GitHub Dependabot `cooldown` applies only to version updates, not security
    updates.
  - GitHub Dependabot security updates are enabled in GitHub security settings;
    `dependabot.yml` can configure their behavior, and routine version updates can
    be disabled with `open-pull-requests-limit: 0`.
  - GitHub dependency review surfaces added/updated dependencies, release dates,
    dependent counts, and vulnerability data for PRs.
  - OpenSSF Scorecard treats pinned build/release dependencies and active
    maintenance as supply-chain risk signals, not proofs.
- Inspected PR #56 and the upstream compare through GitHub/GitHub API evidence:
  - Dependabot updates `taiki-e/install-action` from
    `59012be0884e296ca2da49b530610e72c49039ad` (`v2.81.6`) to
    `7a79fe8c3a13344501c80d99cae481c1c9085912` (`v2.81.10`).
  - Reproducible upstream source:
    `https://github.com/taiki-e/install-action/compare/59012be0884e296ca2da49b530610e72c49039ad...7a79fe8c3a13344501c80d99cae481c1c9085912`
  - The repo diff changes four workflow uses and no product code.
  - The upstream compare is 30 commits ahead and changes no action runtime code,
    generated bundle, Dockerfile, workflow, or install script.
  - Changed upstream files: `CHANGELOG.md`,
    `manifests/cargo-audit.json`, `manifests/cargo-binstall.json`,
    `manifests/cargo-shear.json`, `manifests/cosign.json`,
    `manifests/gungraun-runner.json`, `manifests/just.json`,
    `manifests/mise.json`, `manifests/parse-changelog.json`,
    `manifests/parse-dockerfile.json`, `manifests/rclone.json`,
    `manifests/release-plz.json`, `manifests/syft.json`,
    `manifests/tombi.json`, `manifests/uv.json`, `manifests/vacuum.json`,
    `manifests/wasm-bindgen.json`, and `manifests/wasmtime.json`.
  - The changed manifests do not include the tools xPare installs through the
    action today: `cargo-deny`, `zizmor`, `shellcheck`, `cargo-machete`,
    `cargo-llvm-cov`, and `cargo-mutants`.
- Updated `.github/dependabot.yml`:
  - Removed the all-actions group so action bumps stay one-action-per-PR.
  - Removed the no-op Cargo cooldown because Cargo routine version updates are
    disabled and security updates are intentionally undelayed.
- Added `cargo xtask check-dependabot-policy` and tests so CI rejects a
  reintroduced GitHub Actions group, Cargo cooldown, missing Cargo
  `open-pull-requests-limit: 0`, or multi-ecosystem grouping.
- Ran a pre-push adversarial subagent review. It found that the first pass was
  too comment-only, lacked reproducible upstream-review evidence fields, and had
  a Dependabot comment that could imply `dependabot.yml` alone enables security
  updates. The mechanical guard and docs above are the resulting fixes.
- Parent follow-up review of the subagent patch found one parser edge: a stray
  `default-days: 7` in the GitHub Actions block could satisfy the check even
  without a `cooldown:` key. Fixed it and added
  `dependabot_policy_rejects_actions_default_days_without_cooldown`.
- Updated the dependency guardrail with the supply-chain-first merge rubric.
- Updated `docs/agent-tasks/dependency-ci.md`, `AGENTS.md`, and the PR template
  so auto-reviewers know to produce `merge` / `hold` / `close/defer`
  recommendations with upstream-diff evidence.
- `git diff --check` passed.
- `cargo fmt --all --check` passed.
- `cargo test -p xtask dependabot_policy` passed: 6 tests.
- `cargo test -p xtask` passed: 118 tests.
- `cargo clippy -p xtask --all-targets -- -D warnings` passed.
- `cargo run -p xtask -- check-agent-workflow` passed.
- `cargo run -p xtask -- check-dependabot-policy` passed.
- `cargo run -p xtask -- check-workflows` passed: `actionlint` clean and
  `zizmor --offline .github/workflows` reported no findings.
- `ruby -e 'require "yaml"; YAML.load_file(".github/dependabot.yml")'` passed.
- `cargo run --locked -p xtask -- ci` passed metadata, fmt, clippy, workspace
  tests, structural checks including `check-dependabot-policy`, docs, ABI,
  entitlements, release posture, shellcheck, actionlint, zizmor, and
  cargo-machete, then failed only at `cargo-deny` because the sandbox could not
  lock `/Users/marcus/.cargo/advisory-dbs/db.lock`.
- Escalated `cargo run --locked -p xtask -- check-supply-chain` passed:
  advisories, bans, licenses, and sources ok. Existing warnings remained for
  unmatched license allowances and duplicate `wit-bindgen`.

## Proof Gaps

- A human/agent upstream diff review can reduce malicious-update risk but cannot
  prove an upstream dependency is non-malicious.
- Reputation and ecosystem signals are probabilistic. They must not override a
  concrete adverse code diff or a failed deterministic gate.
