//! HTML → plain text extraction.
//!
//! **Implementation owner: strippers stream (A1).** This is the shared rich→plain
//! workhorse: the shell hands the core an HTML string and the core extracts text.
//! It is hand-rolled, pure-safe-Rust on purpose (no opaque upstream HTML parser),
//! so memory-unsafety is impossible and the only residual risks — panics and hangs
//! on adversarial input — are pinned down by the corpus, property tests, and fuzzer.
//!
//! Scaffold placeholder below is the identity transform; replace with the real
//! state machine (tags, comments, `<script>`/`<style>` raw-text, entity decoding).

/// Strip HTML tags and decode common entities, producing plain text.
pub fn strip_html(input: &str) -> String {
    // TODO(A1): replace with the real HTML→text state machine.
    input.to_string()
}
