# Release Workflow Credential Boundary

## Change Class

- Security / privacy posture
- Dependency / CI

## Intended Behavior

The official release workflow keeps Apple Developer ID and notary credentials
available only for native signing and notarization. After `make dist`, the
workflow captures a manifest over every signed release asset that will be
published or attested, cleans all Apple material fail-closed, verifies the
manifest binding, encrypts the signed-asset handoff, and uploads only the
encrypted handoff blob as a short-retention workflow artifact. Attestation and
SBOM generation finish before release creation; a run-only release-write job then
downloads artifacts by artifact ID, verifies and decrypts the signed handoff,
re-verifies the signed manifest, downloads the SBOM, and creates a complete draft
GitHub Release once. Any signed-asset drift before encrypted handoff or draft
creation fails the release. The signed-manifest baseline is bound to a prior
step-output digest, so a later action cannot rewrite both the manifest file and
the assets to satisfy the diff.

## Must-Preserve Invariants

- No new network, persistence, logging, or entitlement posture.
- GitHub Actions remain pinned and least-privilege.
- `cargo xtask ci` remains the canonical gate.
- The release job must fail closed before post-signing `uses:` actions if Apple
  signing or notary material cannot be removed from the temporary keychain.
- Third-party metadata actions must not be in the trusted path from signed asset
  capture to the digest-bound pre-artifact-upload verification.
- Third-party metadata actions must not run with release-asset write permission.
- Draft GitHub Releases must not become a durable handoff point for required
  metadata: required metadata must exist before the draft is created.
- Existing GitHub Releases are immutable from `github-release`: reruns must not
  refresh assets on drafts or published releases because a draft-status check
  before upload is racy with maintainer publication.

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
  digest-bound verification and encryption hand them off as a workflow artifact.
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
  `contents: write`; the later publication job may have release write only if it
  contains no `uses:` actions, downloads fixed artifact IDs, verifies and
  decrypts the signed handoff, re-verifies the signed manifest, and creates a
  complete draft once.
- The checksum-subject attestation job must not request artifact metadata write
  permission unless the workflow later publishes registry storage records.
- Provenance attestation must use the signed-release checksum subject list
  captured before publication; it must not download assets from the draft
  release or require draft-visible release credentials.
- Any run-only release-write job must set explicit repository context for `gh`
  (`GH_REPO` or an equivalent `--repo`) because there may be no checkout remote in
  that job.
- Release workflow and local release scripts must not contain release upload,
  clobber, release delete, release asset delete, or raw release-asset upload API
  paths in executable text.
- `github-release` must inspect whether a release already exists and fail before
  release creation if it does; the `github-release)` case must include the staged
  SBOM in the one-shot `gh release create` command.
- Raw signed release assets must not be uploaded as public workflow artifacts;
  only an encrypted handoff blob with one-day retention may cross that boundary.
- Publish jobs must not use same-run artifact-name downloads (`gh run download`
  on `${GITHUB_RUN_ID}`); they must download by `artifact-id` through the Actions
  artifact API.

## Threats / Bug Classes Considered

- A compromised post-signing action mutates release checksums or signed assets.
- A compromised post-signing action rewrites the signed-manifest baseline and
  the signed assets together before later verification runs.
- A compromised post-signing action mutates `$GITHUB_PATH`, `$GITHUB_ENV`,
  trusted tools, workspace files, or release scripts before manifest
  verification and artifact handoff run.
- A release-write metadata path clobbers already-verified draft or published
  release assets.
- A required metadata job fails after draft creation, leaving a
  maintainer-visible draft missing provenance or SBOM metadata.
- A metadata job without checkout lacks repository context, so `gh release`
  commands fail after the draft has already been created.
- A read-only metadata job tries to download a draft release, which requires
  draft-visible release access and reintroduces release-write token pressure.
- A release workflow rerun for an existing tag clobbers draft or public release
  assets, including the race where a maintainer publishes a draft between a
  draft-status check and asset upload.
- A public workflow artifact becomes a distributable endpoint for signed official
  binaries before attestation, SBOM, and human-reviewed draft publication finish.
- A same-run `gh run download` artifact-name lookup fails in the current release
  workflow even though an artifact ID download would succeed.
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
  attestation, explicit `gh` repository context in release-write jobs, existing
  release clobber prevention in `github-release`, commented/echoed guard
  bypasses, release mutation primitive bans, complete-SBOM publication,
  guard-after-create ordering mistakes, raw signed asset artifact uploads, and
  same-run artifact-name downloads, and excess attestation artifact metadata
  permission.
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
  invariants were being expressed as point-in-time checks around a mutable draft
  release staging area. The stronger boundary is to keep third-party `uses:`
  actions out of the signed-asset capture to verified artifact handoff window and
  to create the draft release only after all required metadata is available.
- Split metadata work by authority instead of only by order: attestation and
  SBOM generation may execute third-party actions because they have no
  release-asset write permission; the publication job has release write
  permission but only trusted `run:` steps.
- Do not use the draft release as the metadata handoff point. The signing job
  exports the protected checksum subject list through a job output, encrypts the
  signed-asset handoff before artifact upload, the attestation job attests that
  checksum list without release write permission or draft downloads, the SBOM is
  staged as a workflow artifact, and the publication job downloads by artifact ID,
  decrypts, and creates the complete draft once.
- Treat Actions artifacts in public repos as read-access surfaces, not private
  staging. Uploading raw signed official binaries there bypasses the intended
  complete-draft review point even if the later publish job fails.
- Treat release asset replacement as a new release event, not a rerun of the
  current one. `github-release` is not idempotent for existing releases; delete a
  draft before rerunning, or publish a new tag for a corrected public release.

## Evidence Packet

- `cargo fmt --all` passed.
- `cargo fmt --all --check` passed.
- `cargo test -p xtask` passed: 104 tests.
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
