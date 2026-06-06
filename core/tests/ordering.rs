//! Canonical pipeline ordering (exec-plan 0005 / DESIGN.md D13).
//!
//! `Ordering::Canonical` (the default) stable-sorts operations by their documented
//! rank before running, so the result is correct and efficient regardless of the
//! order a caller assembled them; `Ordering::AsGiven` runs them exactly as listed.

use proptest::prelude::*;
use safetystrip_core::{transform, BracketStyle, Config, Operation};

const SQ: BracketStyle = BracketStyle::Square;

fn defang() -> Operation {
    Operation::Defang { style: SQ }
}

fn mask_all() -> Operation {
    Operation::MaskIdentifiers {
        emails: true,
        ipv4: true,
        ipv6: true,
    }
}

// ---------------------------------------------------------------------------
// Correctness orderings
// ---------------------------------------------------------------------------

/// The motivating case: CleanUrls must run before Defang, and canonical ordering
/// guarantees it no matter which order the caller enabled them in.
#[test]
fn canonical_runs_clean_urls_before_defang() {
    let input = "https://e.com/?utm_source=x";
    let cleaned_then_defanged = "hxxps[://]e[.]com/";

    // Either input order yields the same correct result under canonical ordering.
    assert_eq!(
        transform(
            input,
            &Config::canonical(vec![defang(), Operation::CleanUrls])
        ),
        cleaned_then_defanged
    );
    assert_eq!(
        transform(
            input,
            &Config::canonical(vec![Operation::CleanUrls, defang()])
        ),
        cleaned_then_defanged
    );

    // as_given in the wrong order leaves the tracker (defang mangles the URL so
    // CleanUrls no longer recognizes it) — this is exactly what canonical prevents.
    assert_eq!(
        transform(
            input,
            &Config::as_given(vec![defang(), Operation::CleanUrls])
        ),
        "hxxps[://]e[.]com/?utm_source=x"
    );
}

/// Privacy masking runs before defang and reductions in canonical order. That keeps
/// masking privacy-preserving by default even if callers list it late.
#[test]
fn canonical_runs_masking_before_defang_and_extractors() {
    let input = "user@example.test 10.0.0.1";
    assert_eq!(
        transform(input, &Config::canonical(vec![defang(), mask_all()])),
        "[email] [ipv4]"
    );
    assert_eq!(
        transform(
            input,
            &Config::canonical(vec![Operation::ExtractEmails, mask_all()])
        ),
        ""
    );
}

/// Canonical output does not depend on the order distinct ops were listed in.
#[test]
fn canonical_is_input_order_independent() {
    let input = "  Hello   WORLD  \n\n  Hello   WORLD  \n";
    let ops = vec![
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::DedupeLines,
    ];
    let mut reversed = ops.clone();
    reversed.reverse();
    assert_eq!(
        transform(input, &Config::canonical(ops)),
        transform(input, &Config::canonical(reversed))
    );
}

// ---------------------------------------------------------------------------
// Efficiency ordering (output-equivalent, dedupe before sort)
// ---------------------------------------------------------------------------

/// Dedupe+sort produce the same lines in any order (dedupe is global first-occurrence);
/// canonical picks the cheaper dedupe-first order, and the output matches either
/// hand-order.
#[test]
fn dedupe_and_sort_are_output_equivalent() {
    let input = "b\na\nb\na";
    let expected = "a\nb";
    assert_eq!(
        transform(
            input,
            &Config::as_given(vec![
                Operation::DedupeLines,
                Operation::SortLines {
                    descending: false,
                    case_insensitive: false
                }
            ])
        ),
        expected
    );
    assert_eq!(
        transform(
            input,
            &Config::as_given(vec![
                Operation::SortLines {
                    descending: false,
                    case_insensitive: false
                },
                Operation::DedupeLines
            ])
        ),
        expected
    );
    assert_eq!(
        transform(
            input,
            &Config::canonical(vec![
                Operation::SortLines {
                    descending: false,
                    case_insensitive: false
                },
                Operation::DedupeLines
            ])
        ),
        expected
    );
}

// ---------------------------------------------------------------------------
// Property: transform's canonical path == manually pre-sorting, then as_given
// ---------------------------------------------------------------------------

fn op_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        Just(Operation::StripHtml),
        Just(Operation::CollapseWhitespace),
        Just(Operation::TrimTrailingWhitespace),
        Just(Operation::RemoveBlankLines),
        Just(Operation::DedupeLines),
        Just(Operation::CleanUrls),
        Just(mask_all()),
        Just(defang()),
        Just(Operation::SortLines {
            descending: false,
            case_insensitive: false
        }),
        ".*".prop_map(|prefix| Operation::PrefixLines { prefix }),
    ]
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just('a'),
            Just('b'),
            Just(' '),
            Just('\n'),
            Just('.'),
            any::<char>()
        ],
        0..40,
    )
    .prop_map(|c| c.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Running in `Canonical` mode equals stable-sorting the ops by rank ourselves
    /// and running them `AsGiven` — i.e. `transform` applies exactly the documented
    /// canonical order.
    #[test]
    fn canonical_equals_manually_presorted_as_given(input in text(), ops in prop::collection::vec(op_strategy(), 0..8)) {
        let mut presorted = ops.clone();
        presorted.sort_by_key(|o| o.canonical_rank());
        prop_assert_eq!(
            transform(&input, &Config::canonical(ops)),
            transform(&input, &Config::as_given(presorted))
        );
    }
}
