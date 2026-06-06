//! The transformation pipeline: fold a [`Config`]'s operations over the input.
//!
//! `transform` is **infallible** — every operation maps `&str -> String` and always
//! produces output — and **deterministic**: the same `(input, config)` yields the
//! same result, with no dependence on environment, time, locale, or hash ordering.
//!
//! ## Sensitive-data hygiene
//!
//! The input and every intermediate are clipboard-derived and may hold secrets
//! (passwords, tokens). Each intermediate is held in a [`Zeroizing`] buffer so its
//! bytes are wiped from the heap as soon as the next pass supersedes it, rather than
//! lingering in freed memory until the allocator happens to reuse it. The final
//! result is returned directly (the caller owns it, so no extra copy is made); the
//! `core-ffi` shim zeroizes that output buffer when the caller frees it. So every
//! buffer except the caller-owned result is wiped after use. The measurable cost is
//! the per-intermediate wipe — tens of percent of throughput on 100+ MiB inputs, but
//! negligible at clipboard scale (sub-MiB), where the absolute time is microseconds
//! either way. Quantified in `docs/performance.md`.

use zeroize::Zeroizing;

use crate::config::{Config, Operation, Ordering};
use crate::ops;

/// Apply the config's operations to `input`.
///
/// The execution order depends on [`Config::ordering`]: [`Ordering::Canonical`] (the
/// default) stable-sorts the operations by [`Operation::canonical_rank`] so the result
/// is correct and efficient regardless of how a UI assembled them;
/// [`Ordering::AsGiven`] runs them in the exact order provided. Either way `transform`
/// is deterministic and never panics on any input (enforced by property tests, an
/// adversarial corpus, and the fuzz targets). Intermediates are zeroized on drop (see
/// the module docs).
pub fn transform(input: &str, config: &Config) -> String {
    // Identity pipeline: nothing to wipe, return the single copy directly.
    if config.operations.is_empty() {
        return input.to_string();
    }
    // Resolve execution order. We sort *references*, never the ops themselves, so a
    // canonical run clones no operation parameters. `sort_by_key` is a stable sort, so
    // operations sharing a rank keep their provided order.
    let mut ordered: Vec<&Operation> = config.operations.iter().collect();
    if config.ordering == Ordering::Canonical {
        ordered.sort_by_key(|op| op.canonical_rank());
    }
    // Each intermediate lives in a Zeroizing buffer, wiped when the next pass replaces
    // it (and the last input is wiped when this function returns). The FINAL output is
    // returned directly — no extra copy — and the core-ffi shim wipes it on free.
    let mut current = Zeroizing::new(input.to_string());
    let last = ordered.len() - 1;
    for (i, op) in ordered.into_iter().enumerate() {
        let out = apply(&current, op);
        if i == last {
            return out;
        }
        current = Zeroizing::new(out);
    }
    unreachable!("operations is non-empty, so the loop returns at i == last")
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
        Operation::Defang { style } => ops::defang::defang(text, *style),
        Operation::Refang => ops::defang::refang(text),
        Operation::CleanUrls => ops::urls::clean_urls(text),
    }
}
