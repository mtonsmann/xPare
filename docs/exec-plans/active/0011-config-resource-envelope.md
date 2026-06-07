# Exec Plan 0011 - Config resource envelope

Status: **active** - Started: 2026-06-06

## Goal

Close the transform-pipeline fuzz OOM class where a tiny input plus adversarial
free-text operation parameters can request multi-GB intermediates. Keep the fix
proportional for 1.0: accepted configs stay product-shaped, the core remains
infallible once handed a config, and future shells inherit one validation contract.

## Change Class

Core transform/config validation, fuzzing, docs, and small shell UX mirroring. No C
ABI change, no dependency change, no privacy-posture change, and no transform-output
change for configs inside the new envelope.

## Decisions

### D-1 - Validate the config envelope at the core boundary

`parse_config` will reject configs that exceed a small, documented resource
envelope: more than 32 operations, free-text parameters longer than 256 UTF-8 bytes,
or free-text parameters containing `\r`/`\n` where they can create extra line
boundaries. This keeps the JSON config channel as the source of truth for FFI, CLI,
macOS, and future shells.

### D-2 - Do not make `transform` fallible in this pass

A budgeted `transform(input, config) -> Result<_, _>` would be the strongest
arbitrary-config defense, but it is a larger core/FFI contract change. For 1.0, the
accepted config space should match product use cases and block the exponential line
growth class found by fuzzing. A fallible transform remains future hardening if
arbitrary untrusted configs become in-scope.

### D-3 - Fuzz only valid product configs

The `transform_pipeline` target should continue to synthesize operation order and
parameters, but it should clamp/sanitize generated configs into the same valid
envelope before calling `transform`. The fuzzer then searches hostile clipboard text
and valid user configs, not intentionally impossible resource requests.

### D-4 - Mirror validation in the shell for humane UX as follow-up

The Rust core remains authoritative. The macOS settings UI may mirror the same
limits so users see immediate feedback instead of only an FFI/config error. This
needs draft text-field state to avoid silently normalizing user input, so it is not
part of the first core/fuzz closure patch.

## Work Items

1. Add core validation constants, errors, and tests in `core/src/config.rs` and
   `core/tests/config_roundtrip.rs`.
2. Align `fuzz/fuzz_targets/transform_pipeline.rs` to produce valid configs only.
3. Add regression protection for newline-bearing affix parameters and over-limit
   operation/parameter counts.
4. Update guardrails/docs with the lesson from the fuzz finding.
5. Document the shell UX mirror as follow-up unless it stays small and clear.
6. Run focused core tests, fmt, and the adjusted fuzz target with a short smoke.

## Acceptance

- `parse_config` rejects the fuzz-discovered amplification shape before transform.
- Valid single-line prefix/suffix/join/split configs still parse and transform.
- The transform-pipeline fuzz target no longer generates invalid config shapes.
- Docs state the resource-envelope rule and the remaining proof gap: accepted configs
  are bounded, but transform still is not a general output-budgeted API.
