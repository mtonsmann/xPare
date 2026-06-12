# Execution Plan: CodeQL Action Pin Comment Closure

## Change Class

- Dependency / CI

## Issue Class

GitHub Actions workflow pins can use an annotated tag object SHA while the
version comment names the action release. GitHub Actions resolves the ref, but
zizmor/GitHub Advanced Security reports `ref-version-mismatch`, and the local
offline workflow gate did not block the mistake.

## Goal

Resolve the PR #43 GitHub Advanced Security comments for CodeQL action pins and
add a repeatable local blocker so future CodeQL pin updates use the peeled commit
SHA with an exact release-version comment.

## Decision Log

- 2026-06-12: Keep CodeQL additive and workflow-local; do not change branch
  protection or required checks.
- 2026-06-12: Pin `github/codeql-action/*` to the peeled commit behind
  `v4.36.2`, `8aad20d150bbac5944a9f9d289da16a4b0d87c1e`, not the annotated tag
  object.
- 2026-06-12: Make `xtask` require the exact `# v4.36.2` comment and reject the
  known annotated tag object SHAs that caused the finding.

## Must-Preserve Invariants

- CodeQL remains additive, not a required release or PR gate.
- Workflow actions stay SHA-pinned with version comments.
- Workflow permissions remain least-privilege.
- No core, ABI, shell runtime, or privacy/data-handling behavior changes.

## Verification Plan

- `cargo fmt --all --check`
- `cargo test -p xtask codeql_workflow_posture`
- `cargo run -p xtask -- check-codeql-workflow-posture`
- `cargo run -p xtask -- check-workflows`
- `cargo run -p xtask -- ci`

## Performance Plan

Not applicable: workflow posture and docs only; no runtime behavior changes.

## Evidence Packet

- `gh api /repos/github/codeql-action/git/ref/tags/v4` and
  `/git/tags/411bbbe57033eedfc1a82d68c01345aa96c737d7` confirmed that the old
  pin was the annotated `v4` tag object and that it peels to commit
  `8aad20d150bbac5944a9f9d289da16a4b0d87c1e`.
- `gh api /repos/github/codeql-action/git/ref/tags/v4.36.2` and
  `/git/tags/1a818fd5f97ed0ee9a823421bd5b171add01227f` confirmed that
  `v4.36.2` also peels to commit
  `8aad20d150bbac5944a9f9d289da16a4b0d87c1e`.
- `cargo fmt --all --check` -> pass.
- `cargo test -p xtask codeql_workflow_posture` -> pass; includes
  `codeql_workflow_posture_rejects_annotated_tag_object_pin`.
- `cargo run -p xtask -- check-codeql-workflow-posture` -> pass.
- `cargo run -p xtask -- check-workflows` -> pass.
- First `cargo run -p xtask -- ci` reached `cargo-deny` and failed only because
  the sandbox could not acquire `/Users/marcus/.cargo/advisory-dbs/db.lock`.
- Escalated `cargo run -p xtask -- ci` -> pass.

## Proof Gaps

- The local offline check rejects this known class for the CodeQL pins but does
  not query GitHub for arbitrary future annotated tag objects.
