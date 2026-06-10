<!--
This PR template enforces xPare's evidence-first workflow
(docs/agent-workflow.md). The diff is a proposal; the filled sections below are the
evidence a reviewer trusts. Docs-only changes may skip runtime checks, but a skipped
check must be explained in "Proof gaps / skipped checks".
-->

## Change class

<!-- Pick one (matches the correctness brief and CONTRIBUTING.md). -->

- [ ] Core transform
- [ ] FFI / ABI
- [ ] macOS shell
- [ ] Security / privacy posture
- [ ] Dependency / CI
- [ ] Docs only

## Correctness brief

<!-- Link the filled docs/templates/correctness-brief.md, or summarize:
intended behavior + must-preserve invariants. -->

## Invariants preserved

<!-- Which load-bearing invariants this change keeps intact (determinism;
canonical == sorted as_given; fused == sequential; resource envelope; no panic;
no network; no content logging; minimal entitlements; frozen ABI). And any NEW
invariant introduced, with where it is now enforced. -->

## Compatibility / privacy / security posture impact

<!-- ABI (version bump? header regenerated?), entitlements, network capability,
zeroization, supported transforms. State "none" explicitly if nothing changed. -->

- ABI: <none / bumped XP_ABI_VERSION, header regenerated, non-Swift shell confirmed>
- Privacy / security: <none / describe>

## Tests / properties / fuzzing added or updated

<!-- Name them. New behavior needs both a regression test and adversarial-input
coverage. Note any reference_transform.rs property, new corpus file, or
fuzz/regressions/<target>/ entry. -->

## Commands run

<!-- The actual commands and their results (pass/fail). -->

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask ci
```

## Regression protection

<!-- For any fixed review/scan/fuzz finding CLASS: name the class, the mechanical
blocker added at the owning layer, and the guardrail/posture doc updated
(see docs/guardrails/review-finding-closure.md). -->

## Proof gaps / skipped checks

<!-- What is NOT proven, and any relevant check skipped + why. This is
verification-guided development, not formal verification. -->
