# Contributing to xPare

xPare is a memory-safe, plain-text clipboard utility: a pure Rust
transformation **core** driven by native **shells**. A small set of invariants
(no `unsafe` in the core, a frozen C ABI, no network anywhere, no OS/IO/network
dependencies in the core, deterministic output, minimal macOS entitlements) is
enforced **mechanically** by the `xtask` crate so the same checks run locally
and in CI. See `AGENTS.md` and `docs/guardrails/` for the rationale.

The engineering loop is **evidence-first**: a change is judged by the correctness
evidence it ships, not the diff. Before non-trivial work, fill in a correctness
brief ([`docs/templates/correctness-brief.md`](docs/templates/correctness-brief.md))
and follow [`docs/agent-workflow.md`](docs/agent-workflow.md); the PR template asks
for the resulting **evidence packet**. Pipeline/op behavior is anchored by a
Cedar-style **reference interpreter** — the optimized production `transform` is
differentially tested against a simple one-op-at-a-time reference in
[`core/tests/reference_transform.rs`](core/tests/reference_transform.rs).

## The full local gate

```sh
cargo xtask ci
```

This is the single source of truth and exactly what CI runs. In fail-fast order
it runs:

1. a lockfile-sync gate — `cargo metadata --locked` over the root **and** `fuzz/`
   workspaces, so a stale `Cargo.lock` fails with one clear remediation message
2. `cargo fmt --all --check`
3. `cargo clippy --locked --workspace --all-targets -- -D warnings`
4. `cargo test --locked --workspace`
5. `cargo xtask check-unsafe-forbid` — core still declares `#![forbid(unsafe_code)]`
6. `cargo xtask check-core-deps` — core's transitive dep tree is on a strict allowlist
7. `cargo xtask check-no-network` — no network/OS-capable crate anywhere in the tree
8. `cargo xtask check-no-content-logging` — no shipped line logs/persists clipboard content
9. `cargo xtask check-pipeline-zeroization` — fused core scratch storage is wiped before release
10. `cargo xtask check-agent-workflow` — the AI-native workflow docs exist with required headings
11. `cargo xtask check-clipboard-safety` — default targets never touch the real clipboard
12. `cargo xtask check-c-ffi-surface` — no unexpected handwritten C/C++/Objective-C surface
13. `cargo xtask check-test-hygiene` — every ignored test has a reason; the count is ratcheted
14. `cargo xtask check-docs` — docs build with `-D warnings` (no broken intra-doc links or invalid HTML)
15. `cargo xtask check-abi` — the checked-in C header matches the FFI source
16. `cargo xtask check-entitlements` — the macOS entitlements file is exactly minimal
17. `cargo xtask check-release-posture` — official signing cannot broaden entitlements
18. `cargo xtask check-shell` — `shellcheck` over the shell scripts
19. `cargo xtask check-workflows` — `actionlint` + `zizmor` over `.github/workflows/`
20. `cargo xtask check-unused-deps` — `cargo-machete`: no declared-but-unused dependency
21. `cargo xtask check-supply-chain` — `cargo-deny`: advisories, licenses, bans, sources

If any check fails, it prints a remediation-oriented message. **Fix the code so
the check passes; do not weaken the check.** Each check can also be run on its
own (e.g. `cargo xtask check-abi`) for a fast inner loop.

> Steps 1–4 shell out to `cargo` (`cargo fmt` has no `--locked` flag; it resolves
> nothing). `rust-toolchain.toml` pins the **stable channel** + `clippy` +
> `rustfmt`, so any current stable works. The MSRV is **1.85** (`rust-version` in
> `Cargo.toml`), checked by a dedicated CI job — keep new code MSRV-compatible.
> MSRV bumps are minor releases.
>
> Steps 18–21 shell out to external linters. The cargo-installable ones
> (`cargo-deny`, `zizmor`, `cargo-machete`) auto-install a pinned version on first
> use; the system tools (`shellcheck`, `actionlint`) print a one-line install
> command if missing. CI pre-installs all of them (pinned), so a green
> `cargo xtask ci` locally means a green PR — there is no required check outside
> this one command. Optional fuzzing uses the same pattern through
> `cargo xtask check-fuzz`, which installs nightly and the pinned `cargo-fuzz`
> tool when a fresh agent is missing them. Releases add one stronger gate: the
> manual Release Fuzz workflow must pass on the exact release SHA — with a
> sufficient per-target budget — before `.github/workflows/release.yml` will
> package the tag (see [Fuzzing](#fuzzing-never-panics-invariant)).

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
| **Core transform** | `core/` transform logic, ops, pipeline | `cargo test -p xpare-core`, `cargo clippy -p xpare-core --all-targets -- -D warnings`, `cargo fmt`, and the relevant fuzz target (below). New behavior needs regression **and** adversarial-input tests (prefer a reference-interpreter clause + property over a lone example); output must stay deterministic. For dead-code / weak-test confidence, run the best-effort `cargo xtask check-mutants` (`XP_DIFF_BASE=origin/main` scopes it to your diff). See [code & test hygiene](docs/guardrails/code-and-test-hygiene.md). |
| **FFI boundary / ABI** | `core-ffi/` (incl. `cbindgen.toml`), config serialization, capabilities/version, `CXPare` shim files | `cargo xtask check-abi`, `cargo xtask check-c-ffi-surface`, `cargo test -p xpare-ffi`. An intended ABI change means: bump `XP_ABI_VERSION`, run `cargo xtask gen-header`, and call it out in the PR (confirm a non-Swift shell could still consume the boundary). Adding a transform must **not** change the ABI. |
| **Shell** | `shells/macos/` (Swift), reserved `windows/`/`linux/` | `cargo build -p xpare-ffi --release` then `swift build --package-path shells/macos`. Touching entitlements → `cargo xtask check-entitlements`; touching release signing → `cargo xtask check-release-posture`; touching the build/release shell scripts → `cargo xtask check-shell`. No transform logic belongs in a shell. |
| **Security / privacy posture** | entitlements, logging, in-memory lifetime, data paths, anything network-adjacent | `cargo xtask check-no-network`, `cargo xtask check-no-content-logging`, `cargo xtask check-pipeline-zeroization`, `cargo xtask check-entitlements`, `cargo xtask check-release-posture`, `cargo xtask check-unsafe-forbid`. Any new entitlement, network-capable dependency, data path, or weakening of wipe-before-release zeroization is a posture change — justify it in the PR and update `SECURITY.md`. |
| **Dependencies & CI** | crate versions, `Cargo.toml`/`Cargo.lock`, lints, `xtask`, `.github/workflows/`, shell scripts | `cargo xtask check-core-deps`, `cargo xtask check-no-network`, `cargo xtask check-supply-chain` + `cargo xtask check-unused-deps` (any dependency/lockfile change), `cargo xtask check-workflows` (any workflow change), plus `cargo test -p xtask` / `cargo clippy -p xtask --all-targets -- -D warnings` when editing `xtask`. New crates: prefer boring, audited, API-stable ones; a new core dependency must be a pure-data crate (no OS/IO/net) and added to the `xtask` allowlist with justification — and it must actually be used (`check-unused-deps`). |
| **Docs only** | `README`, `ARCHITECTURE.md`, `DESIGN.md`, `docs/`, runbooks | `cargo fmt --all --check` (still run the formatter); if you edited Rust doc comments, `cargo xtask check-docs`. Other checks may be skipped if the PR explains why. |

## Automated PR review (advisory)

An automated agent review (anti-slop on every code PR; security focus when the PR
touches security-relevant surface) may post findings against the repo's guardrails —
see [the hygiene guardrail](docs/guardrails/code-and-test-hygiene.md) § "Tier-2
review". It is **advisory**: it never blocks; the required signal stays
`cargo xtask ci`, and a recurring finding should graduate into a deterministic
`xtask` check. (Reviewer wiring is a maintainer concern, documented in the
hygiene guardrail.)

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
parsers, so the strippers and the full pipeline are fuzzed to pin down that they
never panic or hang — strong probabilistic evidence over billions of generated
inputs, not a proof. `fuzz/` is its own workspace (libFuzzer + the nightly-only
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

# Long local campaign sized from current CPU load and available memory.
# Defaults to every fuzz target and splits FUZZ_HOURS across them:
make fuzz-overnight FUZZ_HOURS=8

# Focus on one or more targets:
make fuzz-overnight FUZZ_HOURS=8 FUZZ_TARGETS="transform_pipeline strip_html"
```

Available targets are discovered mechanically with `cargo +nightly fuzz list`.
Run the target(s) covering any core transform you change, and keep any new crashing
input as a checked-in regression under `fuzz/regressions/<target>/` (the overnight
script stages these for you — see [Triaging a finding](#triaging-a-finding)). CI runs
a short best-effort nightly fuzz smoke (`continue-on-error`) through the same
`cargo xtask check-fuzz` path; the required signal is the property/corpus tests
in `cargo xtask ci`.

`scripts/overnight-fuzz.sh HOURS [TARGET ...]` is for unattended local campaigns.
With no targets, it discovers every target with `cargo +nightly fuzz list` and
splits the total runtime across them. It targets about 85% system-load saturation
by default, caps workers by available memory, writes logs under `fuzz-runs/`, and
refuses to add load when the machine is already near the target unless
`FUZZ_ALLOW_OVERCOMMIT=1` is set. Use `FUZZ_DRY_RUN=1 make fuzz-overnight` to
check the selected targets and worker count without starting libFuzzer.

Before cutting a release, run the manual Release Fuzz workflow on the release
candidate ref:

```sh
gh workflow run release-fuzz.yml --ref v1.2.3-rc.1 -f minutes_per_target=30
```

The release workflow checks GitHub Actions for a successful Release Fuzz run whose
`head_sha` exactly matches the tag commit, **and** enforces a per-target budget
floor read from the run's title: at least **10 minutes/target** for prerelease
tags (any tag containing `-`, e.g. `v1.0.0-rc.1`) and at least
**30 minutes/target** for final releases. If the final `v1.2.3` tag points at a
different commit than the RC, release packaging fails until Release Fuzz is rerun
on the new SHA. Crash artifacts and the generated fuzz corpus are uploaded as
short-retention workflow artifacts.

### Triaging a finding

`scripts/overnight-fuzz.sh` triages automatically. After each target it re-runs every
new `crash`/`oom`/`timeout` artifact **single-threaded** — a unit starved during a
saturated multi-worker run is a contention artifact, not a bug, and is dropped here —
then minimizes and decodes the genuine ones and stages a committable reproducer plus a
triage note under `fuzz/regressions/<target>/`. Pass `--auto-commit` to commit each as
it is found (on a fresh branch if you are on `main`), or run the printed `git`
one-liner yourself. `--no-triage` skips the step.

To triage by hand (or a finding from a plain `cargo fuzz run`):

```sh
# 1. Minimize — never share or commit the raw blob.
cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/crash-…

# 2. Decode — for arbitrary-based targets (e.g. transform_pipeline) this prints the
#    structured input (the synthesized operation pipeline), not opaque bytes.
cargo +nightly fuzz fmt <target> <minimized-input>

# 3. Keep it as a permanent regression: copy the minimized input into
#    fuzz/regressions/<target>/ (checked in, unlike corpus/ and artifacts/) and commit.
cargo +nightly fuzz run <target> fuzz/regressions/<target>/<file> -- -runs=1  # confirm
```

To report a finding upstream, open a GitHub issue with the target name, toolchain +
commit SHA, the libFuzzer/sanitizer output, the decoded input, and the **minimized**
reproducer attached — not a tarball. Closing the finding means more than the one fix:
keep the committed regression, add a focused test for the behavior, and follow the
[finding-closure guardrail](docs/guardrails/review-finding-closure.md).

## Miri (FFI undefined-behavior detection)

The core is `#![forbid(unsafe_code)]`, so all `unsafe` lives in `core-ffi` (pointer
validation, the leaked-`Box<[u8]>` buffer protocol, zeroize-on-free, lossy-UTF-8
decode). `core-ffi/tests/abi_roundtrip.rs` drives the real `extern "C"` entry points
through raw pointers; running it under [Miri](https://github.com/rust-lang/miri)'s
UB detector turns "the boundary is exercised" into "no undefined behavior was
detected on the tested executions".

```sh
cargo run -p xtask -- check-miri      # auto-installs nightly + the miri component
```

Like fuzzing, this is **nightly-only and best-effort**, so it is intentionally
**outside** the required `cargo xtask ci` gate (CI runs it as a `continue-on-error`
job). It is dynamic UB detection over the executions the tests drive, not a proof
and not input coverage — cargo-fuzz owns coverage. The fuzz-lite sweep scales itself
down under Miri (`cfg!(miri)`) so the pass stays fast.

## Kani (bounded proofs over the resource envelope)

[Kani](https://github.com/model-checking/kani) is a bounded model checker: it proves
a property for **all** inputs within bounds (via CBMC), not just the ones a test
drives. The harnesses live in `core/src/config.rs` behind `#[cfg(kani)]` (so they are
invisible to normal builds and to `cargo metadata` — `kani` never enters the
dependency tree `check-core-deps` guards) and prove the crisp resource-envelope
arithmetic: the saturating growth-product gate accepts a pipeline **iff** its true,
arbitrary-precision worst-case growth is within `MAX_PIPELINE_GROWTH_FACTOR`, so no
saturation wrap can falsely accept an amplifying config.

```sh
cargo run -p xtask -- check-kani      # installs kani-verifier + CBMC on first use
```

Kani is **heavy** (`cargo kani setup` downloads a CBMC toolchain), so — unlike
fuzz/Miri — it is not a per-PR job. And because a bounded model check is
deterministic and exhaustive within its bounds, re-running it on unchanged code
proves nothing new (a *time* cadence would be pointless — that only helps fuzzing,
where more wall-clock buys more coverage). So [`proofs.yml`](.github/workflows/proofs.yml)
is **event-driven**: it runs only when something that could change the result changes
— the proven arithmetic (`core/src/config.rs`), the check or the pinned Kani version
(`xtask/src/main.rs`), or the workflow itself — plus on demand, and locally with the
command above. It stays **outside** the required gate; the everyday signal is the
proptest growth-envelope property in `reference_transform.rs` and the saturation tests
in `config_roundtrip.rs`. The proofs cover the arithmetic only — **not** the
`String`-bearing config or the text transformer.

## Coverage & mutation testing (best-effort)

These two checks measure how much the code is actually *exercised and asserted*, the
deepest anti-slop signal. `check-coverage` runs [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)
and fails if line coverage drops below a ratcheted floor (`COVERAGE_FLOOR_PCT`, currently
95%; product baseline ~95.6%), measuring the product crates only — the `xtask` enforcement
harness is excluded. `check-mutants` runs [cargo-mutants](https://github.com/sourcefrog/cargo-mutants)
(config in `.cargo/mutants.toml`): a *surviving* mutant means either dead code or a test that runs
but asserts too little. The fix for a survivor is to **strengthen a test** (which becomes a
permanent regression), never to delete the check.

```sh
cargo run -p xtask -- check-coverage   # auto-bootstraps the llvm-tools component
cargo run -p xtask -- check-mutants    # full tree

# Scope the mutation run to the lines you changed (fast PR feedback):
XP_DIFF_BASE=origin/main cargo run -p xtask -- check-mutants
```

Both are **heavy and deterministic**, so — like Miri and Kani — they sit **outside** the
required `cargo xtask ci` gate. Re-running them on unchanged code proves nothing new, so
they are event-driven, not scheduled: on demand locally, and path-filtered in
[`hygiene.yml`](.github/workflows/hygiene.yml) (a `continue-on-error` job that scopes the
mutation run to the PR diff via `XP_DIFF_BASE`). There is **no cron** — a stable repo
pays nothing. See [code & test hygiene](docs/guardrails/code-and-test-hygiene.md).

## macOS shell anti-slop (`check-swift`, best-effort)

The numbered gates above are Rust/cargo-specific, so the Swift macOS shell
(`shells/macos/`) has its own cross-language anti-slop tier in `cargo xtask check-swift`
(`make swift`):

```sh
cargo run -p xtask -- check-swift   # builds the FFI staticlib, then lint + test + coverage
```

It runs, in order: `swift format lint --strict` (driven by
[`shells/macos/.swift-format`](shells/macos/.swift-format)); a `cargo build -p
xpare-ffi --release` so the package can link the staticlib over the frozen C ABI;
`swift test`; and a **Sources-only** line-coverage floor via `llvm-cov`
(`SWIFT_COVERAGE_FLOOR_PCT` in `xtask/src/main.rs` — a hard ratchet matching the Rust
product floor, with the OS-facing pasteboard/hotkey layers tested headlessly and only the
SwiftUI app executable left unmeasured). If
[`swiftlint`](https://github.com/realm/SwiftLint) is on `PATH` it also runs a
style/complexity pass against [`shells/macos/.swiftlint.yml`](shells/macos/.swiftlint.yml)
(non-`--strict`: warnings advise, only `error`-severity findings fail). CI installs a
**pinned, checksum-verified** SwiftLint (the `portable_swiftlint.zip` release asset, hashed
like actionlint), so this phase runs there; locally it's **run-if-present** and skips with a
note. SourceKit is disabled for determinism (a CLT-only host can't load it).

This tier is **macOS-only and best-effort**: it sits outside the required `cargo xtask ci`
(which runs on Linux), runs in the `continue-on-error` `macos-shell` CI job, and skips
cleanly where the Swift toolchain or sources are absent. The security-critical transform
logic lives in the gated Rust core, not the shell. See
[code & test hygiene](docs/guardrails/code-and-test-hygiene.md).

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
