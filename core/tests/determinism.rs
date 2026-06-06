//! Determinism + never-panics property tests.
//!
//! These are the load-bearing invariant checks for the core's untrusted-input path:
//!
//! * **Determinism:** `transform(x, c) == transform(x, c)` for every input `x` and
//!   config `c`. This would fail if any op leaked hash-set *iteration order* (vs.
//!   hash-set *membership*, which is fine), depended on time/locale, etc.
//! * **Never panics:** `transform` returns for every input, including adversarial
//!   bytes, lone `\r`, mixed CRLF, control chars, and huge whitespace runs. A panic
//!   here would surface as `SS_ERR_INTERNAL` at the FFI boundary, so we forbid it.
//!
//! The strategies deliberately bias toward the bytes that break naive line/case/
//! whitespace handling: newlines, carriage returns, tabs, `@`, `.`, `http`, quotes,
//! and multi-byte / case-expanding Unicode (`ß`, `İ`, `Σ`).

use proptest::prelude::*;
use safetystrip_core::{
    ops, transform, BracketStyle, CaseKind, Config, Operation, Ordering, CONFIG_VERSION,
};

/// A pool of "interesting" characters plus arbitrary chars, so generated strings hit
/// the edges of every op while still exploring the whole `char` space.
fn interesting_char() -> impl Strategy<Value = char> {
    prop_oneof![
        // Structural / whitespace characters the line + whitespace ops care about.
        20 => prop_oneof![
            Just('\n'), Just('\r'), Just(' '), Just('\t'),
            Just('.'), Just('!'), Just('?'),
            Just('@'), Just('/'), Just(':'),
            Just('<'), Just('>'), Just('"'), Just('\''),
            Just(','), Just(';'), Just('('), Just(')'),
        ],
        // ASCII letters/words used by case + extraction heuristics.
        8 => prop::char::range('a', 'z'),
        4 => prop::char::range('A', 'Z'),
        2 => prop::char::range('0', '9'),
        // Case-expanding / non-ASCII to stress full-Unicode case mapping.
        3 => prop_oneof![
            Just('ß'), Just('İ'), Just('Σ'), Just('ﬁ'), Just('é'),
            Just('\u{00a0}'), Just('\u{0307}'), Just('🦀'),
        ],
        // Anything at all, including other control chars and surrog’less scalars.
        4 => any::<char>(),
    ]
}

/// Build interesting strings from the char pool, plus the empty string.
fn interesting_string() -> impl Strategy<Value = String> {
    prop::collection::vec(interesting_char(), 0..80).prop_map(|chars| chars.into_iter().collect())
}

/// Short arbitrary string for op parameters (separators, prefixes, delimiters).
fn param_string() -> impl Strategy<Value = String> {
    prop::collection::vec(interesting_char(), 0..6).prop_map(|chars| chars.into_iter().collect())
}

/// Strategy over every `Operation`, with arbitrary parameters where applicable.
fn operation_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        Just(Operation::StripHtml),
        Just(Operation::StripMarkdown),
        Just(Operation::CollapseWhitespace),
        Just(Operation::TrimTrailingWhitespace),
        Just(Operation::RemoveBlankLines),
        Just(Operation::UnwrapLines),
        prop_oneof![
            Just(CaseKind::Upper),
            Just(CaseKind::Lower),
            Just(CaseKind::Title),
            Just(CaseKind::Sentence),
        ]
        .prop_map(|case| Operation::ChangeCase { case }),
        (any::<bool>(), any::<bool>()).prop_map(|(descending, case_insensitive)| {
            Operation::SortLines {
                descending,
                case_insensitive,
            }
        }),
        Just(Operation::DedupeLines),
        param_string().prop_map(|prefix| Operation::PrefixLines { prefix }),
        param_string().prop_map(|suffix| Operation::SuffixLines { suffix }),
        param_string().prop_map(|separator| Operation::JoinWith { separator }),
        param_string().prop_map(|delimiter| Operation::SplitOn { delimiter }),
        Just(Operation::ExtractEmails),
        Just(Operation::ExtractUrls),
        prop_oneof![Just(BracketStyle::Square), Just(BracketStyle::Round)]
            .prop_map(|style| Operation::Defang { style }),
        Just(Operation::Refang),
        Just(Operation::CleanUrls),
    ]
}

/// Either ordering mode, so the determinism/panic properties cover canonical
/// reordering as well as as-given.
fn ordering_strategy() -> impl Strategy<Value = Ordering> {
    prop_oneof![Just(Ordering::Canonical), Just(Ordering::AsGiven)]
}

/// A config with an arbitrary ordered pipeline (0..8 ops) and either ordering mode.
fn config_strategy() -> impl Strategy<Value = Config> {
    (
        prop::collection::vec(operation_strategy(), 0..8),
        ordering_strategy(),
    )
        .prop_map(|(operations, ordering)| Config {
            version: CONFIG_VERSION,
            operations,
            ordering,
        })
}

fn reference_unwrap_lines(input: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_paragraph = false;
    let mut start = 0usize;

    for (i, &b) in input.as_bytes().iter().enumerate() {
        if b == b'\n' {
            reference_push_unwrapped_line(
                input[start..i].trim_end_matches('\r'),
                &mut paragraphs,
                &mut current,
                &mut in_paragraph,
            );
            start = i + 1;
        }
    }
    reference_push_unwrapped_line(
        &input[start..],
        &mut paragraphs,
        &mut current,
        &mut in_paragraph,
    );
    if in_paragraph {
        paragraphs.push(current);
    }
    paragraphs.join("\n\n")
}

fn reference_push_unwrapped_line(
    line: &str,
    paragraphs: &mut Vec<String>,
    current: &mut String,
    in_paragraph: &mut bool,
) {
    let piece = line.trim();
    if piece.is_empty() {
        if *in_paragraph {
            paragraphs.push(std::mem::take(current));
            *in_paragraph = false;
        }
        return;
    }

    if *in_paragraph {
        current.push(' ');
    } else {
        *in_paragraph = true;
    }
    current.push_str(piece);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// `transform` is deterministic and never panics for arbitrary input + pipeline.
    #[test]
    fn transform_is_deterministic_and_never_panics(
        input in interesting_string(),
        cfg in config_strategy(),
    ) {
        let a = transform(&input, &cfg);
        let b = transform(&input, &cfg);
        prop_assert_eq!(a, b);
    }

    /// Each single op, in isolation, is deterministic and panic-free — narrows the
    /// blame when the combined test fails.
    #[test]
    fn single_op_is_deterministic(
        input in interesting_string(),
        op in operation_strategy(),
    ) {
        let cfg = Config::as_given(vec![op]);
        prop_assert_eq!(transform(&input, &cfg), transform(&input, &cfg));
    }

    /// `unwrap_lines` is implemented as a streaming state machine; keep it equivalent
    /// to the previous paragraph-list model across mixed newlines and Unicode seams.
    #[test]
    fn unwrap_lines_matches_reference(input in interesting_string()) {
        let optimized = ops::lines::unwrap_lines(&input);
        prop_assert_eq!(optimized, reference_unwrap_lines(&input));
    }

    /// Idempotent ops stay idempotent on arbitrary input: applying twice equals once.
    /// (Upper/Lower case folding, collapse, trim, remove-blank, dedupe are idempotent.)
    #[test]
    fn idempotent_ops_are_idempotent(input in interesting_string()) {
        for op in [
            Operation::CollapseWhitespace,
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
            Operation::DedupeLines,
            Operation::ChangeCase { case: CaseKind::Upper },
            Operation::ChangeCase { case: CaseKind::Lower },
            // Defang is idempotent by construction (the already-defanged guard);
            // CleanUrls is too (a cleaned URL has no trackers left). Refang is
            // deliberately excluded — it is a global reverse-substitution, so
            // nested markers like `[[.]]` collapse on each pass (documented).
            Operation::Defang { style: BracketStyle::Square },
            Operation::Defang { style: BracketStyle::Round },
            Operation::CleanUrls,
        ] {
            let cfg = Config::as_given(vec![op.clone()]);
            let once = transform(&input, &cfg);
            let twice = transform(&once, &cfg);
            prop_assert_eq!(&once, &twice);
        }
    }

    /// Structural invariants of the A2 ops hold for arbitrary input.
    #[test]
    fn op_invariants_hold(input in interesting_string()) {
        // collapse: tabs are gone and no run of two-or-more ASCII spaces survives.
        let collapsed = transform(
            &input,
            &Config::as_given(vec![Operation::CollapseWhitespace]),
        );
        prop_assert!(!collapsed.contains("  "), "collapse left a double space");
        prop_assert!(!collapsed.contains('\t'), "collapse left a tab");

        // trim: no line ends with an ASCII space or tab.
        let trimmed = transform(
            &input,
            &Config::as_given(vec![Operation::TrimTrailingWhitespace]),
        );
        for line in trimmed.split('\n') {
            prop_assert!(!line.ends_with(' '));
            prop_assert!(!line.ends_with('\t'));
        }

        // remove_blank: no *content* line is whitespace-only. A single trailing '\n'
        // is the legitimate line terminator (it round-trips), so strip it before
        // checking the content lines.
        let no_blanks = transform(
            &input,
            &Config::as_given(vec![Operation::RemoveBlankLines]),
        );
        let body = no_blanks.strip_suffix('\n').unwrap_or(&no_blanks);
        if !body.is_empty() {
            for line in body.split('\n') {
                prop_assert!(
                    !line.chars().all(char::is_whitespace),
                    "remove_blank left a blank line: {line:?}"
                );
            }
        }

        // extract_emails / extract_urls: every output line is non-empty.
        for op in [Operation::ExtractEmails, Operation::ExtractUrls] {
            let extracted = transform(&input, &Config::as_given(vec![op]));
            if !extracted.is_empty() {
                for line in extracted.split('\n') {
                    prop_assert!(!line.is_empty());
                }
            }
        }
    }

    /// The internal W3 fusion for TrimTrailingWhitespace -> RemoveBlankLines must
    /// remain byte-for-byte identical to applying the two public operations in order.
    #[test]
    fn trim_remove_blank_fusion_matches_public_ops(input in interesting_string()) {
        let trimmed = ops::whitespace::trim_trailing_whitespace(&input);
        let reference = ops::lines::remove_blank_lines(&trimmed);

        let as_given = Config::as_given(vec![
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
        ]);
        let fused_as_given = transform(&input, &as_given);
        prop_assert_eq!(&fused_as_given, &reference);

        let canonical = Config {
            version: CONFIG_VERSION,
            operations: vec![
                Operation::RemoveBlankLines,
                Operation::TrimTrailingWhitespace,
            ],
            ordering: Ordering::Canonical,
        };
        let fused_canonical = transform(&input, &canonical);
        prop_assert_eq!(&fused_canonical, &reference);
    }

    /// The internal W3 fusion for CollapseWhitespace -> TrimTrailingWhitespace ->
    /// RemoveBlankLines must remain byte-for-byte identical to applying the three
    /// public operations in order.
    #[test]
    fn collapse_trim_remove_blank_fusion_matches_public_ops(input in interesting_string()) {
        let collapsed = ops::whitespace::collapse_whitespace(&input);
        let trimmed = ops::whitespace::trim_trailing_whitespace(&collapsed);
        let reference = ops::lines::remove_blank_lines(&trimmed);

        let as_given = Config::as_given(vec![
            Operation::CollapseWhitespace,
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
        ]);
        let fused_as_given = transform(&input, &as_given);
        prop_assert_eq!(&fused_as_given, &reference);

        let canonical = Config {
            version: CONFIG_VERSION,
            operations: vec![
                Operation::RemoveBlankLines,
                Operation::TrimTrailingWhitespace,
                Operation::CollapseWhitespace,
            ],
            ordering: Ordering::Canonical,
        };
        let fused_canonical = transform(&input, &canonical);
        prop_assert_eq!(&fused_canonical, &reference);
    }

    /// The internal W3 fusion for TrimTrailingWhitespace -> RemoveBlankLines ->
    /// DedupeLines must remain byte-for-byte identical to applying the three public
    /// operations in order.
    #[test]
    fn trim_remove_blank_dedupe_fusion_matches_public_ops(input in interesting_string()) {
        let trimmed = ops::whitespace::trim_trailing_whitespace(&input);
        let no_blanks = ops::lines::remove_blank_lines(&trimmed);
        let reference = ops::lines::dedupe_lines(&no_blanks);

        let as_given = Config::as_given(vec![
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
            Operation::DedupeLines,
        ]);
        let fused_as_given = transform(&input, &as_given);
        prop_assert_eq!(&fused_as_given, &reference);

        let canonical = Config {
            version: CONFIG_VERSION,
            operations: vec![
                Operation::DedupeLines,
                Operation::RemoveBlankLines,
                Operation::TrimTrailingWhitespace,
            ],
            ordering: Ordering::Canonical,
        };
        let fused_canonical = transform(&input, &canonical);
        prop_assert_eq!(&fused_canonical, &reference);
    }

    /// The internal W3 fusion for CollapseWhitespace -> TrimTrailingWhitespace ->
    /// RemoveBlankLines -> DedupeLines must remain byte-for-byte identical to applying
    /// the four public operations in order.
    #[test]
    fn collapse_trim_remove_blank_dedupe_fusion_matches_public_ops(input in interesting_string()) {
        let collapsed = ops::whitespace::collapse_whitespace(&input);
        let trimmed = ops::whitespace::trim_trailing_whitespace(&collapsed);
        let no_blanks = ops::lines::remove_blank_lines(&trimmed);
        let reference = ops::lines::dedupe_lines(&no_blanks);

        let as_given = Config::as_given(vec![
            Operation::CollapseWhitespace,
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
            Operation::DedupeLines,
        ]);
        let fused_as_given = transform(&input, &as_given);
        prop_assert_eq!(&fused_as_given, &reference);

        let canonical = Config {
            version: CONFIG_VERSION,
            operations: vec![
                Operation::DedupeLines,
                Operation::RemoveBlankLines,
                Operation::TrimTrailingWhitespace,
                Operation::CollapseWhitespace,
            ],
            ordering: Ordering::Canonical,
        };
        let fused_canonical = transform(&input, &canonical);
        prop_assert_eq!(&fused_canonical, &reference);
    }
}
