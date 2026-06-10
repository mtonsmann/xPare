//! The transformation pipeline: fold a [`Config`]'s operations over the input.
//!
//! `transform` is **infallible** — every operation maps `&str -> String` and always
//! produces output — and **deterministic**: the same `(input, config)` yields the
//! same result, with no dependence on environment, time, locale, or hash ordering.
//!
//! ## Sensitive-data hygiene
//!
//! The caller-owned input and every xPare-owned intermediate are
//! clipboard-derived and may hold secrets (passwords, tokens). The first operation
//! borrows the caller input directly; each operation output that feeds another pass is
//! then held in a [`Zeroizing`] buffer so its bytes are wiped from the heap as soon as
//! the next pass supersedes it, rather than lingering in freed memory until the
//! allocator happens to reuse it. Transform-local scratch buffers are wiped on drop
//! and before capacity growth could release old storage; they are not wiped on every
//! reuse while the allocation remains owned by the transform. Operation
//! implementations follow the same posture for the buffers they build their output
//! in: each output is either pre-sized to a provably sufficient capacity (so its
//! allocation never moves) or appended through `ops::wipe`, which zeroizes a
//! superseded allocation before growth frees it (see `core/src/ops/mod.rs`). The
//! final result is returned directly (the caller owns it, so no extra copy is
//! made); the `core-ffi` shim zeroizes that output buffer when the caller frees it.
//!
//! Two op-internal allocations remain best-effort gaps, wiped only by eventual
//! allocator reuse: `sort_lines`' case-insensitive comparison keys (one folded copy
//! per line) and the third-party Markdown parser's internal event buffers inside
//! `strip_markdown`. Everything else xPare owns is wiped after use. The
//! measurable cost is the per-intermediate wipe — tens of percent of throughput on
//! 100+ MiB inputs, but negligible at clipboard scale (sub-MiB), where the absolute
//! time is microseconds either way. Quantified in `docs/performance.md`.

use zeroize::{Zeroize, Zeroizing};

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
    // Borrow the caller-owned input for the first pass. Only operation outputs that
    // feed another pass become xPare-owned intermediates and need `Zeroizing`.
    let (first, consumed) = apply_next(input, &ordered);
    let mut i = consumed;

    // Each output that feeds another pass is moved into a Zeroizing intermediate,
    // wiped when that iteration ends. The FINAL output never enters the loop body
    // and is returned directly — no extra copy — and the core-ffi shim wipes it on
    // free. The loop condition alone decides termination (`apply_next` always
    // consumes at least one operation), so this path needs no panic-family macro:
    // the core's "never panics" contract holds by construction, not by assertion.
    let mut current = first;
    while i < ordered.len() {
        let intermediate = Zeroizing::new(current);
        let (out, consumed) = apply_next(&intermediate, &ordered[i..]);
        i += consumed;
        current = out;
    }
    current
}

/// Dispatch one pipeline step, optionally fusing adjacent operations whose combined
/// behavior is byte-for-byte identical but avoids a zeroized intermediate.
fn apply_next(text: &str, ops: &[&Operation]) -> (String, usize) {
    if ops.len() >= 2
        && matches!(ops[0], Operation::StripHtml)
        && matches!(ops[1], Operation::StripMarkdown)
    {
        if let Some(plain) = ops::markdown::strip_plain_log_markdown(text) {
            return (plain, 2);
        }
    }
    if ops.len() >= 4
        && matches!(ops[0], Operation::CollapseWhitespace)
        && matches!(ops[1], Operation::TrimTrailingWhitespace)
        && matches!(ops[2], Operation::RemoveBlankLines)
        && matches!(ops[3], Operation::DedupeLines)
    {
        return (collapse_trim_remove_blank_then_dedupe_lines(text), 4);
    }
    if ops.len() >= 3
        && matches!(ops[0], Operation::CollapseWhitespace)
        && matches!(ops[1], Operation::TrimTrailingWhitespace)
        && matches!(ops[2], Operation::RemoveBlankLines)
    {
        return (collapse_trim_then_remove_blank_lines(text), 3);
    }
    if ops.len() >= 3
        && matches!(ops[0], Operation::TrimTrailingWhitespace)
        && matches!(ops[1], Operation::RemoveBlankLines)
        && matches!(ops[2], Operation::DedupeLines)
    {
        return (trim_remove_blank_then_dedupe_lines(text), 3);
    }
    if ops.len() >= 2
        && matches!(ops[0], Operation::TrimTrailingWhitespace)
        && matches!(ops[1], Operation::RemoveBlankLines)
    {
        return (trim_trailing_then_remove_blank_lines(text), 2);
    }
    (apply(text, ops[0]), 1)
}

/// Dispatch a single operation to its implementation in [`crate::ops`].
fn apply(text: &str, op: &Operation) -> String {
    match op {
        Operation::StripHtml => ops::html::strip_html(text),
        Operation::StripMarkdown => ops::markdown::strip_markdown(text),
        Operation::HtmlToMarkdown => ops::html_to_markdown::html_to_markdown(text),
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
        Operation::MaskIdentifiers { emails, ipv4, ipv6 } => {
            ops::mask::mask_identifiers(text, *emails, *ipv4, *ipv6)
        }
    }
}

/// Fused `TrimTrailingWhitespace` followed by `RemoveBlankLines`.
///
/// This is an internal W3 planner optimization only: it preserves the exact visible
/// behavior of applying the two public ops in sequence, but saves one allocation,
/// one full pass over the intermediate, and one zeroized intermediate drop.
fn trim_trailing_then_remove_blank_lines(input: &str) -> String {
    let mut preserve_trailing_newline = input.ends_with('\n');
    let mut out = String::with_capacity(input.len());
    let mut wrote_line = false;
    let mut saw_newline = false;
    let mut start = 0usize;
    for (i, ch) in input.char_indices() {
        if ch == '\n' {
            saw_newline = true;
            push_trimmed_nonblank_line(&mut out, &input[start..i], &mut wrote_line);
            start = i + 1;
        }
    }
    if !preserve_trailing_newline {
        let final_line = trim_non_newline_whitespace_end(&input[start..]);
        if final_line.chars().all(char::is_whitespace) {
            preserve_trailing_newline = saw_newline;
        } else {
            push_nonblank_line(&mut out, final_line, &mut wrote_line);
        }
    }
    if preserve_trailing_newline && wrote_line {
        out.push('\n');
    }
    out
}

/// Fused `TrimTrailingWhitespace`, `RemoveBlankLines`, then `DedupeLines`.
///
/// This keeps dedupe keys as borrowed slices from the caller/intermediate input,
/// matching the public `dedupe_lines` storage class while avoiding the cleaned-line
/// intermediate that would otherwise feed dedupe.
fn trim_remove_blank_then_dedupe_lines(input: &str) -> String {
    let mut preserve_trailing_newline = input.ends_with('\n');
    let mut out = String::with_capacity(input.len());
    let mut wrote_line = false;
    let mut saw_newline = false;
    let mut start = 0usize;
    let mut seen = std::collections::HashSet::new();
    for (i, ch) in input.char_indices() {
        if ch == '\n' {
            saw_newline = true;
            push_unique_trimmed_nonblank_line(
                &mut out,
                &input[start..i],
                &mut seen,
                &mut wrote_line,
            );
            start = i + 1;
        }
    }
    if !preserve_trailing_newline {
        let final_line = trim_non_newline_whitespace_end(&input[start..]);
        if final_line.chars().all(char::is_whitespace) {
            preserve_trailing_newline = saw_newline;
        } else if seen.insert(final_line) {
            push_nonblank_line(&mut out, final_line, &mut wrote_line);
        }
    }
    if preserve_trailing_newline && wrote_line {
        out.push('\n');
    }
    out
}

fn push_unique_trimmed_nonblank_line<'a>(
    out: &mut String,
    line: &'a str,
    seen: &mut std::collections::HashSet<&'a str>,
    wrote_line: &mut bool,
) {
    let trimmed = trim_non_newline_whitespace_end(line);
    if trimmed.chars().all(char::is_whitespace) || !seen.insert(trimmed) {
        return;
    }
    push_nonblank_line(out, trimmed, wrote_line);
}

fn push_trimmed_nonblank_line(out: &mut String, line: &str, wrote_line: &mut bool) {
    let trimmed = trim_non_newline_whitespace_end(line);
    if trimmed.chars().all(char::is_whitespace) {
        return;
    }
    push_nonblank_line(out, trimmed, wrote_line);
}

fn push_nonblank_line(out: &mut String, line: &str, wrote_line: &mut bool) {
    if *wrote_line {
        out.push('\n');
    }
    out.push_str(line);
    *wrote_line = true;
}

fn trim_non_newline_whitespace_end(line: &str) -> &str {
    line.trim_end_matches(|c: char| c.is_whitespace() && c != '\n')
}

/// Fused `CollapseWhitespace`, `TrimTrailingWhitespace`, `RemoveBlankLines`, then
/// `DedupeLines`.
///
/// The borrowed fast path is exact only when collapse would be identity for the
/// whole input. Otherwise, keep the existing zeroized cleaned-line intermediate
/// before dedupe so collapsed dedupe keys never require non-zeroized owned scratch.
fn collapse_trim_remove_blank_then_dedupe_lines(input: &str) -> String {
    if !needs_ascii_collapse(input) {
        return trim_remove_blank_then_dedupe_lines(input);
    }
    let cleaned = Zeroizing::new(collapse_trim_then_remove_blank_lines(input));
    ops::lines::dedupe_lines(&cleaned)
}

/// Fused `CollapseWhitespace` followed by `TrimTrailingWhitespace` and
/// `RemoveBlankLines`.
///
/// This extends the two-op line cleanup fusion to the default pipeline's common
/// three-op suffix. It keeps a reusable per-line collapse scratch allocation inside
/// `Zeroizing` storage rather than materializing and zeroizing the full collapse
/// output before trimming/removing.
fn collapse_trim_then_remove_blank_lines(input: &str) -> String {
    let mut preserve_trailing_newline = input.ends_with('\n');
    let mut out = String::with_capacity(input.len());
    let mut wrote_line = false;
    let mut saw_newline = false;
    let mut start = 0usize;
    let mut collapsed = Zeroizing::new(Vec::new());
    for (i, ch) in input.char_indices() {
        if ch == '\n' {
            saw_newline = true;
            push_collapsed_trimmed_nonblank_line(
                &mut out,
                &input[start..i],
                &mut collapsed,
                &mut wrote_line,
            );
            start = i + 1;
        }
    }
    if !preserve_trailing_newline {
        let final_line = collapse_line(&input[start..], &mut collapsed);
        let final_line = trim_non_newline_whitespace_end(final_line);
        if final_line.chars().all(char::is_whitespace) {
            preserve_trailing_newline = saw_newline;
        } else {
            push_nonblank_line(&mut out, final_line, &mut wrote_line);
        }
    }
    if preserve_trailing_newline && wrote_line {
        out.push('\n');
    }
    out
}

fn push_collapsed_trimmed_nonblank_line(
    out: &mut String,
    line: &str,
    collapsed: &mut Vec<u8>,
    wrote_line: &mut bool,
) {
    let collapsed = collapse_line(line, collapsed);
    let trimmed = trim_non_newline_whitespace_end(collapsed);
    if trimmed.chars().all(char::is_whitespace) {
        return;
    }
    push_nonblank_line(out, trimmed, wrote_line);
}

fn prepare_collapse_scratch(scratch: &mut Vec<u8>, needed: usize) {
    if needed > scratch.capacity() {
        // Capacity growth can free the old allocation. Wipe the current allocation
        // first; otherwise stale clipboard-derived bytes could be left behind in
        // allocator-owned memory.
        scratch.zeroize();
    } else {
        // The old bytes remain inside this transform-owned allocation and are wiped
        // by the surrounding `Zeroizing<Vec<u8>>` on drop.
        scratch.clear();
    }
    scratch.reserve(needed);
}

fn collapse_line<'a>(line: &'a str, scratch: &'a mut Vec<u8>) -> &'a str {
    if !needs_ascii_collapse(line) {
        return line;
    }
    prepare_collapse_scratch(scratch, line.len());
    let mut in_run = false;
    for &byte in line.as_bytes() {
        if byte == b' ' || byte == b'\t' {
            if !in_run {
                scratch.push(b' ');
                in_run = true;
            }
        } else {
            in_run = false;
            scratch.push(byte);
        }
    }
    match std::str::from_utf8(scratch) {
        Ok(collapsed) => collapsed,
        Err(_) => line,
    }
}

fn needs_ascii_collapse(line: &str) -> bool {
    let mut previous_space = false;
    for &byte in line.as_bytes() {
        if byte == b'\t' {
            return true;
        }
        if byte == b' ' {
            if previous_space {
                return true;
            }
            previous_space = true;
        } else {
            previous_space = false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `needs_ascii_collapse` gates the borrowed fast path in [`collapse_line`] and
    /// the zero-copy branch of [`collapse_trim_remove_blank_then_dedupe_lines`].
    /// Both branches emit identical bytes, so the predicate's result — not output —
    /// is the observable contract and is pinned directly: a wrong `true` silently
    /// retires the zero-copy path, a wrong `false` skips collapsing entirely.
    #[test]
    fn needs_ascii_collapse_truth_table() {
        // No tab and no two-space run: collapsing would be the identity. The last
        // two cases are non-ASCII whitespace (NBSP), which collapse never touches.
        for line in [
            "",
            "a",
            "lone",
            "a b c",
            " a",
            "a ",
            "caf\u{e9} x",
            "\u{a0}\u{a0}",
        ] {
            assert!(!needs_ascii_collapse(line), "{line:?} needs no collapse");
        }
        // Any tab, or any run of two-plus spaces, requires the collapsing path.
        for line in ["\t", "a\tb", "a  b", "  ", "x  ", "a \t"] {
            assert!(needs_ascii_collapse(line), "{line:?} requires collapse");
        }
    }

    /// The fast path's zero-copy contract: a line needing no collapse comes back as
    /// the input slice itself (no clipboard-derived copy exists to wipe), and the
    /// scratch buffer is left untouched.
    #[test]
    fn collapse_line_fast_path_returns_the_input_slice() {
        let mut scratch: Vec<u8> = Vec::new();
        let line = "a b c";
        let out = collapse_line(line, &mut scratch);
        assert_eq!(out, line);
        assert_eq!(
            out.as_ptr(),
            line.as_ptr(),
            "fast path must borrow the input, not copy it"
        );
        assert!(scratch.is_empty(), "fast path must not touch the scratch");
    }

    /// Scratch reuse across lines: every call must observe an empty buffer
    /// (`prepare_collapse_scratch` clears or wipes it first), whether the next
    /// line's `needed` is equal to, above, or below the current capacity. Which
    /// branch ran (wipe vs. clear) is invisible through output — safe code cannot
    /// read freed or spare-capacity bytes — so the wipe-before-growth comparison
    /// itself is pinned verbatim by `validate_pipeline_zeroization` in xtask, whose
    /// marker tests run as part of the workspace suite.
    #[test]
    fn collapse_scratch_reuse_is_clean_across_lines() {
        let mut scratch: Vec<u8> = Vec::new();
        assert_eq!(collapse_line("a  b", &mut scratch), "a b");
        // Same `needed` as the previous line (the equality boundary).
        assert_eq!(collapse_line("c\t\td", &mut scratch), "c d");
        // Longer line: the wipe-then-grow branch.
        assert_eq!(collapse_line("ee  ff  gg", &mut scratch), "ee ff gg");
        // Shorter line: in-place reuse of the larger allocation.
        assert_eq!(collapse_line("h\ti", &mut scratch), "h i");
    }
}
