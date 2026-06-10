//! Cedar-style executable reference semantics for the transform pipeline.
//!
//! xPare is not an authorization-policy engine, so it does not use Cedar. It
//! borrows Cedar's *method*: a simple, auditable **reference interpreter** that the
//! optimized production pipeline is differentially tested against. The production
//! `transform` fuses adjacent operations and folds intermediates through `Zeroizing`
//! storage for speed and hygiene; this reference does none of that — it resolves the
//! ordering, then applies one operation at a time via the public `ops::*` free
//! functions. If production and reference ever disagree, one of them is wrong.
//!
//! What this proves (by differential property testing):
//!
//! * **production == reference** for arbitrary inputs and pipelines — so every fused
//!   fast path in `pipeline.rs` is byte-for-byte equal to naive sequential
//!   application, for any config that happens to trigger it.
//! * **canonical == explicitly sorted as_given** — `Ordering::Canonical` equals
//!   stable-sorting by `Operation::canonical_rank` and running `AsGiven`.
//! * **determinism** — the same `(input, config)` always yields the same output.
//! * **resource envelope** — an accepted config's output never amplifies past the
//!   per-op growth product (and so never past `MAX_PIPELINE_GROWTH_FACTOR`).
//!
//! What it does NOT prove: that the documented op semantics are themselves "correct"
//! beyond their frozen doc-comment rules, or anything about browser/RFC fidelity.
//! This is verification-*guided* development, not formal verification.

use proptest::prelude::*;
use xpare_core::{
    ops, transform, BracketStyle, CaseKind, Config, Operation, Ordering, CONFIG_VERSION,
    MAX_CONFIG_OPERATIONS, MAX_PIPELINE_GROWTH_FACTOR,
};

// ---------------------------------------------------------------------------
// The reference interpreter (test-only).
//
// It must NOT call production `transform`, and must NOT include any optimized /
// fused path. It resolves the ordering exactly as documented (a stable sort by
// canonical rank for `Canonical`, input order for `AsGiven`) and applies operations
// strictly one at a time. It may call the individual `ops::*` functions — that keeps
// the reference simple and auditable while still being an independent path from the
// fused production pipeline.
// ---------------------------------------------------------------------------

/// Apply a single operation, the slow obvious way. Mirrors `pipeline::apply` but is
/// reached only one op at a time, so no fusion is possible.
fn apply_one(text: &str, op: &Operation) -> String {
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

/// Resolve the documented execution order, then fold the ops one at a time.
fn reference_transform(input: &str, config: &Config) -> String {
    let mut ordered: Vec<&Operation> = config.operations.iter().collect();
    if config.ordering == Ordering::Canonical {
        // Stable sort by canonical rank — the documented canonical order. A stable
        // sort keeps the provided order for any rank ties, matching `pipeline.rs`.
        ordered.sort_by_key(|op| op.canonical_rank());
    }
    let mut current = input.to_string();
    for op in ordered {
        current = apply_one(&current, op);
    }
    current
}

// ---------------------------------------------------------------------------
// Generators. Inputs are biased toward the bytes that exercise the strippers,
// IOC/URL/masking heuristics, line model, and case mapping; params are kept short
// and bounded so a generated pipeline runs fast under normal `cargo test`.
// ---------------------------------------------------------------------------

fn interesting_char() -> impl Strategy<Value = char> {
    prop_oneof![
        20 => prop_oneof![
            Just('\n'), Just('\r'), Just(' '), Just('\t'),
            Just('.'), Just('!'), Just('?'),
            Just('@'), Just('/'), Just(':'),
            Just('<'), Just('>'), Just('"'), Just('\''),
            Just(','), Just(';'), Just('('), Just(')'),
        ],
        // HTML/markdown/IOC fragments so strip_* / clean_urls / defang see real work.
        6 => prop_oneof![
            Just('h'), Just('t'), Just('p'), Just('s'),
            Just('*'), Just('_'), Just('`'), Just('#'),
            Just('['), Just(']'), Just('='), Just('&'),
        ],
        8 => prop::char::range('a', 'z'),
        4 => prop::char::range('A', 'Z'),
        2 => prop::char::range('0', '9'),
        3 => prop_oneof![
            Just('ß'), Just('İ'), Just('Σ'), Just('ﬁ'), Just('é'),
            Just('\u{00a0}'), Just('\u{0307}'), Just('🦀'),
        ],
        4 => any::<char>(),
    ]
}

fn interesting_string() -> impl Strategy<Value = String> {
    prop::collection::vec(interesting_char(), 0..80).prop_map(|chars| chars.into_iter().collect())
}

/// Short free-text op parameter (kept well inside the byte/line-break envelope).
fn param_string() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just(' '),
            Just('>'),
            Just(','),
            Just('|'),
            Just('-'),
            // No CR/LF: the envelope rejects them, and a bare param would diverge from
            // a real (parse_config-accepted) pipeline.
            any::<char>().prop_filter("no CR/LF", |c| *c != '\n' && *c != '\r'),
        ],
        0..6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn operation_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        Just(Operation::StripHtml),
        Just(Operation::StripMarkdown),
        Just(Operation::HtmlToMarkdown),
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
        (any::<bool>(), any::<bool>(), any::<bool>())
            .prop_map(|(emails, ipv4, ipv6)| Operation::MaskIdentifiers { emails, ipv4, ipv6 }),
    ]
}

fn config_strategy() -> impl Strategy<Value = Config> {
    (
        prop::collection::vec(operation_strategy(), 0..8),
        prop_oneof![Just(Ordering::Canonical), Just(Ordering::AsGiven)],
    )
        .prop_map(|(operations, ordering)| Config {
            version: CONFIG_VERSION,
            operations,
            ordering,
        })
}

// ---------------------------------------------------------------------------
// Independent oracle for the documented per-op growth bound.
//
// This re-states `Operation::max_growth_factor` (private to the core) as an
// independent reimplementation — the same Cedar-style "second source of truth"
// pattern the fusion references use. `Config::validate` rejects any pipeline whose
// product exceeds `MAX_PIPELINE_GROWTH_FACTOR`, so for an accepted config this
// product is a sound upper bound on `out.len() / in.len()`.
// ---------------------------------------------------------------------------

fn reference_growth_factor(op: &Operation) -> u64 {
    match op {
        Operation::PrefixLines { prefix } => 1 + prefix.len() as u64,
        Operation::SuffixLines { suffix } => 1 + suffix.len() as u64,
        Operation::JoinWith { separator } => (separator.len() as u64).max(1),
        // Ordered-list numbering at the indent clamp reaches (10 + digits(items))/4
        // per item — ~4.7x at the 2 GiB FFI input ceiling — so HtmlToMarkdown
        // declares 5, not 3 (see `Operation::max_growth_factor` for the derivation).
        Operation::HtmlToMarkdown => 5,
        Operation::ChangeCase { .. } | Operation::Defang { .. } => 3,
        Operation::MaskIdentifiers { .. } => 2,
        _ => 1,
    }
}

fn reference_growth_product(config: &Config) -> u64 {
    config.operations.iter().fold(1u64, |acc, op| {
        acc.saturating_mul(reference_growth_factor(op))
    })
}

// ---------------------------------------------------------------------------
// Differential properties.
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// The headline property: the optimized production pipeline equals the naive
    /// one-op-at-a-time reference for arbitrary inputs and pipelines, under either
    /// ordering. This is what makes every fusion in `pipeline.rs` provably equivalent
    /// to sequential application for any config that triggers it.
    #[test]
    fn production_equals_reference(input in interesting_string(), config in config_strategy()) {
        prop_assert_eq!(transform(&input, &config), reference_transform(&input, &config));
    }

    /// `Ordering::Canonical` equals stable-sorting the ops by `canonical_rank` and
    /// running them `AsGiven` — proven through the independent reference so it does
    /// not lean on the production reordering it is meant to check.
    #[test]
    fn canonical_equals_presorted_as_given(
        input in interesting_string(),
        ops in prop::collection::vec(operation_strategy(), 0..8),
    ) {
        let mut presorted = ops.clone();
        presorted.sort_by_key(|o| o.canonical_rank());

        let canonical = Config { version: CONFIG_VERSION, operations: ops, ordering: Ordering::Canonical };
        let as_given_sorted = Config::as_given(presorted);

        // Reference canonical == reference presorted-as_given.
        prop_assert_eq!(
            reference_transform(&input, &canonical),
            reference_transform(&input, &as_given_sorted)
        );
        // And production agrees with that explicitly-sorted run.
        prop_assert_eq!(
            transform(&input, &canonical),
            transform(&input, &as_given_sorted)
        );
    }

    /// Determinism: `transform(x, c) == transform(x, c)`.
    #[test]
    fn production_is_deterministic(input in interesting_string(), config in config_strategy()) {
        prop_assert_eq!(transform(&input, &config), transform(&input, &config));
    }

    /// Resource envelope: for any config that `Config::validate` accepts, the output
    /// never amplifies past the per-op growth product (and so never past
    /// `MAX_PIPELINE_GROWTH_FACTOR`). Configs whose product would exceed the cap are
    /// rejected by the validator, so we only assert the bound on accepted ones.
    #[test]
    fn accepted_config_stays_within_growth_envelope(
        input in interesting_string(),
        config in config_strategy(),
    ) {
        prop_assume!(config.validate().is_ok());
        let out = transform(&input, &config);

        let product = reference_growth_product(&config);
        let bound = (input.len() as u64).saturating_mul(product);
        prop_assert!(
            out.len() as u64 <= bound,
            "output {} B exceeds per-op growth bound {} B (product {}) for input {} B",
            out.len(), bound, product, input.len(),
        );
        // The validator's contract: an accepted config's product is within the cap.
        prop_assert!(product <= MAX_PIPELINE_GROWTH_FACTOR);
    }
}

// ---------------------------------------------------------------------------
// Explicit fused-path coverage.
//
// The property above only hits a fusion when the generator happens to produce the
// triggering op sequence. These deterministic cases GUARANTEE every fusion currently
// in `pipeline.rs` is exercised against the reference, on inputs crafted to actually
// reach the fused branch (multi-line, trailing whitespace, blank + duplicate lines,
// embedded HTML+Markdown). If a future edit changes a fusion, one of these — or the
// property — fails.
// ---------------------------------------------------------------------------

/// Run an `as_given` pipeline both ways and assert production == reference.
fn assert_fusion(label: &str, input: &str, ops: Vec<Operation>) {
    let config = Config::as_given(ops);
    assert_eq!(
        transform(input, &config),
        reference_transform(input, &config),
        "fusion `{label}` diverged from the reference for input {input:?}"
    );
}

/// An input that exercises every fused line/whitespace branch at once: leading and
/// internal multi-space runs and tabs (collapse), trailing whitespace (trim), blank
/// lines (remove-blank), and duplicate content lines (dedupe).
const MESSY_LINES: &str = "  a\tb   \n\n  a\tb   \n   \nc \nc \n\n";

#[test]
fn fusion_strip_html_then_strip_markdown_matches_reference() {
    let input = "<p>**bold** <script>alert(1)</script> _x_</p>\n<div>a &amp; b</div>";
    assert_fusion(
        "strip_html+strip_markdown",
        input,
        vec![Operation::StripHtml, Operation::StripMarkdown],
    );
}

#[test]
fn fusion_trim_then_remove_blank_matches_reference() {
    assert_fusion(
        "trim+remove_blank",
        MESSY_LINES,
        vec![
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
        ],
    );
}

#[test]
fn fusion_trim_remove_blank_dedupe_matches_reference() {
    assert_fusion(
        "trim+remove_blank+dedupe",
        MESSY_LINES,
        vec![
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
            Operation::DedupeLines,
        ],
    );
}

#[test]
fn fusion_collapse_trim_remove_blank_matches_reference() {
    assert_fusion(
        "collapse+trim+remove_blank",
        MESSY_LINES,
        vec![
            Operation::CollapseWhitespace,
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
        ],
    );
}

#[test]
fn fusion_collapse_trim_remove_blank_dedupe_matches_reference() {
    assert_fusion(
        "collapse+trim+remove_blank+dedupe",
        MESSY_LINES,
        vec![
            Operation::CollapseWhitespace,
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
            Operation::DedupeLines,
        ],
    );
}

/// The collapse+trim+remove_blank+dedupe fusion has a borrowed fast path that is only
/// exact when collapse would be identity for the whole input. Cover that branch too:
/// an input with NO collapsible runs still must match the reference.
#[test]
fn fusion_collapse_chain_no_collapse_needed_matches_reference() {
    let input = "a\n\na\nb \n";
    assert_fusion(
        "collapse+trim+remove_blank+dedupe (no-collapse fast path)",
        input,
        vec![
            Operation::CollapseWhitespace,
            Operation::TrimTrailingWhitespace,
            Operation::RemoveBlankLines,
            Operation::DedupeLines,
        ],
    );
}

// ---------------------------------------------------------------------------
// Sanity checks on the reference itself.
// ---------------------------------------------------------------------------

#[test]
fn reference_identity_pipeline_is_input() {
    let cfg = Config::empty();
    for input in ["", "hello", "a\nb\n", "<p>x</p>"] {
        assert_eq!(reference_transform(input, &cfg), input);
        assert_eq!(transform(input, &cfg), reference_transform(input, &cfg));
    }
}

#[test]
fn reference_growth_product_never_underestimates_a_full_pipeline() {
    // A single representative op of each growth class; the product is the saturating
    // product of the documented factors, matching the validator's bound.
    let cfg = Config::as_given(vec![
        Operation::PrefixLines {
            prefix: "> ".into(),
        }, // 3
        Operation::JoinWith {
            separator: ", ".into(),
        }, // 2
        Operation::ChangeCase {
            case: CaseKind::Upper,
        }, // 3
        Operation::StripHtml, // 1
    ]);
    assert_eq!(reference_growth_product(&cfg), 3 * 2 * 3);
    assert!(cfg.validate().is_ok());
    // And a degenerate all-max pipeline saturates rather than wrapping.
    let big = "a".repeat(255);
    let huge = Config::as_given(vec![
        Operation::PrefixLines { prefix: big };
        MAX_CONFIG_OPERATIONS
    ]);
    assert_eq!(reference_growth_product(&huge), u64::MAX);
}
