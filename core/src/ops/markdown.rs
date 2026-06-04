//! Markdown → plain text.
//!
//! **Implementation owner: strippers stream (A1).** CommonMark is too irregular to
//! reimplement safely, so this wraps the boring, well-audited `pulldown-cmark`
//! parser: walk its event stream and emit the text content, dropping formatting.
//! Our event-handling code is still fuzzed and property-tested for panic freedom.
//!
//! Scaffold placeholder below is the identity transform.

/// Strip Markdown formatting, producing plain text.
pub fn strip_markdown(input: &str) -> String {
    // TODO(A1): replace with the pulldown-cmark event walker.
    input.to_string()
}
