//! Case transformations.
//!
//! **Implementation owner: pipeline stream (A2).**
//!
//! Full Unicode case-mapping methods ([`char::to_uppercase`] /
//! [`char::to_lowercase`]) are the semantic source of truth. Those methods return
//! iterators because one input `char` can map to several output `char`s (e.g.
//! `'ß'`.to_uppercase() yields `"SS"`, `'İ'` lowercases to `"i\u{307}"`). Whole-text
//! upper/lower have an ASCII byte fast path and fall back to the Unicode path as soon
//! as non-ASCII appears. The implementations are panic-free (no indexing, no
//! `unwrap`) and linear in the number of bytes/chars.
//!
//! Because Unicode case mapping can expand bytes (e.g. `İ` → `i̇`, 2 → 3 bytes),
//! the output can outgrow the `input.len()` starting capacity. Appends therefore
//! go through `ops::wipe`, which wipes a superseded allocation before growth
//! frees it, so no clipboard-derived bytes linger in allocator-owned memory.
//! (The ASCII fast paths map 1:1 and allocate exactly once.)

use crate::ops::wipe::push_char_wiping;
use crate::CaseKind;

/// Recase the whole text according to `kind`.
///
/// See the per-kind helpers for the exact, documented rules.
pub fn change_case(input: &str, kind: CaseKind) -> String {
    match kind {
        CaseKind::Upper => to_upper(input),
        CaseKind::Lower => to_lower(input),
        CaseKind::Title => to_title(input),
        CaseKind::Sentence => to_sentence(input),
    }
}

/// Full Unicode uppercase of the entire text.
fn to_upper(input: &str) -> String {
    if input.is_ascii() {
        return input.to_ascii_uppercase();
    }
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        for mapped in ch.to_uppercase() {
            push_char_wiping(&mut out, mapped);
        }
    }
    out
}

/// Full Unicode lowercase of the entire text.
fn to_lower(input: &str) -> String {
    if input.is_ascii() {
        return input.to_ascii_lowercase();
    }
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        for mapped in ch.to_lowercase() {
            push_char_wiping(&mut out, mapped);
        }
    }
    out
}

/// Title Case.
///
/// Rule (documented, exact):
/// * A "word" is a maximal run of non-whitespace characters
///   (`char::is_whitespace()` is the separator). Whitespace and punctuation are
///   preserved verbatim and in place.
/// * Within each word, the **first** `char` is uppercased (full Unicode) and every
///   subsequent `char` is lowercased (full Unicode).
/// * "First char of a word" is literal: if a word starts with punctuation or a
///   digit (e.g. `"(hello"`, `"3rd"`), that leading char has no uppercase mapping
///   and is emitted unchanged, and the rest of the word is lowercased — so
///   `"(HELLO)"` -> `"(hello)"` and `"3RD"` -> `"3rd"`. This is intentional: we do
///   not hunt for "the first letter"; we case by position. (Documented in DESIGN.md.)
fn to_title(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    // `at_word_start` is true at the very start and after any whitespace char.
    let mut at_word_start = true;
    for ch in input.chars() {
        if ch.is_whitespace() {
            push_char_wiping(&mut out, ch);
            at_word_start = true;
        } else if at_word_start {
            for mapped in ch.to_uppercase() {
                push_char_wiping(&mut out, mapped);
            }
            at_word_start = false;
        } else {
            for mapped in ch.to_lowercase() {
                push_char_wiping(&mut out, mapped);
            }
        }
    }
    out
}

/// Sentence case.
///
/// Rule (documented, exact):
/// * First, the entire text is lowercased (full Unicode).
/// * Then the first cased letter of each sentence is uppercased. A new sentence
///   begins at the start of the text and after a sentence terminator (`'.'`, `'!'`,
///   or `'?'`) **immediately followed by at least one whitespace char**. While in
///   the "expecting capital" state, the first char that actually has an uppercase
///   mapping (a cased letter) is uppercased; leading punctuation/digits/whitespace
///   are emitted unchanged and the state persists until such a letter is found.
/// * Only `'.'`/`'!'`/`'?'` terminate a sentence, and only when whitespace follows
///   (so `"e.g."` and `"3.14"` do not start a new sentence; `"end. next"` does).
///   Newlines count as whitespace, so a terminator at end-of-line capitalizes the
///   next line's first letter. Example: `"hi! go. ok? yes"` ->
///   `"Hi! Go. Ok? Yes"`; `"!!! go"` -> `"!!! Go"`.
fn to_sentence(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    // `expect_capital` - we are at/seeking the first cased letter of a sentence.
    let mut expect_capital = true;
    // `prev_terminator` - the previous emitted char was `.`/`!`/`?`.
    let mut prev_terminator = false;
    for ch in input.chars() {
        if ch.is_ascii() {
            push_sentence_lowered_char(
                ch.to_ascii_lowercase(),
                &mut out,
                &mut expect_capital,
                &mut prev_terminator,
            );
        } else {
            for lowered in ch.to_lowercase() {
                push_sentence_lowered_char(
                    lowered,
                    &mut out,
                    &mut expect_capital,
                    &mut prev_terminator,
                );
            }
        }
    }
    out
}

fn push_sentence_lowered_char(
    ch: char,
    out: &mut String,
    expect_capital: &mut bool,
    prev_terminator: &mut bool,
) {
    // A sentence boundary is "terminator then whitespace": when we see whitespace
    // right after a terminator, the *next* cased letter starts a new sentence.
    if *prev_terminator && ch.is_whitespace() {
        *expect_capital = true;
    }

    if *expect_capital {
        if ch.is_ascii() {
            push_char_wiping(out, ch.to_ascii_uppercase());
            if ch.is_ascii_alphabetic() {
                *expect_capital = false;
            }
        } else {
            push_unicode_upper(ch, out, expect_capital);
        }
    } else {
        push_char_wiping(out, ch);
    }

    *prev_terminator = matches!(ch, '.' | '!' | '?');
}

fn push_unicode_upper(ch: char, out: &mut String, expect_capital: &mut bool) {
    let mut upper = ch.to_uppercase();
    let first = upper.next().unwrap_or(ch);
    let mut has_mapping = first != ch;
    push_char_wiping(out, first);
    for mapped in upper {
        has_mapping = true;
        push_char_wiping(out, mapped);
    }
    if has_mapping {
        *expect_capital = false;
    }
}
