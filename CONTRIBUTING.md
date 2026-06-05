# Contributing to SafetyStrip

SafetyStrip is a memory-safe, plain-text clipboard utility: a pure Rust
transformation **core** driven by native **shells**. A small set of invariants
(no `unsafe` in the core, a frozen C ABI, no network anywhere, no OS/IO/network
dependencies in the core, deterministic output, minimal macOS entitlements) is
enforced **mechanically** by the `xtask` crate so the same checks run locally
and in CI. See `AGENTS.md` and `docs/guardrails/` for the rationale.

## The full local gate

```sh
cargo xtask ci
```

This is the single source of truth and exactly what CI runs. In fail-fast order
it runs:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo xtask check-unsafe-forbid` — core still declares `#![forbid(unsafe_code)]`
5. `cargo xtask check-core-deps` — core's transitive dep tree is on a strict allowlist
6. `cargo xtask check-no-network` — no network/OS-capable crate anywhere in the tree
7. `cargo xtask check-abi` — the checked-in C header matches the FFI source
8. `cargo xtask check-entitlements` — the macOS entitlements file is minimal

If any check fails, it prints a remediation-oriented message. **Fix the code so
the check passes; do not weaken the check.** Each check can also be run on its
own (e.g. `cargo xtask check-abi`) for a fast inner loop.

> The first three steps shell out to `cargo`. Make sure the pinned toolchain
> (`rust-toolchain.toml`, stable + `clippy` + `rustfmt`) is installed.

### `make` shortcuts (optional)

A root `Makefile` wraps the common commands for convenience — `make help` lists
them. It is a thin layer that **delegates** to the canonical commands (so there is
no second source of truth): `make ci` is exactly `cargo xtask ci`, `make checks`
runs the structural checks, `make bench` runs the benchmarks, `make app`/`make run`
build/launch the macOS shell. Use them or the underlying commands interchangeably.

## Run checks that match the risk of the change

You do not have to run the full gate for every edit, but you must run everything
relevant to your change class (the same classes as `AGENTS.md`). Run
`cargo xtask ci` before opening a PR. If you skip a relevant check, say why in
the PR.

| Change class | What you touched | Run at minimum |
|---|---|---|
| **Core transform** | `core/` transform logic, ops, pipeline | `cargo test -p safetystrip-core`, `cargo clippy -p safetystrip-core --all-targets -- -D warnings`, `cargo fmt`, and the relevant fuzz target (below). New behavior needs regression **and** adversarial-input tests; output must stay deterministic. |
| **FFI boundary / ABI** | `core-ffi/` (incl. `cbindgen.toml`), config serialization, capabilities/version | `cargo xtask check-abi`, `cargo test -p safetystrip-ffi`. An intended ABI change means: bump `SS_ABI_VERSION`, run `cargo xtask gen-header`, and call it out in the PR (confirm a non-Swift shell could still consume the boundary). Adding a transform must **not** change the ABI. |
| **Shell** | `shells/macos/` (Swift), reserved `windows/`/`linux/` | `cargo build -p safetystrip-ffi --release` then `swift build --package-path shells/macos`. Touching entitlements → `cargo xtask check-entitlements`. No transform logic belongs in a shell. |
| **Security / privacy posture** | entitlements, logging, data paths, anything network-adjacent | `cargo xtask check-no-network`, `cargo xtask check-entitlements`, `cargo xtask check-unsafe-forbid`. Any new entitlement, network-capable dependency, or data path is a posture change — justify it in the PR and update `SECURITY.md`. |
| **Dependencies & CI** | crate versions, `Cargo.toml`, lints, `xtask`, `.github/workflows/` | `cargo xtask check-core-deps`, `cargo xtask check-no-network`, plus `cargo test -p xtask` / `cargo clippy -p xtask --all-targets -- -D warnings` when editing `xtask`. New crates: prefer boring, audited, API-stable ones; a new core dependency must be a pure-data crate (no OS/IO/net) and added to the `xtask` allowlist with justification. |
| **Docs only** | `README`, `ARCHITECTURE.md`, `DESIGN.md`, `docs/`, runbooks | `cargo fmt --all --check` (still run the formatter). Other checks may be skipped if the PR explains why. |

## Fuzzing (never-panics invariant)

The core feeds arbitrary, possibly adversarial bytes through hand-rolled
parsers, so the strippers and the full pipeline are fuzzed to prove they never
panic or hang. `fuzz/` is its own workspace (libFuzzer + the nightly-only
toolchain stay out of normal stable builds), so it needs nightly and
`cargo-fuzz`:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz            # once

# Run a target (Ctrl-C to stop, or bound it with a time budget):
cargo +nightly fuzz run <target>
cargo +nightly fuzz run strip_html -- -max_total_time=60
```

Available targets: `strip_html`, `strip_markdown`, `transform_pipeline`. Run the
target(s) covering any core transform you change, and commit any new crashing
input found under `fuzz/` so it is replayed as a regression. CI runs a short
best-effort nightly fuzz smoke (`continue-on-error`); the required signal is the
property/corpus tests in `cargo xtask ci`.

## Pull requests

- State the change class and any compatibility/posture impact (ABI, privacy,
  entitlements, supported transforms).
- Keep diffs narrow and single-purpose; don't mix transform logic, ABI changes,
  shell code, dependency posture, and formatting.
- Update the relevant guardrail and `ARCHITECTURE.md` when an invariant or the
  boundary moves.
- Make sure `cargo xtask ci` is green.
