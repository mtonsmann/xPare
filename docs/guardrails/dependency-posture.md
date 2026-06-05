# Guardrail: dependency posture

**When to consult:** adding or changing a crate dependency or version range, editing
`Cargo.toml`/`Cargo.lock`, touching lints, the `xtask` checks, CI
(`.github/workflows/`), or structural tests.

SafetyStrip's safety and privacy guarantees are only as strong as its dependency
tree. A single transitive crate with network or broad OS capability would undermine
the whole posture. So dependencies are **boring, audited, API-stable, and
capability-constrained**, and the constraint is enforced mechanically.

## The rules

1. **Prefer boring, audited, API-stable crates.** Favor ubiquitous, well-reviewed
   libraries over novel ones. Justify every new dependency.
2. **The core's dependency tree is a tiny allowlist of pure-data crates.** The full
   transitive *normal* dependency closure of `safetystrip-core` must stay on
   `CORE_DEP_ALLOWLIST` in `xtask/src/main.rs`. Today that is: `serde` family +
   `serde_json` (config), `pulldown-cmark` (Markdown), the proc-macro toolchain
   `serde_derive` needs (`proc-macro2`, `quote`, `syn`, `unicode-ident`,
   `unicode-xid`), and pure formatting/data helpers (`itoa`, `ryu`, `zmij`, `memchr`,
   `bitflags`, `unicase`). **No OS, filesystem, or network crate may enter the core's
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
   `check-no-network` closures skip dev/build deps, so e.g. `proptest` and `criterion`
   (and their larger trees) do not pollute or trip them.
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
   over-broad `GITHUB_TOKEN` permissions) and **gates CI** via the `zizmor` job;
   `.github/dependabot.yml` bumps the pinned SHAs so the pins don't rot. The actions
   themselves are supply-chain just like crates — boring, audited, pinned.

## How the checks work

- `check-core-deps` runs `cargo metadata`, walks `safetystrip-core`'s transitive
  **normal** dependency closure (dev/build deps excluded), and fails if any crate is
  not on `CORE_DEP_ALLOWLIST`.
- `check-no-network` walks the closure of **every** workspace member and fails if any
  crate on `NETWORK_BANLIST` appears anywhere.

Both print remediation-oriented messages that explain how to *fix* the violation,
not how to silence it.

## Enforcing checks

- `cargo xtask check-core-deps`
- `cargo xtask check-no-network`
- `make zizmor` (workflow security; the `zizmor` CI job runs it on every push/PR).
- When editing `xtask` itself: `cargo test -p xtask`,
  `cargo clippy -p xtask --all-targets -- -D warnings`.
- All of the above are part of `cargo xtask ci` (the same command CI runs).

## What a PR must call out

- Every new dependency: what it is, why it is needed, that it is boring/audited, and
  that it carries no OS/IO/network capability.
- Any addition to `CORE_DEP_ALLOWLIST` (with the justification that it is pure-data)
  or any change to `NETWORK_BANLIST`.
- Anything pulling in `unsafe` or network capability — that is also a
  [memory-safety](memory-safety.md) / [privacy](privacy-and-data-handling.md)
  posture change.
