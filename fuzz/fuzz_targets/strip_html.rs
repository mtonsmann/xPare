#![no_main]
//! Fuzz the HTML stripper.
//!
//! Feeds arbitrary bytes -> `String::from_utf8_lossy` -> `ops::html::strip_html`
//! and asserts the core never panics (libFuzzer aborts on any panic, so simply
//! returning is the success condition). HTML is a hand-rolled, untrusted-input
//! parser, so this is exactly the deep, nightly-only layer over the cheaper
//! stable property + corpus-replay tests.
//!
//! Owner: fuzz stream (E).
//!
//! Run, seeding from the checked-in adversarial corpus. Copy the seeds into the
//! fuzzer's own (gitignored) corpus dir first — never point `fuzz run` directly at
//! `../core/tests/corpus`, as libFuzzer treats the positional dir as writable and
//! would litter that protected tree with discovered inputs:
//!   mkdir -p corpus/strip_html && cp ../core/tests/corpus/html/* corpus/strip_html/
//!   cargo +nightly fuzz run strip_html
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Lossy decode mirrors the FFI boundary: the core only ever sees valid UTF-8,
    // and invalid byte sequences become U+FFFD rather than being rejected.
    let text = String::from_utf8_lossy(data);
    let _ = xpare_core::ops::html::strip_html(&text);
});
