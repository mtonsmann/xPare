#![no_main]
//! Fuzz the config JSON boundary: `parse_config` (serde deserialization + the
//! resource-envelope validation, including the pipeline-growth bound).
//!
//! `parse_config` is the one piece of the core that runs on the FFI's
//! `config_json` argument, and `core-ffi` calls it **outside** the `catch_unwind`
//! that guards the transform — so a panic here would unwind across the C ABI, which
//! is undefined behavior. That makes its panic-freedom a hard safety invariant, not
//! just a nicety. This target asserts it: arbitrary bytes -> `from_utf8_lossy` ->
//! `parse_config`, and the only success condition is "did not panic" (`Ok` and every
//! `Err(ConfigError::…)` are both fine). Inputs that happen to parse as valid JSON
//! also exercise the version gate and `Config::validate` (op-count, text-param, and
//! growth-factor envelopes).
//!
//! Owner: fuzz stream (E).
//!
//! Run, optionally seeding from representative configs (the fuzzer discovers the rest
//! from coverage):
//!   cargo +nightly fuzz run parse_config
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The shell hands the core a NUL-terminated UTF-8 string; mirror that by
    // lossy-decoding arbitrary bytes before parsing. We deliberately ignore the
    // result — both a parsed `Config` and a structured `ConfigError` are valid
    // outcomes; the invariant under test is that parsing/validation never panics.
    let json = String::from_utf8_lossy(data);
    let _ = xpare_core::parse_config(&json);
});
