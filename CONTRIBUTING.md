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
7. `cargo xtask check-no-content-logging` — no shipped line logs/persists clipboard content
8. `cargo xtask check-pipeline-zeroization` — fused core scratch storage is wiped before release
9. `cargo xtask check-clipboard-safety` — default targets never touch the real clipboard
10. `cargo xtask check-c-ffi-surface` — no unexpected handwritten C/C++/Objective-C surface
11. `cargo xtask check-abi` — the checked-in C header matches the FFI source
12. `cargo xtask check-entitlements` — the macOS entitlements file is exactly minimal
13. `cargo xtask check-release-posture` — official signing cannot broaden entitlements
14. `cargo xtask check-shell` — `shellcheck` over the shell scripts
15. `cargo xtask check-workflows` — `actionlint` + `zizmor` over `.github/workflows/`
16. `cargo xtask check-supply-chain` — `cargo-deny`: advisories, licenses, bans, sources

If any check fails, it prints a remediation-oriented message. **Fix the code so
the check passes; do not weaken the check.** Each check can also be run on its
own (e.g. `cargo xtask check-abi`) for a fast inner loop.

> The first three steps shell out to `cargo`. Make sure the pinned toolchain
> (`rust-toolchain.toml`, stable + `clippy` + `rustfmt`) is installed.
>
> Steps 14–16 shell out to external linters. The cargo-installable ones
> (`cargo-deny`, `zizmor`) auto-install a pinned version on first use; the system
> tools (`shellcheck`, `actionlint`) print a one-line install command if missing.
> CI pre-installs all four (pinned), so a green `cargo xtask ci` locally means a
> green PR — there is no required check outside this one command. Optional fuzzing
> uses the same pattern through `cargo xtask check-fuzz`, which installs nightly and
> the pinned `cargo-fuzz` tool when a fresh agent is missing them. Releases add one
> stronger gate: the manual Release Fuzz workflow must pass on the exact release
> SHA before `.github/workflows/release.yml` will package the tag.

### `make` shortcuts (optional)

A root `Makefile` wraps the common commands for convenience — `make help` lists
them. It is a thin layer that **delegates** to the canonical commands (so there is
no second source of truth): `make ci` is exactly `cargo xtask ci`, `make checks`
runs the structural checks, `make bench` / `make bench-large` run the quick /
256 MB benchmarks, `make app`/`make run` build/launch the macOS shell. Use them or
the underlying commands interchangeably.

## Run checks that match the risk of the change

You do not have to run the full gate for every edit, but you must run everything
relevant to your change class (the same classes as `AGENTS.md`). Run
`cargo xtask ci` before opening a PR. If you skip a relevant check, say why in
the PR.

| Change class | What you touched | Run at minimum |
|---|---|---|
| **Core transform** | `core/` transform logic, ops, pipeline | `cargo test -p safetystrip-core`, `cargo clippy -p safetystrip-core --all-targets -- -D warnings`, `cargo fmt`, and the relevant fuzz target (below). New behavior needs regression **and** adversarial-input tests; output must stay deterministic. |
| **FFI boundary / ABI** | `core-ffi/` (incl. `cbindgen.toml`), config serialization, capabilities/version, `CSafetyStrip` shim files | `cargo xtask check-abi`, `cargo xtask check-c-ffi-surface`, `cargo test -p safetystrip-ffi`. An intended ABI change means: bump `SS_ABI_VERSION`, run `cargo xtask gen-header`, and call it out in the PR (confirm a non-Swift shell could still consume the boundary). Adding a transform must **not** change the ABI. |
| **Shell** | `shells/macos/` (Swift), reserved `windows/`/`linux/` | `cargo build -p safetystrip-ffi --release` then `swift build --package-path shells/macos`. Touching entitlements → `cargo xtask check-entitlements`; touching release signing → `cargo xtask check-release-posture`; touching the build/release shell scripts → `cargo xtask check-shell`. No transform logic belongs in a shell. |
| **Security / privacy posture** | entitlements, logging, in-memory lifetime, data paths, anything network-adjacent | `cargo xtask check-no-network`, `cargo xtask check-no-content-logging`, `cargo xtask check-pipeline-zeroization`, `cargo xtask check-entitlements`, `cargo xtask check-release-posture`, `cargo xtask check-unsafe-forbid`. Any new entitlement, network-capable dependency, data path, or weakening of wipe-before-release zeroization is a posture change — justify it in the PR and update `SECURITY.md`. |
| **Dependencies & CI** | crate versions, `Cargo.toml`/`Cargo.lock`, lints, `xtask`, `.github/workflows/`, shell scripts | `cargo xtask check-core-deps`, `cargo xtask check-no-network`, `cargo xtask check-supply-chain` (any dependency/lockfile change), `cargo xtask check-workflows` (any workflow change), plus `cargo test -p xtask` / `cargo clippy -p xtask --all-targets -- -D warnings` when editing `xtask`. New crates: prefer boring, audited, API-stable ones; a new core dependency must be a pure-data crate (no OS/IO/net) and added to the `xtask` allowlist with justification. |
| **Docs only** | `README`, `ARCHITECTURE.md`, `DESIGN.md`, `docs/`, runbooks | `cargo fmt --all --check` (still run the formatter). Other checks may be skipped if the PR explains why. |

## Closing review findings

Security scans, ordinary code reviews, fuzzing, CI failures, and performance
reviews can all uncover a class of issue, not just one bad line. Closing that
class requires more than the immediate fix.

For each finding class, follow
[`docs/guardrails/review-finding-closure.md`](docs/guardrails/review-finding-closure.md):

- name the issue class in the PR,
- add a repeatable blocker at the owning layer,
- update the relevant guardrail or posture doc with the lesson,
- run the gate that proves the blocker works.

Use tests for behavior, corpus/property coverage for adversarial input,
`perf_guard` or the documented benchmark flow for performance, and `xtask` or
standard linters for structural invariants. If no mechanical blocker is practical,
document the proof gap and add the strongest repeatable substitute available.

## Fuzzing (never-panics invariant)

The core feeds arbitrary, possibly adversarial bytes through hand-rolled
parsers, so the strippers and the full pipeline are fuzzed to prove they never
panic or hang. `fuzz/` is its own workspace (libFuzzer + the nightly-only
toolchain stay out of normal stable builds).

```sh
# Build every target. On a fresh agent this installs nightly and the pinned
# cargo-fuzz version on demand, matching the CI fuzz-smoke job.
make fuzz

# Briefly run every target:
make fuzz-smoke FUZZ_SMOKE_SECONDS=60

# Or run one target manually (Ctrl-C to stop):
cargo +nightly fuzz run <target>
cargo +nightly fuzz run strip_html -- -max_total_time=60
```

Available targets are discovered mechanically with `cargo +nightly fuzz list`.
Run the target(s) covering any core transform you change, and commit any new
crashing input found under `fuzz/` so it is replayed as a regression. CI runs a
short best-effort nightly fuzz smoke (`continue-on-error`) through the same
`cargo xtask check-fuzz` path; the required signal is the property/corpus tests
in `cargo xtask ci`.

Before cutting a release, run the manual Release Fuzz workflow on the release
candidate ref:

```sh
gh workflow run release-fuzz.yml --ref v1.2.3-rc.1 -f minutes_per_target=30
```

The release workflow checks GitHub Actions for a successful Release Fuzz run whose
`head_sha` exactly matches the tag commit. If the final `v1.2.3` tag points at a
different commit than the RC, release packaging fails until Release Fuzz is rerun
on the new SHA. Crash artifacts and the generated fuzz corpus are uploaded as
short-retention workflow artifacts.

## Pull requests

- State the change class and any compatibility/posture impact (ABI, privacy,
  entitlements, supported transforms).
- Keep diffs narrow and single-purpose; don't mix transform logic, ABI changes,
  shell code, dependency posture, and formatting.
- Update the relevant guardrail and `ARCHITECTURE.md` when an invariant or the
  boundary moves.
- For any security, correctness, or performance finding class fixed by the PR,
  call out the regression protection and docs lesson added.
- Make sure `cargo xtask ci` is green.
