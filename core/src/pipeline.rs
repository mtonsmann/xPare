//! The transformation pipeline: fold a [`Config`]'s operations over the input.
//!
//! `transform` is **infallible** — every operation maps `&str -> String` and always
//! produces output — and **deterministic**: the same `(input, config)` yields the
//! same result, with no dependence on environment, time, locale, or hash ordering.

use crate::config::{Config, Operation};
use crate::ops;

/// Apply every operation in `config.operations` to `input`, left to right.
///
/// Order is significant and exactly as given; the core never reorders. Never
/// panics on any input (enforced by property tests, an adversarial corpus, and the
/// fuzz targets).
pub fn transform(input: &str, config: &Config) -> String {
    let mut text = input.to_string();
    for op in &config.operations {
        text = apply(&text, op);
    }
    text
}

/// Dispatch a single operation to its implementation in [`crate::ops`].
fn apply(text: &str, op: &Operation) -> String {
    match op {
        Operation::StripHtml => ops::html::strip_html(text),
        Operation::StripMarkdown => ops::markdown::strip_markdown(text),
        Operation::CollapseWhitespace => ops::whitespace::collapse_whitespace(text),
        Operation::TrimTrailingWhitespace => ops::whitespace::trim_trailing_whitespace(text),
        Operation::RemoveBlankLines => ops::lines::remove_blank_lines(text),
        Operation::UnwrapLines => ops::lines::unwrap_lines(text),
        Operation::ChangeCase { case } => ops::case::change_case(text, *case),
        Operation::SortLines {
            descending,
            case_insensitive,
        } => ops::lines::sort_lines(text, *descending, *case_insensitive),
        Operation::DedupeLines => ops::lines::dedupe_lines(text),
        Operation::PrefixLines { prefix } => ops::lines::prefix_lines(text, prefix),
        Operation::SuffixLines { suffix } => ops::lines::suffix_lines(text, suffix),
        Operation::JoinWith { separator } => ops::lines::join_with(text, separator),
        Operation::SplitOn { delimiter } => ops::lines::split_on(text, delimiter),
        Operation::ExtractEmails => ops::lines::extract_emails(text),
        Operation::ExtractUrls => ops::lines::extract_urls(text),
    }
}
