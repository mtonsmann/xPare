#![no_main]
//! Fuzz the full pipeline: arbitrary input + arbitrary config -> transform, never panics.
//!
//! Owner: fuzz stream (E). Enhance to derive an arbitrary `Config` (ordered list of
//! operations with arbitrary params) via the `arbitrary` crate so the fuzzer
//! explores operation orderings, not just inputs.
//! Run with `cargo +nightly fuzz run transform_pipeline`.
use libfuzzer_sys::fuzz_target;
use safetystrip_core::{transform, Config, Operation};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    // Placeholder config exercising the adversarial-input ops; E should replace this
    // with an `arbitrary`-derived Config.
    let config = Config {
        version: safetystrip_core::CONFIG_VERSION,
        operations: vec![Operation::StripHtml, Operation::StripMarkdown],
    };
    let _ = transform(&text, &config);
});
