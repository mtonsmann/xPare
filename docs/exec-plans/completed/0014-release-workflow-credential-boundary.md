# Release Workflow Credential Boundary

## Change Class

- Security / privacy posture
- Dependency / CI

## Intended Behavior

The official release workflow keeps Apple Developer ID and notary credentials
available only for native signing and notarization. After `make dist`, the
workflow captures a manifest over every signed release asset that will be
published or attested, cleans all Apple material fail-closed, then runs
trusted manifest verification and publication before any post-signing
third-party metadata action can run. Any signed-asset drift before publication
fails the release. The signed-manifest baseline is also bound to a prior step
output digest, so a later action cannot rewrite both the manifest file and the
assets to satisfy the diff.

## Must-Preserve Invariants

- No new network, persistence, logging, or entitlement posture.
- GitHub Actions remain pinned and least-privilege.
- `cargo xtask ci` remains the canonical gate.
- The release job must fail closed before post-signing `uses:` actions if Apple
  signing or notary material cannot be removed from the temporary keychain.
- Third-party metadata actions must not be in the trusted path from signed asset
  capture to GitHub Release publication.
- Third-party metadata actions must not run with release-asset write permission.
- Draft GitHub Releases must not become a durable handoff point for required
  metadata: if required post-publication metadata fails, the incomplete draft
  must be deleted before a maintainer can publish it.

## New Invariants

- The protected signed-release manifest includes `*.zip`, `*.zip.sha256`, and
  `SHA256SUMS*`.
- Notary credentials are stored in and consumed from the temporary keychain.
- No action step, including short-form `- uses: owner/action@sha`, can run inside
  the Apple credential window; optional YAML whitespace around `uses :` is
  normalized before matching.
- Cleanup must delete the temporary keychain without swallowing real failures
  before any post-signing third-party action.
- No `uses:` action may run after signed assets are captured and before
  `make github-release` publishes them.
- Required release workflow step names must be unique and must come from actual
  workflow step entries, not comments or run-script text.
- Notary keychain enforcement must inspect the actual continued `notarytool
  store-credentials` command, not adjacent comments or echoed strings.
- The signed-release manifest baseline must be digest-bound through the
  `signed_manifest` step output before post-signing actions run, and later
  verification steps must validate that binding before diffing current assets.
- YAML action-key matching must normalize simple quoted keys such as `'uses'`
  and `"uses"` so equivalent action syntax cannot bypass the boundary guard.
- Provenance attestation and SBOM generation must run in jobs without
  `contents: write`; the later SBOM attachment job may have release write only
  if it contains no `uses:` actions and uploads the fixed SBOM workflow artifact.
- Provenance attestation must use the signed-release checksum subject list
  captured before publication; it must not download assets from the draft
  release or require draft-visible release credentials.
- Any run-only release-write metadata job must set explicit repository context
  for `gh` (`GH_REPO` or an equivalent `--repo`) because there may be no checkout
  remote in that job.
- The incomplete-draft cleanup job may delete only a still-draft release and must
  never delete the tag.

## Threats / Bug Classes Considered

- A compromised post-signing action mutates release checksums or signed assets.
- A compromised post-signing action rewrites the signed-manifest baseline and
  the signed assets together before later verification runs.
- A compromised post-signing action mutates `$GITHUB_PATH`, `$GITHUB_ENV`,
  trusted tools, workspace files, or release scripts before manifest
  verification and `make github-release` run.
- A compromised post-publication metadata action uses a job-level release write
  token to clobber already-verified draft release assets.
- A required metadata job fails after `make github-release` creates a draft,
  leaving a maintainer-visible draft missing provenance or SBOM metadata.
- A metadata job without checkout lacks repository context, so `gh release`
  commands fail after the draft has already been created.
- A read-only metadata job tries to download a draft release, which requires
  draft-visible release access and reintroduces release-write token pressure.
- A notary profile is accidentally stored in the login keychain and survives
  temporary keychain deletion.
- A short-form action step bypasses the release workflow structural guard.
- Cleanup failures are ignored and leave signing material available to later
  third-party actions.

## Test Plan

- Update `xtask` release workflow guard tests for short-form `uses:` actions,
  missing per-zip checksum manifest coverage, missing notary temp-keychain
  binding, fail-open cleanup, missing signed-manifest baseline binding, quoted
  YAML action keys, cleanup failure handlers, and post-signing actions before
  publication or with release-asset write permission.
- Add guard tests for checksum-subject attestation, no draft release download in
  attestation, explicit `gh` repository context in release-write metadata jobs,
  and fail-closed cleanup of incomplete draft releases.
- Keep the current workflow passing test as the happy path.

## Verification / Proof Plan

- `cargo test -p xtask` proves the focused structural guard behavior.
- `cargo run -p xtask -- check-workflows` proves the release workflow passes the
  project workflow gate plus actionlint/zizmor.
- `cargo fmt --all --check` and targeted clippy/test commands cover formatting
  and Rust hygiene for the changed `xtask` code.

## Decision Log

- Keep the release boundary guard in `xtask` rather than adding a YAML parser
  dependency; the invariant is order and presence of named workflow steps, and
  actionlint already owns YAML syntax. The structural guard still has to ignore
  comments, reject duplicate boundary steps, normalize YAML key spacing, and
  inspect continued shell commands where a specific argv matters.
- Treat `*.zip.sha256` as a signed-release metadata asset because users may
  download and trust it directly.
- Require notary profile access through the temp keychain on both storage and
  submission, so deleting the temp keychain is a real credential boundary.
- Use the GitHub Actions step-output context as the release-manifest binding
  point: the manifest file stays in runner temp storage for later diffs, while
  its SHA-256 digest is captured by the runner before post-signing actions can
  mutate workspace or temp files.
- Treat the repeated review findings as one root issue: trusted release
  invariants were being expressed as point-in-time checks after third-party
  actions had already regained control of the runner. The stronger boundary is
  to keep all third-party `uses:` actions out of the signed-asset capture to
  publication window.
- Split post-publication metadata work by authority instead of only by order:
  attestation and SBOM generation are allowed to execute third-party actions
  because they have no release-asset write permission; the SBOM attachment job
  has release write permission but only trusted `run:` steps.
- Do not use the draft release as the metadata handoff point. The signing job
  exports the protected checksum subject list through a job output, the
  attestation job attests that list without release write permission or draft
  downloads, and a separate run-only cleanup job deletes an incomplete draft if
  required metadata fails.

## Evidence Packet

- `cargo fmt --all` passed.
- `cargo fmt --all --check` passed.
- `cargo test -p xtask` passed: 91 tests.
- `cargo clippy -p xtask --all-targets -- -D warnings` passed.
- `cargo run -p xtask -- check-codeql-workflow-posture` passed: CodeQL remains
  additive, pinned, least-privilege, and custom packs are wired.
- `cargo run -p xtask -- check-release-posture` passed.
- `cargo run -p xtask -- check-shell` passed.
- `cargo run -p xtask -- check-workflows` passed: actionlint clean; zizmor
  offline reported no findings.
- `cargo xtask ci` passed through metadata, formatting, workspace clippy,
  workspace tests, docs, ABI, entitlement, release-posture, shell, workflow, and
  unused-dependency gates. The final cargo-deny step failed inside the sandbox
  because it could not lock `/Users/marcus/.cargo/advisory-dbs/db.lock` on a
  read-only path.
- `cargo xtask check-supply-chain` rerun with filesystem access passed:
  advisories, bans, licenses, and sources all ok.

## Proof Gaps

- The workflow guard is structural over this checked-in release workflow; it
  does not execute the macOS release job or prove GitHub runner behavior.
