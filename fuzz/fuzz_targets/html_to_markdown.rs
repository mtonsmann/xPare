#![no_main]
//! Fuzz the one-shot HTML→Markdown converter.
//!
//! Feeds arbitrary bytes -> `String::from_utf8_lossy` -> `ops::html_to_markdown`
//! and asserts the core never panics (libFuzzer aborts on any panic, so simply
//! returning is the success condition). Like `strip_html`, this is a hand-rolled
//! parser over untrusted HTML — it consumes raw markup to preserve structure while
//! dropping `<script>`/`<style>` bodies and unsafe link schemes and escaping
//! entity-decoded text — so it warrants the same deep, nightly-only fuzz layer over
//! the cheaper stable property + corpus-replay tests. It was previously exercised
//! only indirectly (as one operation among twenty in `transform_pipeline`); this
//! target hammers it directly.
//!
//! Owner: fuzz stream (E).
//!
//! Run, seeding from the checked-in adversarial HTML corpus. Copy the seeds into the
//! fuzzer's own (gitignored) corpus dir first — never point `fuzz run` directly at
//! `../core/tests/corpus`, as libFuzzer treats the positional dir as writable and
//! would litter that protected tree with discovered inputs:
//!   mkdir -p corpus/html_to_markdown && cp ../core/tests/corpus/html/* corpus/html_to_markdown/
//!   cargo +nightly fuzz run html_to_markdown
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Lossy decode mirrors the FFI boundary: the core only ever sees valid UTF-8,
    // and invalid byte sequences become U+FFFD rather than being rejected.
    let text = String::from_utf8_lossy(data);
    let _ = xpare_core::ops::html_to_markdown::html_to_markdown(&text);
});
