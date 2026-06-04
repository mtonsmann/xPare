//! Case transformations.
//!
//! **Implementation owner: pipeline stream (A2).** Scaffold placeholder below is
//! the identity transform; replace with real Upper/Lower/Title/Sentence logic and
//! add tests, including non-ASCII behavior.

use crate::CaseKind;

/// Recase the whole text according to `kind`.
pub fn change_case(input: &str, kind: CaseKind) -> String {
    // TODO(A2): implement Upper/Lower/Title/Sentence.
    let _ = kind;
    input.to_string()
}
