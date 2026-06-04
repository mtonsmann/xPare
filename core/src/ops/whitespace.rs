//! Whitespace normalization.
//!
//! **Implementation owner: pipeline stream (A2).** Scaffold placeholders below are
//! identity transforms; replace with the real implementations and add tests.

/// Collapse runs of spaces/tabs to a single space. Newlines are preserved.
pub fn collapse_whitespace(input: &str) -> String {
    // TODO(A2): collapse runs of intra-line whitespace to a single space.
    input.to_string()
}

/// Trim trailing whitespace from each line (line structure preserved).
pub fn trim_trailing_whitespace(input: &str) -> String {
    // TODO(A2): trim trailing whitespace per line.
    input.to_string()
}
