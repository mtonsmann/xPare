# Agent task: core transform change

Prompt template for changing transform logic — an op in `core/src/ops/*`, the
pipeline, or the config schema. Copy the relevant parts into your working brief.

## Files to read

- [`docs/agent-workflow.md`](../agent-workflow.md) — the evidence-first loop.
- [`docs/guardrails/transform-correctness-and-adversarial-input.md`](../guardrails/transform-correctness-and-adversarial-input.md)
  and [`docs/guardrails/memory-safety.md`](../guardrails/memory-safety.md).
- `core/src/pipeline.rs`, `core/src/config.rs`, and the `core/src/ops/*.rs` you touch.
- `core/src/lib.rs` (`CAPABILITIES_JSON`).
- Tests: `core/tests/reference_transform.rs`, `core/tests/determinism.rs`,
  `core/tests/ordering.rs`, `core/tests/config_roundtrip.rs`, and the per-op test
  file (`strippers.rs`, `html_to_markdown.rs`, `defang.rs`, `clean_urls.rs`,
  `mask_identifiers.rs`, `pipeline.rs`).
- `DESIGN.md` (decision log; op semantics live in the implementing function's doc).

## Hard constraints

- The core stays `#![forbid(unsafe_code)]`; no OS/IO/network/logging/global state.
- `transform(input, config)` stays **deterministic** and **never panics** on any
  input (incl. adversarial bytes, lone `\r`, control chars, huge whitespace runs).
- **Adding a transform is data, not API:** a new `Operation` variant + a `pipeline.rs`
  arm + a `CAPABILITIES_JSON` entry + a pure `ops/` function. It must NOT touch the
  C ABI and must not add a dependency outside the core allowlist.
- Stay **linear-time** with bounded lookahead *and lookback* (no O(n²) scans).
- Keep accepted configs inside the resource envelope; if you add a free-text param or
  an op that can expand output, update `Config::validate`, its growth factor, the
  config tests, and `transform_pipeline` fuzz sanitization in the same diff.

## Implementation rules

- Change the op's doc comment in the same diff as any behavior change — it is the
  frozen contract.
- If you add/change a fusion in `pipeline.rs`, it must be byte-for-byte identical to
  sequential application (the reference interpreter proves this) and keep fused
  scratch in `Zeroizing` storage (`check-pipeline-zeroization`).
- If you add an op, add it to the reference interpreter's `apply_one` and the
  operation strategies in the property tests.

## Required tests

- Differential: `transform == reference_transform` (extend
  `core/tests/reference_transform.rs`); new ops join the strategy and the reference's
  `apply_one`. New fusions get an explicit fusion-trigger config.
- A right-answer regression test **and** adversarial-input coverage.
- Determinism / idempotence property where the law is crisp.
- The fuzz target covering the parser you changed; commit any crash under
  `fuzz/regressions/<target>/`.

## Required evidence

- `cargo test -p xpare-core`, `cargo clippy -p xpare-core --all-targets
  -- -D warnings`, `cargo fmt --all --check`, and `cargo xtask ci`.
- The fuzz smoke command and its result if a hand-rolled parser changed.
- Explicit statement: no ABI change, no new dependency, output behavior change (if
  any) called out with the doc-comment + test update.

## Proof gaps to report

- Sanitizers are heuristic, not browser/RFC parsers — say so.
- Fuzzing proves panic/hang freedom probabilistically, not correctness.
- The reference interpreter proves production == naive-sequential semantics; it does
  not prove the semantics themselves are "right" beyond the documented op rules.
