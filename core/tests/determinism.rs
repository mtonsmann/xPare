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
use safetystrip_core::{transform, CaseKind, Config, Operation, CONFIG_VERSION};

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
    ]
}

/// A config with an arbitrary ordered pipeline (0..8 ops).
fn config_strategy() -> impl Strategy<Value = Config> {
    prop::collection::vec(operation_strategy(), 0..8).prop_map(|operations| Config {
        version: CONFIG_VERSION,
        operations,
    })
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
        let cfg = Config { version: CONFIG_VERSION, operations: vec![op] };
        prop_assert_eq!(transform(&input, &cfg), transform(&input, &cfg));
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
        ] {
            let cfg = Config { version: CONFIG_VERSION, operations: vec![op.clone()] };
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
            &Config { version: CONFIG_VERSION, operations: vec![Operation::CollapseWhitespace] },
        );
        prop_assert!(!collapsed.contains("  "), "collapse left a double space");
        prop_assert!(!collapsed.contains('\t'), "collapse left a tab");

        // trim: no line ends with an ASCII space or tab.
        let trimmed = transform(
            &input,
            &Config { version: CONFIG_VERSION, operations: vec![Operation::TrimTrailingWhitespace] },
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
            &Config { version: CONFIG_VERSION, operations: vec![Operation::RemoveBlankLines] },
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
            let extracted = transform(
                &input,
                &Config { version: CONFIG_VERSION, operations: vec![op] },
            );
            if !extracted.is_empty() {
                for line in extracted.split('\n') {
                    prop_assert!(!line.is_empty());
                }
            }
        }
    }
}
