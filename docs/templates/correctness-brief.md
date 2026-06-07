<!--
Correctness brief — fill this in BEFORE editing, then paste/link it in the PR.
It is the proposal half of "agents propose; deterministic tools dispose": state
the property before you write code that could violate it. Keep it short and
concrete. Delete the guidance comments when done.
See docs/agent-workflow.md for the loop this belongs to.
-->

# Correctness brief: <short title>

## Change class

<!-- Pick exactly one. It selects the guardrail and the minimum checks. -->

- [ ] Core transform (`core/src/ops/*`, `pipeline.rs`, `config.rs`)
- [ ] FFI / ABI (`core-ffi/*`, the C header, config serialization)
- [ ] macOS shell (`shells/macos/*`)
- [ ] Security / privacy posture (entitlements, logging, data paths, zeroization)
- [ ] Dependency / CI (`Cargo.*`, `xtask`, workflows, scripts)
- [ ] Docs only

## Intended behavior

<!-- What should be true after this change that is not true now? One paragraph. -->

## Must-preserve invariants

<!-- The invariants this change must NOT break. Reference the enforced-invariants
table in ARCHITECTURE.md and the change-class guardrail. Examples: determinism;
canonical == sorted as_given; fused == sequential; resource envelope; no panic;
no network; no content logging; minimal entitlements; frozen ABI. -->

## New invariants

<!-- Any invariant this change introduces, and where it is now enforced
(test / property / reference model / fuzz / xtask check). "None" is a valid answer. -->

## Threats / bug classes considered

<!-- What could go wrong: panic on adversarial input, O(n^2) blow-up, non-determinism
(hash-iteration order), resource amplification / arithmetic wrap, active content
surviving sanitization, unsafe-scheme link leak, ABI drift, content reaching a log
sink, broadened entitlement, etc. -->

## Test plan

<!-- The regression/unit/integration tests added or updated, by file. New behavior
needs both the right-answer test AND adversarial-input coverage. -->

## Fuzz / property plan

<!-- Property tests (incl. the reference-interpreter differential check in
core/tests/reference_transform.rs) and the fuzz target(s) run. Note any new corpus
or fuzz/regressions/<target>/ entry. -->

## Verification / proof plan

<!-- How the must-preserve invariants are mechanically checked: which property,
reference-model differential, xtask check, or linter now blocks a regression.
If a bounded proof (e.g. Kani over the arithmetic envelope) applies, note it here. -->

## Commands to run

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask ci
# plus, if a hand-rolled parser changed:
# make fuzz-smoke FUZZ_SMOKE_SECONDS=60
```

## Evidence packet

<!-- The commands actually run and their results (pass/fail with the relevant
output, not "looks good"). This is what the reviewer trusts. -->

## Proof gaps

<!-- What is NOT proven. This is verification-guided development, not formal
verification. Be explicit (e.g. "HTML not parsed per browser semantics",
"FFI memory behavior exercised, not formally proven"). -->
