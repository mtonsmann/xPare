# Agent task: FFI / ABI change

Prompt template for changes to the C ABI surface (`core-ffi/*`), config
serialization, the capabilities/version query, or the generated header.

## Files to read

- [`docs/agent-workflow.md`](../agent-workflow.md).
- [`docs/guardrails/ffi-boundary-and-abi-stability.md`](../guardrails/ffi-boundary-and-abi-stability.md).
- `core-ffi/src/lib.rs`, `core-ffi/cbindgen.toml`, `core-ffi/include/safetystrip.h`.
- `core-ffi/tests/abi_roundtrip.rs` (the boundary-contract tests).
- `ARCHITECTURE.md` (the boundary contract), `DESIGN.md` D2/D4/D7.

## Hard constraints

- The C ABI is **frozen**. The four symbols are `ss_abi_version`,
  `ss_capabilities_json`, `ss_transform`, `ss_buffer_free` — and nothing more.
- **Adding or changing a transform is NOT an ABI change** — feature selection
  crosses as the `config_json` string. Do not bump the ABI for a transform.
- A real ABI change is a **compatibility event**: bump `SS_ABI_VERSION`, run
  `cargo xtask gen-header`, call it out in the PR, and confirm a non-Swift shell
  could still consume the boundary. The checked-in header is the source of truth;
  `check-abi` fails on drift.
- `core-ffi` is the only crate allowed `unsafe`; every unsafe op keeps an explicit
  SAFETY comment (`#![deny(unsafe_op_in_unsafe_fn)]`). Do not move `unsafe` into the
  core.

## Implementation rules

- Every entry point validates pointers, lossy-decodes input UTF-8, rejects oversized
  input *before* reading/allocating, and wraps the core call in `catch_unwind` so a
  panic becomes a status code, never an unwind across the boundary.
- Returned buffers are owned by the caller and freed (and zeroized) via
  `ss_buffer_free`; `ss_buffer_free(null, len)` is a no-op.
- Error paths must set `*out = null` and `*out_len = 0`.

## Required tests

- Extend `core-ffi/tests/abi_roundtrip.rs`: status code, out-param clearing, and the
  ownership/free protocol for any new path. Compare output against
  `safetystrip_core::transform` rather than hardcoding brittle strings.
- If the ABI version changed, assert `ss_abi_version()` and the header `#define` agree.

## Required evidence

- `cargo xtask check-abi`, `cargo xtask check-c-ffi-surface`,
  `cargo test -p safetystrip-ffi`, and `cargo xtask ci`.
- If Miri is available, the FFI-adjacent tests under Miri (pointer/slice/ownership).
- Explicit ABI-version + header-regeneration statement, or "no ABI change".

## Proof gaps to report

- FFI memory behavior is exercised by tests (and optionally Miri), not formally
  proven. `catch_unwind` containment is defense-in-depth over the fuzzed never-panics
  core, not a proof the core cannot panic.
