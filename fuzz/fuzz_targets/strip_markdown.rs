#![no_main]
//! Fuzz the Markdown stripper: arbitrary bytes -> lossy &str -> strip_markdown, never panics.
//! Owner: fuzz stream (E). Run with `cargo +nightly fuzz run strip_markdown`.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = safetystrip_core::ops::markdown::strip_markdown(&text);
});
