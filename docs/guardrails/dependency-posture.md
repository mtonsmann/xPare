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
   the pinned SHAs so the pins don't rot. The official release workflow has one
   extra project-specific invariant inside `check-workflows`: Apple signing/notary
   material may exist only around `make dist`; the notary profile must be stored
   in and consumed from the temporary keychain; cleanup must fail closed before
   any post-signing `uses:` action; and no third-party `uses:` action may run
   between signed-asset manifest capture and `make github-release`. The signed
   zip, per-zip checksum, and aggregate-checksum manifest must be re-verified
   immediately before publication. The baseline manifest cannot stand alone as
   a mutable `$RUNNER_TEMP` file: bind it to a prior step output digest before
   later third-party actions run, and validate that binding before the
   pre-publication asset comparison. That guard validates real workflow step
   keys, normalizes YAML key spelling used by action steps, and validates the
   actual continued notarytool command instead of accepting comments or adjacent
   prose as proof. The actions themselves are supply-chain just like crates —
   boring, audited, pinned, and kept outside the signing credential window and
   the signed-asset publication window. Post-publication metadata actions must
   also be isolated from release-asset write permission: attestation and SBOM
   generation run in `contents: read` jobs, and the only later `contents: write`
   job is a run-only SBOM attachment step that downloads the fixed workflow
   artifact and uploads that one file.
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

## How the checks work

- `check-core-deps` runs `cargo metadata`, walks `xpare-core`'s transitive
  **normal + build** dependency closure (dev deps excluded), and fails if any crate
  is not on `CORE_DEP_ALLOWLIST`.
- `check-no-network` walks the same normal + build closure of **every** workspace
  member and fails if any crate on `NETWORK_BANLIST` appears anywhere.
- `check-supply-chain` runs `cargo-deny check` (advisories + licenses + bans + sources)
  against `deny.toml`.
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
