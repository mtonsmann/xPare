//! Line operations and best-effort extraction.
//!
//! **Implementation owner: pipeline stream (A2).**
//!
//! ## Line model (shared with `ops::whitespace`, documented once here)
//!
//! Text is a sequence of lines separated by `'\n'`. We split *only* on `'\n'`.
//! Any trailing run of `'\r'` immediately preceding a `'\n'` is treated as part of
//! the line ending (CRLF, and its degenerate multi-`\r` forms) — line content
//! excludes those trailing `'\r'`s — but a `'\r'` elsewhere (interior or with no
//! following `'\n'`) is an ordinary character, never a line break. Stripping the
//! whole run (not just one `\r`) is what keeps the content-preserving line ops
//! idempotent.
//!
//! ### Trailing-newline preservation
//!
//! Two split helpers encode the same line model with different end-of-text handling:
//!
//! * [`split_lines`] is the faithful split on `'\n'`: a trailing `'\n'` yields a final
//!   *empty* fragment, so `"a\n"` is `["a", ""]`. [`join_with`] uses this (it is a
//!   literal "replace each `'\n'`").
//! * [`content_lines`] is the "line list" view: a trailing `'\n'` is recorded as a
//!   boolean and the empty trailing fragment is dropped, so `"a\n"` is
//!   `(["a"], true)`. The line-structure-preserving ops ([`remove_blank_lines`],
//!   [`sort_lines`], [`dedupe_lines`], [`prefix_lines`], [`suffix_lines`]) use this
//!   and re-emit one trailing `'\n'` iff the flag is set, so a trailing newline
//!   round-trips exactly without spawning a spurious empty last line.
//!
//! [`unwrap_lines`] and the extractors document their own newline behavior on each
//! function.
//!
//! All functions iterate by `char`/line and never index into byte offsets, so they
//! are panic-free on any input and run in linear time.

use std::collections::HashSet;

/// Split `input` into line *contents* on `'\n'`, stripping any trailing run of `'\r'`
/// (CRLF) from each line. The final fragment after the last `'\n'` is always included
/// (it is empty when the input ends with `'\n'`), and an empty input yields a single
/// empty line.
///
/// Splitting scans bytes for `b'\n'` (a single byte that can never be a fragment of a
/// multi-byte UTF-8 char) and slices on those boundaries, so it is panic-free and
/// linear, while iteration of *content* elsewhere is by `char`.
fn split_lines(input: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let bytes = input.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            // Treat any trailing run of '\r' as part of the line ending (CRLF and its
            // degenerate multi-CR forms), so a line's content never ends in '\r'. This
            // is what keeps the content-preserving ops idempotent: a content line
            // re-joined with '\n' can never re-form a CRLF that would re-strip on the
            // next pass (the bug that "\r\r\n" -> "\r\n" -> "\n" would otherwise cause).
            let content = input[start..i].trim_end_matches('\r');
            lines.push(content);
            start = i + 1;
        }
    }
    // Trailing fragment after the last '\n' (or the whole string if no '\n').
    lines.push(&input[start..]);
    lines
}

/// "Line list" view of `input`: the content lines plus whether the input ended with a
/// trailing `'\n'`. Identical to [`split_lines`] except the trailing empty fragment
/// produced by a final `'\n'` is dropped and reported as the boolean instead, so the
/// caller can round-trip the trailing newline without a spurious empty last line.
fn content_lines(input: &str) -> (Vec<&str>, bool) {
    let mut lines = split_lines(input);
    let trailing_newline = input.ends_with('\n');
    if trailing_newline {
        // The faithful split always appends an empty fragment after the final '\n';
        // drop it here so it does not become an extra blank line.
        lines.pop();
    }
    (lines, trailing_newline)
}

/// True if a line is blank: empty or composed solely of whitespace.
fn is_blank(content: &str) -> bool {
    content.chars().all(char::is_whitespace)
}

/// Join already-decided output line *contents* with `'\n'`, re-adding a single
/// trailing `'\n'` iff `trailing_newline` is set. Used by the line-preserving ops so
/// they share one definition of trailing-newline handling.
fn join_lines(contents: &[&str], trailing_newline: bool) -> String {
    let body_len: usize = contents.iter().map(|c| c.len()).sum();
    let separators = contents.len().saturating_sub(1);
    let mut out = String::with_capacity(body_len + separators + usize::from(trailing_newline));
    for (i, c) in contents.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(c);
    }
    if trailing_newline {
        out.push('\n');
    }
    out
}

/// Remove blank/empty lines (lines that are empty or whitespace-only); preserve the
/// relative order of the remaining lines.
///
/// Documented behavior:
/// * "Blank" = empty or only-whitespace (`char::is_whitespace`), so a line of spaces
///   or a lone `\r` is removed.
/// * A trailing newline is preserved iff the input had one *and* at least one line
///   survives. If every line is blank the result is the empty string (no stray
///   newline).
pub fn remove_blank_lines(input: &str) -> String {
    let (lines, trailing_newline) = content_lines(input);
    let kept: Vec<&str> = lines.into_iter().filter(|c| !is_blank(c)).collect();
    if kept.is_empty() {
        return String::new();
    }
    join_lines(&kept, trailing_newline)
}

/// Join wrapped lines into paragraphs, preserving paragraph breaks.
///
/// EXACT rule (documented):
/// * A blank line (empty or whitespace-only) is a **paragraph separator**.
///   Consecutive blank lines collapse to a single separator.
/// * Consecutive non-blank lines are joined into one line, with a single `' '`
///   between them; each joined piece is trimmed at the seam (leading/trailing
///   whitespace of each contributing line is dropped) so no double spaces appear.
/// * Paragraphs in the output are separated by exactly one blank line, i.e. `"\n\n"`.
/// * Leading and trailing blank lines produce no leading/trailing separators.
/// * Trailing newline: the output has **no** trailing newline (it is a clean
///   paragraph block). This is intentional and documented — unwrap produces a
///   normalized block, not a line list.
pub fn unwrap_lines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_paragraph = false;
    let mut pending_separator = false;
    let mut wrote_paragraph = false;
    let mut start = 0usize;
    for (i, &b) in input.as_bytes().iter().enumerate() {
        if b == b'\n' {
            let line = input[start..i].trim_end_matches('\r');
            push_unwrapped_line(
                &mut out,
                line,
                &mut in_paragraph,
                &mut pending_separator,
                &mut wrote_paragraph,
            );
            start = i + 1;
        }
    }
    push_unwrapped_line(
        &mut out,
        &input[start..],
        &mut in_paragraph,
        &mut pending_separator,
        &mut wrote_paragraph,
    );
    out
}

fn push_unwrapped_line(
    out: &mut String,
    line: &str,
    in_paragraph: &mut bool,
    pending_separator: &mut bool,
    wrote_paragraph: &mut bool,
) {
    let piece = line.trim();
    if piece.is_empty() {
        if *in_paragraph {
            *in_paragraph = false;
            *pending_separator = true;
        }
        return;
    }

    if *in_paragraph {
        out.push(' ');
    } else if *pending_separator && *wrote_paragraph {
        out.push_str("\n\n");
        *pending_separator = false;
    }
    out.push_str(piece);
    *in_paragraph = true;
    *wrote_paragraph = true;
}

/// Sort lines. `descending` reverses the final order; `case_insensitive` folds case
/// for the comparison only (output preserves each line's original casing).
///
/// Documented behavior:
/// * The sort is **stable**: lines that compare equal keep their input order.
/// * Comparison is by Unicode scalar order of the (optionally case-folded) content.
///   Case folding uses full Unicode lowercase of the whole line for comparison.
/// * The line set is exactly the split lines (a trailing newline does not create an
///   extra empty line beyond the trailing fragment). A trailing newline in the input
///   is preserved in the output.
pub fn sort_lines(input: &str, descending: bool, case_insensitive: bool) -> String {
    let (lines, trailing_newline) = content_lines(input);
    let sorted: Vec<&str> = if case_insensitive {
        // Case-insensitive: precompute one folded key per line so the comparator does
        // no per-comparison work. The keys cost ~input size in memory — that is the
        // price of case folding; the case-sensitive path below allocates nothing.
        let mut keyed: Vec<(String, &str)> = lines
            .iter()
            .map(|&content| {
                let key: String = content.chars().flat_map(char::to_lowercase).collect();
                (key, content)
            })
            .collect();
        keyed.sort_by(|a, b| orient(descending, a.0.cmp(&b.0)));
        keyed.into_iter().map(|(_, content)| content).collect()
    } else {
        // Case-sensitive: sort the borrowed slices directly. No per-line key
        // allocation, so memory stays O(number of lines), not O(input bytes) — this
        // is what keeps large inputs (e.g. log files) sorting in bounded extra space.
        let mut sorted = lines;
        sorted.sort_by(|a, b| orient(descending, a.cmp(b)));
        sorted
    };
    join_lines(&sorted, trailing_newline)
}

/// Apply sort direction to a comparison result. Used with a *stable* `sort_by` so
/// that equal lines keep their original relative order in **both** directions
/// (ascending and descending), rather than ascending-then-`reverse()` which would
/// flip the order of equal runs.
fn orient(descending: bool, ord: std::cmp::Ordering) -> std::cmp::Ordering {
    if descending {
        ord.reverse()
    } else {
        ord
    }
}

/// Remove duplicate lines, keeping the first occurrence and original order.
///
/// Documented behavior:
/// * Duplicate = identical line content (exact, case-sensitive, post-CRLF-strip).
/// * A [`HashSet`] is used only for O(1) membership; output is emitted in original
///   order, so the result is fully deterministic despite the hash set.
/// * A trailing newline in the input is preserved.
pub fn dedupe_lines(input: &str) -> String {
    let (lines, trailing_newline) = content_lines(input);
    let mut seen: HashSet<&str> = HashSet::with_capacity(lines.len());
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    for line in lines {
        if seen.insert(line) {
            kept.push(line);
        }
    }
    join_lines(&kept, trailing_newline)
}

/// Prefix every non-empty line with `prefix`.
///
/// Documented behavior:
/// * "Non-empty" means the line content is not the empty string. A whitespace-only
///   line (e.g. `"   "`) is *not* empty and **is** prefixed; only truly empty lines
///   (zero chars) are left untouched, so blank-line spacing is preserved without a
///   dangling prefix on empty lines.
/// * Line structure and a trailing newline are preserved.
pub fn prefix_lines(input: &str, prefix: &str) -> String {
    affix_lines(input, prefix, "")
}

/// Suffix every non-empty line with `suffix`.
///
/// Documented behavior: mirror of [`prefix_lines`] — empty lines (zero chars) are
/// left untouched; whitespace-only lines are suffixed. Trailing newline preserved.
pub fn suffix_lines(input: &str, suffix: &str) -> String {
    affix_lines(input, "", suffix)
}

/// Shared implementation for [`prefix_lines`]/[`suffix_lines`]: wrap each non-empty
/// line with `prefix` ... `suffix`.
fn affix_lines(input: &str, prefix: &str, suffix: &str) -> String {
    let (lines, trailing_newline) = content_lines(input);
    let mut out = String::with_capacity(input.len() + lines.len() * (prefix.len() + suffix.len()));
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            // Leave genuinely empty lines untouched.
            continue;
        }
        out.push_str(prefix);
        out.push_str(line);
        out.push_str(suffix);
    }
    if trailing_newline {
        out.push('\n');
    }
    out
}

/// Join all lines into one, separated by `separator`: replace each `'\n'` with
/// `separator`.
///
/// Documented behavior:
/// * Every `'\n'` in the input becomes `separator`; CRLF line endings have their
///   `'\r'` stripped first (per the shared line model), so `"a\r\nb"` joins as
///   `"a" + separator + "b"`.
/// * Because each `'\n'` becomes a separator, a trailing newline becomes a trailing
///   separator (the final empty fragment is joined too). E.g. `join_with("a\n", "-")`
///   == `"a-"`.
pub fn join_with(input: &str, separator: &str) -> String {
    let lines = split_lines(input);
    lines.join(separator)
}

/// Split on a custom delimiter: replace each occurrence of `delimiter` with `'\n'`.
///
/// Documented behavior:
/// * An **empty** `delimiter` is a no-op (returns the input unchanged) — splitting on
///   "" is undefined/degenerate, so we deliberately do nothing.
/// * Otherwise every (non-overlapping, left-to-right) occurrence of `delimiter` is
///   replaced by a single `'\n'`. Existing newlines are left as-is.
pub fn split_on(input: &str, delimiter: &str) -> String {
    if delimiter.is_empty() {
        return input.to_string();
    }
    input.replace(delimiter, "\n")
}

/// Extract email-like tokens, one per line.
///
/// **Heuristic, not RFC 5322.** Documented rule:
/// * Tokenize the input on whitespace (`char::is_whitespace`).
/// * A token is an email iff, after trimming a small set of surrounding punctuation
///   (`< > ( ) [ ] { } , ; : " '`), it contains exactly one `'@'`, has a non-empty
///   local part before it, and has a domain after it that contains a `'.'` which is
///   neither the first nor last char of the domain.
/// * Output: each matched (trimmed) token on its own line, in first-seen order, with
///   no trailing newline. Duplicates are *not* removed (compose with `dedupe_lines`).
///   If nothing matches, the result is the empty string.
pub fn extract_emails(input: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for raw in input.split_whitespace() {
        let token = trim_token_punct(raw);
        if is_email(token) {
            out.push(token);
        }
    }
    out.join("\n")
}

/// Extract URL-like tokens, one per line.
///
/// **Heuristic, not a URL parser.** Documented rule:
/// * Tokenize on whitespace.
/// * A token is a URL iff, after trimming surrounding punctuation, it starts with
///   `"http://"`, `"https://"`, or `"www."` (scheme/prefix match is case-sensitive)
///   and has at least one more char after the prefix.
/// * Output: each matched (trimmed) token on its own line, first-seen order, no
///   trailing newline; duplicates kept. Empty result if nothing matches.
pub fn extract_urls(input: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for raw in input.split_whitespace() {
        let token = trim_token_punct(raw);
        if is_url(token) {
            out.push(token);
        }
    }
    out.join("\n")
}

/// Trim a small, fixed set of surrounding punctuation/brackets/quotes from a token.
/// Operates on `char` boundaries via `trim_matches`, so it is panic-free.
///
/// `pub(crate)` so the defang/url-clean ops share the exact same tokenization edge.
pub(crate) fn trim_token_punct(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '"' | '\''
        )
    })
}

/// Email heuristic: see [`extract_emails`] for the documented rule.
pub(crate) fn is_email(token: &str) -> bool {
    // Exactly one '@', non-empty local part, domain with an interior '.'.
    let mut parts = token.split('@');
    let local = match parts.next() {
        Some(l) => l,
        None => return false,
    };
    let domain = match parts.next() {
        Some(d) => d,
        None => return false,
    };
    if parts.next().is_some() {
        // More than one '@'.
        return false;
    }
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    match domain.find('.') {
        Some(dot) => dot > 0 && dot < domain.len() - 1,
        None => false,
    }
}

/// URL heuristic: see [`extract_urls`] for the documented rule.
pub(crate) fn is_url(token: &str) -> bool {
    for prefix in ["http://", "https://", "www."] {
        if let Some(rest) = token.strip_prefix(prefix) {
            if !rest.is_empty() {
                return true;
            }
        }
    }
    false
}
