#![no_main]
//! Fuzz the URL cleaner.
//!
//! Feeds arbitrary bytes -> `String::from_utf8_lossy` -> `ops::urls::clean_urls`
//! and asserts the core never panics (libFuzzer aborts on any panic, so simply
//! returning is the success condition). The query-string splitter is a hand-rolled,
//! untrusted-input parser, so this is the deep, nightly-only layer over the cheaper
//! stable property + corpus-replay tests.
//!
//! Owner: fuzz stream (E).
//!
//! Run, seeding from the checked-in adversarial corpus. Copy the seeds into the
//! fuzzer's own (gitignored) corpus dir first — never point `fuzz run` directly at
//! `../core/tests/corpus`, as libFuzzer treats the positional dir as writable and
//! would litter that protected tree with discovered inputs:
//!   mkdir -p corpus/clean_urls && cp ../core/tests/corpus/clean_urls/* corpus/clean_urls/
//!   cargo +nightly fuzz run clean_urls
use libfuzzer_sys::fuzz_target;
use safetystrip_core::ops::urls::clean_urls;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = clean_urls(&text);
});
