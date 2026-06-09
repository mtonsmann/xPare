//! Golden (exact-output) tests for the whitespace/case/line operations owned by A2.
//!
//! Each case pins the documented behavior of one op on a representative (and where
//! relevant, adversarial) input. These run the ops through the public `transform`
//! pipeline with a single-operation config, so they also exercise dispatch.

use xpare_core::{transform, CaseKind, Config, Operation};

/// Build a single-operation config. Order is irrelevant for one op, but use
/// `as_given` so these goldens stay independent of the canonical-order policy.
fn cfg(op: Operation) -> Config {
    Config::as_given(vec![op])
}

/// Run one operation over `input`.
fn run(op: Operation, input: &str) -> String {
    transform(input, &cfg(op))
}

// ---------------------------------------------------------------------------
// collapse_whitespace
// ---------------------------------------------------------------------------

#[test]
fn collapse_whitespace_runs_to_single_space() {
    assert_eq!(run(Operation::CollapseWhitespace, "a   b\t\t c"), "a b c");
}

#[test]
fn collapse_whitespace_preserves_newlines_and_does_not_trim() {
    // Newlines untouched; trailing/leading runs collapse to one space (not removed).
    assert_eq!(run(Operation::CollapseWhitespace, "a  \n  b   "), "a \n b ");
}

#[test]
fn collapse_whitespace_keeps_cr_verbatim() {
    // '\r' is not collapsed; spaces around it collapse to one each.
    assert_eq!(run(Operation::CollapseWhitespace, "a  \r\nb"), "a \r\nb");
}

#[test]
fn collapse_whitespace_leaves_unicode_space_alone() {
    // No-break space (U+00A0) is not ASCII space/tab, so it is preserved.
    assert_eq!(
        run(Operation::CollapseWhitespace, "a\u{00a0}\u{00a0}b"),
        "a\u{00a0}\u{00a0}b"
    );
}

#[test]
fn collapse_whitespace_empty() {
    assert_eq!(run(Operation::CollapseWhitespace, ""), "");
}

#[test]
fn collapse_whitespace_already_collapsed_text_is_identity() {
    let input = "alpha beta\ncarriage\rreturn\nunicode\u{00a0}\u{00a0}space";
    assert_eq!(run(Operation::CollapseWhitespace, input), input);
}

#[test]
fn collapse_whitespace_collapses_tabs_after_single_spaces() {
    assert_eq!(
        run(Operation::CollapseWhitespace, "alpha \t beta\tgamma"),
        "alpha beta gamma"
    );
}

// ---------------------------------------------------------------------------
// trim_trailing_whitespace
// ---------------------------------------------------------------------------

#[test]
fn trim_trailing_basic() {
    assert_eq!(
        run(Operation::TrimTrailingWhitespace, "a  \nb\t\nc"),
        "a\nb\nc"
    );
}

#[test]
fn trim_trailing_preserves_leading_and_interior() {
    assert_eq!(
        run(Operation::TrimTrailingWhitespace, "  a  b   "),
        "  a  b"
    );
}

#[test]
fn trim_trailing_normalizes_crlf_to_lf() {
    // The '\r' before '\n' is trailing non-newline whitespace, so it is trimmed.
    assert_eq!(
        run(Operation::TrimTrailingWhitespace, "a\r\nb\r\n"),
        "a\nb\n"
    );
}

#[test]
fn trim_trailing_preserves_trailing_newline_and_blank_lines() {
    assert_eq!(
        run(Operation::TrimTrailingWhitespace, "a  \n  \nb\n"),
        "a\n\nb\n"
    );
}

#[test]
fn trim_trailing_unicode_whitespace_trimmed() {
    // No-break space is whitespace and (being non-newline) is trimmed at line end.
    assert_eq!(
        run(Operation::TrimTrailingWhitespace, "a\u{00a0}\u{00a0}"),
        "a"
    );
}

#[test]
fn trim_trailing_unicode_whitespace_before_newline() {
    assert_eq!(
        run(Operation::TrimTrailingWhitespace, "a\u{00a0}\nb\u{2003}\n"),
        "a\nb\n"
    );
}

#[test]
fn trim_trailing_multi_cr_and_final_cr() {
    assert_eq!(run(Operation::TrimTrailingWhitespace, "a\r\r\nb\r"), "a\nb");
}

#[test]
fn trim_trailing_all_blank_preserves_newlines() {
    assert_eq!(run(Operation::TrimTrailingWhitespace, "  \n\t\n"), "\n\n");
}

// ---------------------------------------------------------------------------
// remove_blank_lines
// ---------------------------------------------------------------------------

#[test]
fn remove_blank_lines_drops_empty_and_whitespace_only() {
    assert_eq!(
        run(Operation::RemoveBlankLines, "a\n\n  \nb\n\t\nc"),
        "a\nb\nc"
    );
}

#[test]
fn remove_blank_lines_preserves_trailing_newline() {
    assert_eq!(run(Operation::RemoveBlankLines, "a\n\nb\n"), "a\nb\n");
}

#[test]
fn remove_blank_lines_all_blank_is_empty() {
    assert_eq!(run(Operation::RemoveBlankLines, "\n  \n\t\n"), "");
}

// ---------------------------------------------------------------------------
// unwrap_lines
// ---------------------------------------------------------------------------

#[test]
fn unwrap_lines_joins_paragraph_with_single_space() {
    assert_eq!(
        run(Operation::UnwrapLines, "The quick\nbrown\nfox"),
        "The quick brown fox"
    );
}

#[test]
fn unwrap_lines_separates_paragraphs_with_one_blank_line() {
    assert_eq!(
        run(Operation::UnwrapLines, "one\ntwo\n\nthree\nfour"),
        "one two\n\nthree four"
    );
}

#[test]
fn unwrap_lines_collapses_consecutive_blanks_and_trims_seam() {
    // Multiple blank lines collapse to one separator; whitespace at the join seam
    // (trailing on "two  ", leading on "  three") is trimmed -> single space.
    assert_eq!(
        run(Operation::UnwrapLines, "one\ntwo  \n\n\n  three\nfour"),
        "one two\n\nthree four"
    );
}

#[test]
fn unwrap_lines_strips_crlf_and_multi_cr_line_endings() {
    assert_eq!(
        run(Operation::UnwrapLines, "one\r\r\ntwo\r\n\nthree\r"),
        "one two\n\nthree"
    );
}

#[test]
fn unwrap_lines_strips_leading_and_trailing_blanks_no_trailing_newline() {
    assert_eq!(
        run(Operation::UnwrapLines, "\n\nhello\nworld\n\n"),
        "hello world"
    );
}

#[test]
fn unwrap_lines_empty_and_all_blank() {
    assert_eq!(run(Operation::UnwrapLines, ""), "");
    assert_eq!(run(Operation::UnwrapLines, "\n  \n\t\n"), "");
}

// ---------------------------------------------------------------------------
// change_case: Upper / Lower
// ---------------------------------------------------------------------------

#[test]
fn case_upper_ascii() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Upper
            },
            "abc xyz 123!"
        ),
        "ABC XYZ 123!"
    );
}

#[test]
fn case_upper_unicode_expanding() {
    // German eszett uppercases to two chars; this is why we use full Unicode mapping.
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Upper
            },
            "straße"
        ),
        "STRASSE"
    );
}

#[test]
fn case_lower_ascii() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Lower
            },
            "ABC XYZ 123!"
        ),
        "abc xyz 123!"
    );
}

#[test]
fn case_lower_unicode() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Lower
            },
            "GRÜßE"
        ),
        "grüße"
    );
}

#[test]
fn case_lower_unicode_expanding_fallback() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Lower
            },
            "İ"
        ),
        "i\u{307}"
    );
}

// ---------------------------------------------------------------------------
// change_case: Title
// ---------------------------------------------------------------------------

#[test]
fn case_title_basic() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Title
            },
            "hello world"
        ),
        "Hello World"
    );
}

#[test]
fn case_title_lowercases_rest_of_word() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Title
            },
            "hELLO wORLD"
        ),
        "Hello World"
    );
}

#[test]
fn case_title_preserves_whitespace_runs_and_punct() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Title
            },
            "  foo\tbar-baz  "
        ),
        "  Foo\tBar-baz  "
    );
}

#[test]
fn case_title_leading_punct_in_word() {
    // Word starts with '(' which has no uppercase; rest is lowercased.
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Title
            },
            "(HELLO) 3RD"
        ),
        "(hello) 3rd"
    );
}

// ---------------------------------------------------------------------------
// change_case: Sentence
// ---------------------------------------------------------------------------

#[test]
fn case_sentence_basic() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "hello world. this IS a test! really? yes"
        ),
        "Hello world. This is a test! Really? Yes"
    );
}

#[test]
fn case_sentence_does_not_split_on_intraword_dot() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "e.g. the value is 3.14 ok"
        ),
        "E.g. The value is 3.14 ok"
    );
}

#[test]
fn case_sentence_unicode_capital_clears_expectation() {
    // A single-char non-ASCII uppercase mapping (é -> É) must clear `expect_capital`, so
    // the following letter is lowercased rather than also capitalized. Guards the
    // `has_mapping = first != ch` flag in push_unicode_upper (mutation survivor: != -> ==,
    // which would leave expectation set and yield "ÉA. Next").
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "éa. next"
        ),
        "Éa. Next"
    );
}

#[test]
fn case_sentence_capital_after_terminator_with_leading_punct() {
    // "!!! " is a terminator immediately followed by whitespace, so a new sentence
    // begins and "now" is capitalized.
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "go!!! now"
        ),
        "Go!!! Now"
    );
    // Terminator then whitespace then punctuation then letter: the *letter* is the
    // first cased char of the new sentence and is capitalized.
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "done. \"next\" thing"
        ),
        "Done. \"Next\" thing"
    );
}

#[test]
fn case_sentence_newline_is_whitespace_after_terminator() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "first.\nsecond"
        ),
        "First.\nSecond"
    );
}

#[test]
fn case_sentence_unicode_lowercase_expansion() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "İST. straße"
        ),
        "I\u{307}st. Straße"
    );
}

#[test]
fn case_sentence_unicode_uppercase_expansion() {
    assert_eq!(
        run(
            Operation::ChangeCase {
                case: CaseKind::Sentence
            },
            "ßeta. next"
        ),
        "SSeta. Next"
    );
}

// ---------------------------------------------------------------------------
// sort_lines
// ---------------------------------------------------------------------------

#[test]
fn sort_lines_ascending() {
    assert_eq!(
        run(
            Operation::SortLines {
                descending: false,
                case_insensitive: false
            },
            "banana\napple\ncherry"
        ),
        "apple\nbanana\ncherry"
    );
}

#[test]
fn sort_lines_descending() {
    assert_eq!(
        run(
            Operation::SortLines {
                descending: true,
                case_insensitive: false
            },
            "banana\napple\ncherry"
        ),
        "cherry\nbanana\napple"
    );
}

#[test]
fn sort_lines_case_sensitive_orders_uppercase_first() {
    // ASCII: uppercase letters sort before lowercase.
    assert_eq!(
        run(
            Operation::SortLines {
                descending: false,
                case_insensitive: false
            },
            "banana\nApple\ncherry"
        ),
        "Apple\nbanana\ncherry"
    );
}

#[test]
fn sort_lines_case_insensitive_preserves_original_casing() {
    assert_eq!(
        run(
            Operation::SortLines {
                descending: false,
                case_insensitive: true
            },
            "banana\nApple\nCHERRY"
        ),
        "Apple\nbanana\nCHERRY"
    );
}

#[test]
fn sort_lines_is_stable_for_equal_keys() {
    // Case-insensitive: the two "foo" spellings are equal keys and keep input order.
    assert_eq!(
        run(
            Operation::SortLines {
                descending: false,
                case_insensitive: true
            },
            "Foo\nfoo\nbar"
        ),
        "bar\nFoo\nfoo"
    );
}

#[test]
fn sort_lines_preserves_trailing_newline() {
    assert_eq!(
        run(
            Operation::SortLines {
                descending: false,
                case_insensitive: false
            },
            "b\na\n"
        ),
        "a\nb\n"
    );
}

// ---------------------------------------------------------------------------
// dedupe_lines
// ---------------------------------------------------------------------------

#[test]
fn dedupe_lines_keeps_first_occurrence_and_order() {
    assert_eq!(run(Operation::DedupeLines, "a\nb\na\nc\nb\na"), "a\nb\nc");
}

#[test]
fn dedupe_lines_is_case_sensitive() {
    assert_eq!(run(Operation::DedupeLines, "A\na\nA"), "A\na");
}

#[test]
fn dedupe_lines_preserves_trailing_newline() {
    assert_eq!(run(Operation::DedupeLines, "a\na\n"), "a\n");
}

// ---------------------------------------------------------------------------
// prefix_lines / suffix_lines
// ---------------------------------------------------------------------------

#[test]
fn prefix_lines_skips_empty_lines() {
    assert_eq!(
        run(
            Operation::PrefixLines {
                prefix: "> ".into()
            },
            "a\n\nb"
        ),
        "> a\n\n> b"
    );
}

#[test]
fn prefix_lines_prefixes_whitespace_only_lines() {
    // A whitespace-only line is non-empty, so it gets the prefix.
    assert_eq!(
        run(
            Operation::PrefixLines {
                prefix: "> ".into()
            },
            "a\n \nb"
        ),
        "> a\n>  \n> b"
    );
}

#[test]
fn suffix_lines_skips_empty_lines() {
    assert_eq!(
        run(Operation::SuffixLines { suffix: ";".into() }, "a\n\nb"),
        "a;\n\nb;"
    );
}

#[test]
fn prefix_lines_preserves_trailing_newline() {
    assert_eq!(
        run(
            Operation::PrefixLines {
                prefix: "- ".into()
            },
            "a\nb\n"
        ),
        "- a\n- b\n"
    );
}

// ---------------------------------------------------------------------------
// join_with / split_on
// ---------------------------------------------------------------------------

#[test]
fn join_with_replaces_newlines() {
    assert_eq!(
        run(
            Operation::JoinWith {
                separator: ", ".into()
            },
            "a\nb\nc"
        ),
        "a, b, c"
    );
}

#[test]
fn join_with_trailing_newline_becomes_trailing_separator() {
    assert_eq!(
        run(
            Operation::JoinWith {
                separator: "-".into()
            },
            "a\n"
        ),
        "a-"
    );
}

#[test]
fn join_with_strips_cr_from_crlf() {
    assert_eq!(
        run(
            Operation::JoinWith {
                separator: "|".into()
            },
            "a\r\nb"
        ),
        "a|b"
    );
}

#[test]
fn split_on_replaces_delimiter_with_newline() {
    assert_eq!(
        run(
            Operation::SplitOn {
                delimiter: ", ".into()
            },
            "a, b, c"
        ),
        "a\nb\nc"
    );
}

#[test]
fn split_on_empty_delimiter_is_noop() {
    assert_eq!(
        run(
            Operation::SplitOn {
                delimiter: String::new()
            },
            "abc"
        ),
        "abc"
    );
}

// ---------------------------------------------------------------------------
// extract_emails / extract_urls
// ---------------------------------------------------------------------------

#[test]
fn extract_emails_basic_and_trims_punctuation() {
    assert_eq!(
        run(
            Operation::ExtractEmails,
            "Contact <alice@example.com>, or bob@mail.co.uk please. notme@ x@y"
        ),
        "alice@example.com\nbob@mail.co.uk"
    );
}

#[test]
fn extract_emails_none_matches_is_empty() {
    assert_eq!(run(Operation::ExtractEmails, "no emails here at all"), "");
}

#[test]
fn extract_urls_basic() {
    assert_eq!(
        run(
            Operation::ExtractUrls,
            "see https://example.com/path and (http://a.b) or www.site.org now"
        ),
        "https://example.com/path\nhttp://a.b\nwww.site.org"
    );
}

#[test]
fn extract_urls_none_matches_is_empty() {
    assert_eq!(run(Operation::ExtractUrls, "ftp://x not matched here"), "");
}
