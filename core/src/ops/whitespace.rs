//! Whitespace normalization.
//!
//! **Implementation owner: pipeline stream (A2).**
//!
//! Line model (shared across `ops::whitespace` and `ops::lines`): the text is a
//! sequence of `\n`-separated lines. A `\r` immediately preceding a `\n` is treated
//! as part of the line ending (CRLF), and a `\r` is *not* itself a line separator
//! anywhere else — only `\n` splits lines. These operations work intra-line by
//! iterating over `char`s, so they never panic on adversarial input (no indexing
//! into byte offsets that might fall on a UTF-8 boundary), and run in linear time.

/// Collapse each maximal run of spaces/tabs into a single space.
///
/// Rules (documented, exact):
/// * Only ASCII space (`' '`) and tab (`'\t'`) are collapsed. Every maximal run of
///   one-or-more of these is replaced by a single `' '`.
/// * `'\n'` is never touched, so line structure is preserved exactly (including a
///   trailing newline, CRLF endings, and blank lines).
/// * `'\r'` is treated as an ordinary, non-collapsible character and is emitted
///   verbatim. In practice it only appears just before a `'\n'` (CRLF), where it is
///   preserved; a run like `" \r "` becomes `" \r "` -> `" \r "` collapses the
///   spaces around it but keeps the `\r`.
/// * Does **not** trim line ends: a line that is all spaces collapses to a single
///   space, and trailing spaces collapse to one space (use
///   [`trim_trailing_whitespace`] to remove them).
/// * Other Unicode whitespace (e.g. no-break space, full-width space) is left
///   untouched on purpose; "whitespace" here means the ASCII space/tab a wrapped
///   clipboard paste actually produces. (Documented heuristic; see DESIGN.md.)
pub fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_run = false;
    for ch in input.chars() {
        if ch == ' ' || ch == '\t' {
            if !in_run {
                out.push(' ');
                in_run = true;
            }
            // else: inside an existing run — drop the character.
        } else {
            in_run = false;
            out.push(ch);
        }
    }
    out
}

/// Trim trailing whitespace from each line, preserving line breaks and content.
///
/// Rules (documented, exact):
/// * A "line" is a maximal run of characters with no `'\n'`. For each line, trailing
///   whitespace is removed; the `'\n'` separators themselves are preserved exactly,
///   so the number of lines and the position of every newline is unchanged.
/// * "Whitespace" trimmed here is *non-newline* Unicode whitespace
///   (`char::is_whitespace()` minus `'\n'`). This removes trailing spaces, tabs,
///   carriage returns, no-break spaces, etc.
/// * Because a `\r` before a `\n` is trailing non-newline whitespace, CRLF line
///   endings are normalized to LF (the `\r` is trimmed). This is intentional and
///   documented: trailing-whitespace removal subsumes CRLF→LF here.
/// * A trailing newline in the input is preserved (the final, possibly-empty line
///   after the last `'\n'` is still trimmed, but no newline is added or removed).
/// * Leading and interior whitespace is untouched.
pub fn trim_trailing_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    // Buffer the current line so we can drop its trailing whitespace before flushing.
    let mut line = String::new();
    for ch in input.chars() {
        if ch == '\n' {
            push_trimmed_end(&mut out, &line);
            out.push('\n');
            line.clear();
        } else {
            line.push(ch);
        }
    }
    push_trimmed_end(&mut out, &line);
    out
}

/// Push `line` onto `out` with trailing non-newline whitespace removed.
fn push_trimmed_end(out: &mut String, line: &str) {
    let trimmed = line.trim_end_matches(|c: char| c.is_whitespace() && c != '\n');
    out.push_str(trimmed);
}
