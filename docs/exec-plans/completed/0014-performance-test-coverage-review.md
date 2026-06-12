# Performance Test Coverage Review

## Change class

Dependencies, CI, and automation; native shell performance coverage.

## Goal

Audit the feature surface and make performance evidence repeatable for every
feature class, including macOS shell-only paths that core benchmarks cannot
cover.

## Guardrails

- Do not change the FFI ABI, privacy posture, core dependency boundary, or
  transform semantics.
- Keep Swift shell coverage at the shell layer; no transform logic moves into
  Swift.
- Use synthetic clipboard data only. Do not read the real pasteboard in
  performance tests.
- Keep new checks deterministic enough for CI.

## Plan

1. Map core transform features and shell-owned features to existing tests,
   benches, and performance guards.
2. Identify uncovered feature classes.
3. Add the smallest repeatable guard or measured smoke at the owning layer.
4. Document the coverage map and verification commands.
5. Run targeted checks, then move this plan to `completed/`.

## Decision log

- 2026-06-11: Treat shell-only features as needing Swift performance guards
  because core `perf_guard` and criterion benches cannot exercise
  pasteboard extraction, settings/config assembly, hotkey dispatch, monitor
  polling, or local image OCR preflight.
- 2026-06-11: Keep the shell guards synthetic and deterministic: fake
  pasteboards/injected transformers for controller loops, named pasteboards only
  for raw representation preflight, and no Vision benchmark in CI.

## Outcome

- Added an exhaustive core operation performance coverage map that fails to
  compile when a new core feature lacks an explicit scenario.
- Added missing `make perf` rows for `html_to_markdown`, parameterized line ops,
  reductions, sort flags, and masking.
- Added `make shell-perf` and a Swift performance suite covering settings/config
  assembly, strip/run-once orchestration, pasteboard preflight, monitor polling,
  hotkey dispatch, and image OCR command orchestration.
- Documented the performance coverage map in `docs/performance.md`.

## Verification

- `cargo test -p safetystrip-core --test throughput`
- `make perf PERF_MIB=1 PERF_SAMPLES=1`
- `cargo fmt --all --check`
- `cargo clippy -p safetystrip-core --test throughput -- -D warnings`
- `cargo run -p xtask -- check-clipboard-safety`
- `cargo run -p xtask -- check-agent-workflow`
- `make shell-perf`
