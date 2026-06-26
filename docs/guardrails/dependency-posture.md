# Guardrail: dependency posture

**When to consult:** adding or changing a crate dependency or version range, editing
`Cargo.toml`/`Cargo.lock`, touching lints, the `xtask` checks, CI
(`.github/workflows/`), or structural tests.

xPare's safety and privacy guarantees are only as strong as its dependency
tree. A single transitive crate with network or broad OS capability would undermine
the whole posture. So dependencies are **boring, audited, API-stable, and
capability-constrained**, and the constraint is enforced mechanically.

## The rules

1. **Prefer boring, audited, API-stable crates.** Favor ubiquitous, well-reviewed
   libraries over novel ones. Justify every new dependency.
2. **The core's dependency tree is a tiny allowlist of pure-data crates.** The full
   transitive *normal-and-build* dependency closure of `xpare-core` must stay on
   `CORE_DEP_ALLOWLIST` in `xtask/src/main.rs`. Today that is: `serde` family +
   `serde_json` (config), `pulldown-cmark` (Markdown), the proc-macro toolchain
   `serde_derive` needs (`proc-macro2`, `quote`, `syn`, `unicode-ident`),
   pure formatting/data helpers (`itoa`, `zmij`, `memchr`,
   `bitflags`, `unicase`), and `zeroize` (best-effort wiping of clipboard-derived
   intermediates). **No OS, filesystem, or network crate may enter the core's
   tree.**
3. **No network/OS-capable crate anywhere in the workspace.** `NETWORK_BANLIST` in
   `xtask/src/main.rs` bans async runtimes, HTTP/TLS stacks, websocket/RPC libs, and
   the low-level socket/event-loop crates they build on — across the *entire* tree
   (core, core-ffi, cli, xtask), not just the core. This is the no-exfiltration
   backstop at the dependency level.
4. **Tooling/dev/fuzz deps are constrained too, but separated.** `cbindgen` is a
   build/tooling dep of `xtask`; `proptest` (property tests) and `criterion`
   (benchmarks; `default-features = false` to drop the heavy plotters/rayon extras)
   are dev-only; `libfuzzer-sys`/`arbitrary` live in the **separate `fuzz/` workspace**
   so libFuzzer and the nightly toolchain never leak into the stable build. None of
   these may be a *normal* dependency of the core: the `check-core-deps` **and**
   `check-no-network` closures follow normal **and build** dependency edges but skip
   **dev** deps, so e.g. `proptest` and `criterion` (and their larger trees) do not
   pollute or trip them — while a build-script dependency cannot smuggle capability
   past the check either.
5. **Pin and constrain.** Shared versions live in `[workspace.dependencies]`
   (`Cargo.toml`); `pulldown-cmark` uses `default-features = false` to drop the
   unused bundled-binary feature and keep the surface minimal.
6. **Keep mechanical updates separate from behavior changes.** A `cargo update` or a
   dependency bump goes in its own PR, not mixed with transform/ABI/shell changes.
7. **Fix the code, never weaken the check.** If `cargo update` adds a new transitive
   crate:
   - if it is genuinely **pure-data** (no OS/IO/net), add it to `CORE_DEP_ALLOWLIST`
     in its own PR with justification;
   - if it touches the OS/filesystem/network, the fix is to **drop the dependency
     that pulled it in** — never widen the allowlist to admit capability, and never
     remove a crate from the banlist to make a build pass.
8. **Pin GitHub Actions to commit SHAs; audit workflows with zizmor.** Every `uses:`
   in `.github/workflows/` is pinned to a full commit SHA (with a `# vX.Y.Z` comment)
   so a moved tag can't change what runs in CI, and checkouts set
   `persist-credentials: false`. [`zizmor`](https://docs.zizmor.sh) statically audits
   the workflows (unpinned actions, credential persistence, template injection,
   over-broad `GITHUB_TOKEN` permissions) through `cargo xtask ci`
   (`check-workflows`, which also runs `actionlint` for correctness), so an agent
   catches workflow issues locally before pushing; `.github/dependabot.yml` bumps
   the pinned SHAs so the pins don't rot. `check-dependabot-policy` keeps action
   updates ungrouped with a 7-day version-update cooldown, keeps Cargo routine
   version PRs disabled, and rejects Cargo cooldown/grouping that would delay or
   batch security-update PRs. The official release workflow has one extra
   project-specific invariant inside `check-workflows`: Apple signing/notary
   material may exist only around `make dist`; the notary profile must be stored
   in and consumed from the temporary keychain; cleanup must fail closed before
   any post-signing `uses:` action; and no third-party `uses:` action may run
   between signed-asset manifest capture and the digest-bound pre-handoff
   verification. The signed zip, per-zip checksum, and aggregate-checksum manifest
   must be re-verified before the encrypted handoff artifact is uploaded and
   again before the draft release is created. After encryption, raw signed files
   must be removed from `dist/release/` and absence-checked before the first
   post-signing third-party action runs; constraining an upload-artifact `path`
   is not enough because the action still executes on the same runner filesystem.
   The baseline manifest cannot stand alone as a mutable `$RUNNER_TEMP` file:
   bind it to a prior step output digest before later third-party actions run,
   and validate that binding before every asset comparison. That guard validates
   real workflow step keys, normalizes YAML key spelling used by action steps,
   validates the actual continued notarytool and release-create commands instead
   of accepting comments or adjacent prose as proof, rejects raw signed asset
   uploads as public workflow artifacts, rejects same-run `gh run download` name
   lookups, and rejects release upload, clobber, and unscoped delete primitives.
   The actions themselves are supply-chain just like crates — boring, audited,
   pinned, kept outside the signing credential window, and separated from release
   write permission. A draft GitHub Release is not a safe metadata handoff
   boundary: attestation must use the checksum subject list captured before
   publication, SBOM generation must run with read-only contents permission,
   checksum-only attestation must not request artifact metadata write permission,
   and the only release-write job must be run-only, download fixed artifact IDs,
   decrypt and re-verify the signed handoff, create the draft only after all
   metadata is ready, verify the resulting asset set, and delete only its own
   still-draft release if creation or verification fails. `github-release` must
   also create a complete draft once, include the staged SBOM, and fail closed for
   any existing release so it never races a maintainer publication while
   replacing release assets.
9. **Audit the supply chain and the non-Rust surface mechanically.**
   [`cargo-deny`](https://embarkstudios.github.io/cargo-deny/) (`deny.toml`) scans the
   whole dependency tree for RustSec advisories, yanked crates, license compliance (a
   permissive allowlist; `cbindgen`'s MPL-2.0 is a *scoped* exception, never allowed
   workspace-wide), and a crates.io-only source policy — the *known-vulnerability*
   layer the structural allowlist cannot provide. The shell scripts are linted with
   `shellcheck` (the release plumbing signs and notarizes, so a shell bug is a
   release-integrity bug). All of these run inside `cargo xtask ci`, so the one gate
   stays a complete superset of CI. Same posture rule: fix the dependency, or add a
   *scoped*, justified `ignore`/`exception` in `deny.toml` — never broaden the policy.
10. **Keep non-Rust shipped automation surfaces dependency-light too.** The Swift
    shell intentionally has no external SwiftPM packages; target dependencies must
    stay local. Python helper scripts must remain stdlib-only and capability-light:
    no network, subprocess, multiprocessing, or dynamic code execution. These are
    enforced with `check-swift-package-deps` and `check-python-tooling-posture`.
11. **CodeQL is additive, not a new required gate yet.** The CodeQL workflow is a
    GitHub code-scanning baseline using `security-extended` plus repo-specific
    Rust/Python policy packs. Keep its actions pinned to peeled release commit
    SHAs, not annotated tag object SHAs; each pin needs the exact release-version
    comment (`# vX.Y.Z`) that matches the peeled commit. Keep permissions minimal:
    `contents: read` plus job-scoped `security-events: write`, and keep custom
    packs wired only to their owning language jobs. Do not put CodeQL in branch
    protection until the alert baseline is triaged.

## Dependabot merge recommendations

Treat every dependency PR as a supply-chain review, even when the repository diff
is only a lockfile or a GitHub Actions SHA. A reviewer may recommend **merge** only
after it can state why the update reduces or preserves risk for xPare's actual
dependency surface. The recommendation is advisory evidence, not an automerge
instruction; never bypass `cargo xtask ci`, failed GitHub checks, or branch
protection to land a dependency update.

Classify the PR first:

- **Applicable vulnerability fix:** a CVE/RustSec/GitHub advisory affects code,
  workflow, or release tooling that xPare actually uses. Triage immediately and
  prefer fast merge once the deterministic gates pass and the update diff does
  not introduce a more serious supply-chain concern.
- **Non-applicable vulnerability fix:** the advisory is for an unused feature,
  unreachable target, dev-only path, or package capability xPare does not invoke.
  Do not merge only because the PR says "security"; weigh the reduced advisory
  noise against the new upstream code being admitted.
- **Routine version update:** no known vulnerability is fixed. Merge only when
  the update is narrow, low-risk, and useful for staying on maintained pins; close
  or defer churn that adds no xPare value.
- **New dependency or new action:** a posture change until proven otherwise.
  Require the normal new-dependency justification plus an explicit capability and
  maintainer-trust review.

For **GitHub Actions** updates, inspect the upstream action, not just this repo's
workflow diff:

- Keep action updates one action per PR unless a PR explicitly justifies the
  reviewability tradeoff. Dependabot's ungrouped default is one PR per dependency;
  an all-actions group or repository/org grouped-security-update setting can batch
  unrelated upstream diffs and should be treated as a dependency-posture decision.
- Verify the new SHA belongs to the action's repository and matches the same-line
  release/version comment. Full-length SHA pinning is the immutable execution
  boundary; tags and comments are review aids, not the thing that runs.
- Record the old and new upstream SHAs plus a compare URL or exact diff command.
  Summarize changed files and call out any action code, generated `dist/` bundle,
  Dockerfile, workflow, manifest, or install-script change.
- Look specifically for new network destinations, credential/token access,
  artifact upload/download behavior, shell expansion, subprocess execution,
  release-write behavior, or persistence. Any such change is a security-relevant
  workflow change, even if xPare's YAML only changed one SHA.
- Check whether xPare uses the changed feature path. For a manifest-only
  `taiki-e/install-action` update, for example, confirm whether the changed tool
  manifests include the tools xPare installs (`cargo-deny`, `zizmor`,
  `shellcheck`, `cargo-machete`, `cargo-llvm-cov`, `cargo-mutants`) or only tools
  xPare never requests.
- Treat maintainer/repository signals as probabilistic context: long-lived
  project, active maintenance, release cadence, signed/verified releases,
  issue/advisory history, and whether the action is widely used. Good reputation
  cannot override a bad diff; weak reputation can turn an otherwise small bump
  into a hold.

For **Rust crate** updates:

- Run the relevant `xtask` dependency checks and inspect `cargo metadata`/`cargo
  tree` output for new transitive crates, feature changes, build scripts, source
  changes, yanked versions, and license/source drift.
- If the update touches the core's normal/build dependency closure, every new
  transitive crate must remain pure-data and be justified before it enters
  `CORE_DEP_ALLOWLIST`.
- For advisory fixes, identify the vulnerable crate, affected version range,
  fixed version, and whether xPare reaches the vulnerable API or feature. If the
  vulnerable path is reachable, speed matters; if not, prefer the lowest-risk
  fixed version and do not broaden capability to silence an alert.
- When the code delta is large or the package is security-sensitive, use a
  source diff review (`cargo vet diff`/manual crate diff) before recommending
  merge. `cargo-vet` is a good future ratchet for recording audited crate deltas,
  but it should not become required until the existing tree has an intentional
  baseline.

A merge recommendation must include:

- **Decision:** `merge`, `hold`, or `close/defer`.
- **Applicability:** whether the update fixes an issue xPare can actually hit.
- **Identifiers:** old/new action SHAs and release comments, or old/new crate
  versions and advisory identifiers.
- **Review source:** compare URL, `cargo vet diff` command, crate source diff
  command, or equivalent reproducible source used for the upstream review.
- **Upstream delta:** what changed outside this repo, with special attention to
  executable code and generated bundles.
- **xPare usage path:** where xPare invokes the action/crate/tooling path, or a
  statement that the changed upstream path is unused by xPare.
- **Capability delta:** any new network, filesystem, OS, credential, artifact,
  subprocess, entitlement, or release-write behavior.
- **Trust signals:** maintainer/repository reputation and any negative signals
  found during review.
- **Checks:** exact local/GitHub checks inspected and their pass/fail state.

For automated reviewers, a no-findings review, approval, reaction, or thumbs-up
without this evidence is not a dependency-review recommendation. On a dependency
PR, missing recommendation evidence is itself a P1 review finding because the PR
cannot be evaluated against xPare's supply-chain convention without it.

Recommend **hold** when the upstream diff is too large to review in the current
turn, the action/crate gains a capability xPare does not need, the maintainer or
repository changed hands unexpectedly, a release contains unexplained generated
code or binaries, checks fail, or the PR batches unrelated dependencies. For
routine updates, a hold is not a failure; it is often the right answer when
fresh code offers less value than the supply-chain risk it introduces.

## How the checks work

- `check-core-deps` runs `cargo metadata`, walks `xpare-core`'s transitive
  **normal + build** dependency closure (dev deps excluded), and fails if any crate
  is not on `CORE_DEP_ALLOWLIST`.
- `check-no-network` walks the same normal + build closure of **every** workspace
  member and fails if any crate on `NETWORK_BANLIST` appears anywhere.
- `check-supply-chain` runs `cargo-deny check` (advisories + licenses + bans + sources)
  against `deny.toml`.
- `check-dependabot-policy` verifies `.github/dependabot.yml` keeps GitHub Actions
  bumps one dependency per PR with a 7-day cooldown, disables routine Cargo version
  PRs with `open-pull-requests-limit: 0`, and does not group or cooldown Cargo
  security-update PRs.
- `check-shell` runs `shellcheck` over every shell script; `check-workflows` runs
  `actionlint` (correctness) then `zizmor --offline` (security) over
  `.github/workflows/`.
- `check-codeql-workflow-posture` keeps the additive CodeQL workflow
  least-privilege and pins `github/codeql-action/*` to the audited peeled release
  commit with an exact version comment. This blocks the `ref-version-mismatch`
  class where an annotated tag object SHA is mistaken for the commit SHA.
- `check-swift-package-deps` rejects external SwiftPM package/product/binary/system
  dependency declarations in `shells/macos/Package.swift`.
- `check-python-tooling-posture` scans Python helpers for the small allowed stdlib
  import set, rejects network/process/dynamic-exec tokens, and syntax-checks them
  with `python3 -m py_compile`.
- `check-codeql-workflow-posture` verifies `.github/workflows/codeql.yml` stays
  additive, SHA-pinned, least-privilege, uses `security-extended`, keeps the
  Rust/Python custom query packs wired, and leaves GitHub Actions analysis on the
  built-in suite.
- `check-fuzz` is the optional fuzz/tooling gate: it installs the nightly toolchain
  and pinned `cargo-fuzz` on demand, discovers targets with `cargo fuzz list`, builds
  all targets, and smoke-runs them when `XP_FUZZ_SMOKE_SECONDS=N` is set. The
  manual Release Fuzz workflow uses this same path as the required pre-release
  in-depth fuzz gate.

These all print remediation-oriented messages that explain how to *fix* the violation,
not how to silence it.

## Enforcing checks

- `cargo xtask check-core-deps`
- `cargo xtask check-no-network`
- `cargo xtask check-supply-chain` (cargo-deny; auto-installs the pinned tool on first
  local use, pre-installed in CI)
- `cargo xtask check-dependabot-policy`
- `cargo xtask check-swift-package-deps`
- `cargo xtask check-python-tooling-posture`
- `cargo xtask check-codeql-workflow-posture`
- `cargo xtask check-shell` (shellcheck) and `cargo xtask check-workflows` (actionlint
  + zizmor). The cargo-installable tools auto-install; `shellcheck`/`actionlint` print
  a one-line install hint if missing. `make zizmor` delegates to this same workflow
  lint gate.
- `cargo xtask check-fuzz` (optional nightly fuzz gate; `make fuzz` delegates here).
  Tagged releases require a successful manual Release Fuzz workflow run on the
  exact release SHA before packaging.
- When editing `xtask` itself: `cargo test -p xtask`,
  `cargo clippy -p xtask --all-targets -- -D warnings`.
- All of the above are part of `cargo xtask ci` (the same command CI runs).

## What a PR must call out

- Every new dependency: what it is, why it is needed, that it is boring/audited, and
  that it carries no OS/IO/network capability.
- Any addition to `CORE_DEP_ALLOWLIST` (with the justification that it is pure-data)
  or any change to `NETWORK_BANLIST`.
- Any new SwiftPM package/product/binary/system dependency, or widening of the
  Python helper import/capability allowlist.
- Any change that makes CodeQL required, broadens its permissions, disconnects a
  custom query pack, moves actions back to tags, or changes the query suite.
- Anything pulling in `unsafe` or network capability — that is also a
  [memory-safety](memory-safety.md) / [privacy](privacy-and-data-handling.md)
  posture change.
