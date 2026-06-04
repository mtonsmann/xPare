#![no_main]
//! Fuzz the HTML stripper: arbitrary bytes -> lossy &str -> strip_html, never panics.
//! Owner: fuzz stream (E). Run with `cargo +nightly fuzz run strip_html`.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = safetystrip_core::ops::html::strip_html(&text);
});
