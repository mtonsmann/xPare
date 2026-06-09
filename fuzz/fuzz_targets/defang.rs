#![no_main]
//! Fuzz the defang / refang scanners.
//!
//! Feeds arbitrary bytes -> `String::from_utf8_lossy` -> `ops::defang::{defang,
//! refang}` for both bracket styles, asserting the core never panics (libFuzzer
//! aborts on any panic, so simply returning is the success condition). Defang is a
//! hand-rolled, untrusted-input token scanner, so this is the deep, nightly-only
//! layer over the cheaper stable property + corpus-replay tests.
//!
//! Owner: fuzz stream (E).
//!
//! Run, seeding from the checked-in adversarial corpus. Copy the seeds into the
//! fuzzer's own (gitignored) corpus dir first — never point `fuzz run` directly at
//! `../core/tests/corpus`, as libFuzzer treats the positional dir as writable and
//! would litter that protected tree with discovered inputs:
//!   mkdir -p corpus/defang && cp ../core/tests/corpus/defang/* corpus/defang/
//!   cargo +nightly fuzz run defang
use libfuzzer_sys::fuzz_target;
use xpare_core::ops::defang::{defang, refang};
use xpare_core::BracketStyle;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = defang(&text, BracketStyle::Square);
    let _ = defang(&text, BracketStyle::Round);
    let _ = refang(&text);
});
