//! Config serde round-trip + version tests.
//!
//! The config is the only data that crosses the FFI boundary, so its JSON encoding
//! must round-trip exactly and its version gate must be enforced. We test both with
//! representative fixtures and a proptest over arbitrary `Config` values.

use proptest::prelude::*;
use xpare_core::{
    parse_config, BracketStyle, CaseKind, Config, ConfigError, Operation, Ordering, CONFIG_VERSION,
    MAX_CONFIG_OPERATIONS, MAX_CONFIG_TEXT_PARAM_BYTES, MAX_PIPELINE_GROWTH_FACTOR,
};

/// Every `Operation` variant, with representative parameter values, so the fixture
/// set covers the whole schema surface.
fn all_operations() -> Vec<Operation> {
    vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::HtmlToMarkdown,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::UnwrapLines,
        Operation::ChangeCase {
            case: CaseKind::Upper,
        },
        Operation::ChangeCase {
            case: CaseKind::Lower,
        },
        Operation::ChangeCase {
            case: CaseKind::Title,
        },
        Operation::ChangeCase {
            case: CaseKind::Sentence,
        },
        Operation::SortLines {
            descending: true,
            case_insensitive: false,
        },
        Operation::SortLines {
            descending: false,
            case_insensitive: true,
        },
        Operation::DedupeLines,
        Operation::PrefixLines {
            prefix: "> ".into(),
        },
        Operation::SuffixLines {
            suffix: " ;".into(),
        },
        Operation::JoinWith {
            separator: ", ".into(),
        },
        Operation::SplitOn {
            delimiter: "|".into(),
        },
        Operation::ExtractEmails,
        Operation::ExtractUrls,
        Operation::Defang {
            style: BracketStyle::Square,
        },
        Operation::Defang {
            style: BracketStyle::Round,
        },
        Operation::Refang,
        Operation::CleanUrls,
        Operation::MaskIdentifiers {
            emails: true,
            ipv4: true,
            ipv6: false,
        },
    ]
}

#[test]
fn empty_config_round_trips() {
    let cfg = Config::empty();
    let json = serde_json::to_string(&cfg).expect("serialize");
    let parsed = parse_config(&json).expect("parse");
    assert_eq!(parsed, cfg);
}

#[test]
fn full_config_round_trips() {
    // Non-default ordering so the round-trip also exercises the `ordering` field.
    let cfg = Config {
        version: CONFIG_VERSION,
        operations: all_operations(),
        ordering: Ordering::AsGiven,
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    let parsed = parse_config(&json).expect("parse");
    assert_eq!(parsed, cfg);
}

#[test]
fn each_operation_round_trips_individually() {
    for op in all_operations() {
        let cfg = Config::as_given(vec![op.clone()]);
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed = parse_config(&json).unwrap_or_else(|e| panic!("parse {op:?}: {e}"));
        assert_eq!(parsed, cfg, "round-trip mismatch for {op:?}");
    }
}

#[test]
fn known_operation_encoding_is_internally_tagged() {
    // Pin the wire format for one representative op so an accidental serde attribute
    // change (which would break shells) is caught here too.
    let cfg = Config {
        version: CONFIG_VERSION,
        operations: vec![Operation::ChangeCase {
            case: CaseKind::Title,
        }],
        ordering: Ordering::Canonical,
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert_eq!(
        json,
        r#"{"version":2,"operations":[{"op":"change_case","case":"title"}],"ordering":"canonical"}"#
    );
}

#[test]
fn unsupported_version_is_rejected() {
    let json = r#"{"version":999,"operations":[]}"#;
    match parse_config(json) {
        Err(ConfigError::UnsupportedVersion { found, supported }) => {
            assert_eq!(found, 999);
            assert_eq!(supported, CONFIG_VERSION);
        }
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn version_zero_is_rejected() {
    // Guard against a future where CONFIG_VERSION changes: any non-matching version
    // (here 0, assuming CONFIG_VERSION != 0) must be rejected.
    assert_ne!(CONFIG_VERSION, 0, "test assumes CONFIG_VERSION != 0");
    let json = r#"{"version":0,"operations":[]}"#;
    assert!(matches!(
        parse_config(json),
        Err(ConfigError::UnsupportedVersion { found: 0, .. })
    ));
}

#[test]
fn malformed_json_is_json_error() {
    for bad in [
        "",
        "{",
        "not json",
        r#"{"version":}"#,
        r#"{"version":"one","operations":[]}"#, // wrong type
    ] {
        assert!(
            matches!(parse_config(bad), Err(ConfigError::Json(_))),
            "expected Json error for input {bad:?}"
        );
    }
}

#[test]
fn unknown_field_is_rejected() {
    // `#[serde(deny_unknown_fields)]` on Config means a stray field is a Json error,
    // not a silently-ignored one — important for catching shell/core drift.
    let json = r#"{"version":2,"operations":[],"extra":true}"#;
    assert!(matches!(parse_config(json), Err(ConfigError::Json(_))));
}

#[test]
fn unknown_operation_tag_is_rejected() {
    let json = r#"{"version":2,"operations":[{"op":"does_not_exist"}]}"#;
    assert!(matches!(parse_config(json), Err(ConfigError::Json(_))));
}

#[test]
fn missing_required_param_is_rejected() {
    // prefix_lines requires `prefix`; omitting it is a schema violation.
    let json = r#"{"version":2,"operations":[{"op":"prefix_lines"}]}"#;
    assert!(matches!(parse_config(json), Err(ConfigError::Json(_))));
}

#[test]
fn ordering_defaults_to_canonical_when_absent() {
    // `ordering` is `#[serde(default)]`, so a v2 config omitting it is canonical.
    let cfg = parse_config(r#"{"version":2,"operations":[]}"#).expect("parse");
    assert_eq!(cfg.ordering, Ordering::Canonical);
}

#[test]
fn explicit_as_given_parses() {
    let cfg =
        parse_config(r#"{"version":2,"operations":[],"ordering":"as_given"}"#).expect("parse");
    assert_eq!(cfg.ordering, Ordering::AsGiven);
}

#[test]
fn accepts_max_operations() {
    let cfg = Config::as_given(vec![Operation::CollapseWhitespace; MAX_CONFIG_OPERATIONS]);
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert_eq!(parse_config(&json).expect("parse"), cfg);
}

#[test]
fn rejects_too_many_operations() {
    let cfg = Config::as_given(vec![
        Operation::CollapseWhitespace;
        MAX_CONFIG_OPERATIONS + 1
    ]);
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(matches!(
        parse_config(&json),
        Err(ConfigError::TooManyOperations {
            found,
            max: MAX_CONFIG_OPERATIONS,
        }) if found == MAX_CONFIG_OPERATIONS + 1
    ));
}

#[test]
fn accepts_boundary_text_params() {
    for op in text_param_ops("a".repeat(MAX_CONFIG_TEXT_PARAM_BYTES)) {
        let cfg = Config::as_given(vec![op.clone()]);
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert_eq!(parse_config(&json).expect("parse"), cfg, "{op:?}");
    }
}

#[test]
fn rejects_oversized_text_params() {
    for op in text_param_ops("a".repeat(MAX_CONFIG_TEXT_PARAM_BYTES + 1)) {
        let cfg = Config::as_given(vec![op.clone()]);
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(
            matches!(
                parse_config(&json),
                Err(ConfigError::TextParamTooLong {
                    found,
                    max: MAX_CONFIG_TEXT_PARAM_BYTES,
                    ..
                }) if found == MAX_CONFIG_TEXT_PARAM_BYTES + 1
            ),
            "{op:?}"
        );
    }
}

#[test]
fn rejects_line_breaks_in_text_params() {
    for line_break in ["\n", "\r"] {
        for op in text_param_ops(format!("before{line_break}after")) {
            let cfg = Config::as_given(vec![op.clone()]);
            let json = serde_json::to_string(&cfg).expect("serialize");
            assert!(
                matches!(
                    parse_config(&json),
                    Err(ConfigError::TextParamContainsLineBreak { .. })
                ),
                "{op:?}"
            );
        }
    }
}

#[test]
fn rejects_fuzz_oom_line_affix_pattern_before_transform() {
    let mut ops = vec![Operation::PrefixLines {
        prefix: "~~~\n+c-\0\n\0\0\0".into(),
    }];
    ops.extend(
        std::iter::repeat(Operation::PrefixLines {
            prefix: "> ".into(),
        })
        .take(8),
    );
    ops.push(Operation::StripMarkdown);

    let cfg = Config::as_given(ops);
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(matches!(
        parse_config(&json),
        Err(ConfigError::TextParamContainsLineBreak {
            op: "prefix_lines",
            param: "prefix",
        })
    ));
}

/// A realistic clean-up pipeline (coerce rich text, quote each line, join to CSV)
/// uses short parameters, so its worst-case growth product is tiny and it must be
/// accepted. Guards the amplification bound against false-rejecting real configs.
#[test]
fn accepts_realistic_pipeline_growth() {
    // Products: StripHtml/StripMarkdown/CollapseWhitespace = 1; PrefixLines "> " =
    // 1+2 = 3; JoinWith ", " = 2. Total = 6, far below MAX_PIPELINE_GROWTH_FACTOR.
    let cfg = Config::as_given(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::CollapseWhitespace,
        Operation::PrefixLines {
            prefix: "> ".into(),
        },
        Operation::JoinWith {
            separator: ", ".into(),
        },
    ]);
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert_eq!(parse_config(&json).expect("parse"), cfg);
}

/// A single boundary-size affix is a linear-time pass (factor 257), so it stays
/// under the cap and is accepted — the bound targets *composition*, not one big op.
#[test]
fn accepts_single_boundary_affix() {
    let cfg = Config::as_given(vec![Operation::PrefixLines {
        prefix: "a".repeat(MAX_CONFIG_TEXT_PARAM_BYTES),
    }]);
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert_eq!(parse_config(&json).expect("parse"), cfg);
}

/// The core finding from the overnight fuzz run: a config of individually
/// envelope-legal operations whose *composition* amplifies a tiny input without
/// bound (a `SplitOn` re-maximizes the line count so each following affix/join
/// re-amplifies). None of these parameters contain a line break, so this is rejected
/// specifically by the growth bound — not the line-break or param-length rules —
/// before the infallible `transform` is ever entered.
#[test]
fn rejects_amplifying_pipeline_growth() {
    let big = "x".repeat(MAX_CONFIG_TEXT_PARAM_BYTES); // no '\n'/'\r'
    let cfg = Config::as_given(vec![
        Operation::SplitOn {
            delimiter: "x".into(),
        },
        Operation::PrefixLines {
            prefix: big.clone(),
        },
        Operation::JoinWith {
            separator: big.clone(),
        },
        Operation::SplitOn {
            delimiter: "y".into(),
        },
        Operation::PrefixLines { prefix: big },
    ]);
    let json = serde_json::to_string(&cfg).expect("serialize");
    match parse_config(&json) {
        Err(ConfigError::PipelineMayAmplify { factor, max }) => {
            assert_eq!(max, MAX_PIPELINE_GROWTH_FACTOR);
            assert!(
                factor > MAX_PIPELINE_GROWTH_FACTOR,
                "reported factor {factor} should exceed the cap {max}"
            );
        }
        other => panic!("expected PipelineMayAmplify, got {other:?}"),
    }
}

/// Three boundary-size affixes already blow past the cap (257^3 ~= 1.7e7); a
/// param-length-only or per-op-only check would wave this through.
#[test]
fn rejects_three_boundary_affixes() {
    let big = "a".repeat(MAX_CONFIG_TEXT_PARAM_BYTES);
    let cfg = Config::as_given(vec![
        Operation::PrefixLines {
            prefix: big.clone(),
        },
        Operation::SuffixLines {
            suffix: big.clone(),
        },
        Operation::PrefixLines { prefix: big },
    ]);
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(matches!(
        parse_config(&json),
        Err(ConfigError::PipelineMayAmplify { .. })
    ));
}

/// The growth product is computed with saturating multiplication, so a pipeline whose
/// true product overflows `u64` (many large affixes) still compares as "too large"
/// and is rejected rather than wrapping to a small value and slipping through.
#[test]
fn growth_product_saturates_and_rejects() {
    let big = "a".repeat(MAX_CONFIG_TEXT_PARAM_BYTES);
    // 257^32 vastly exceeds u64::MAX, so the saturating product hits the ceiling.
    let cfg = Config::as_given(vec![
        Operation::PrefixLines { prefix: big };
        MAX_CONFIG_OPERATIONS
    ]);
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(matches!(
        parse_config(&json),
        Err(ConfigError::PipelineMayAmplify { factor, .. }) if factor == u64::MAX
    ));
}

fn text_param_ops(value: String) -> Vec<Operation> {
    vec![
        Operation::PrefixLines {
            prefix: value.clone(),
        },
        Operation::SuffixLines {
            suffix: value.clone(),
        },
        Operation::JoinWith {
            separator: value.clone(),
        },
        Operation::SplitOn { delimiter: value },
    ]
}

// ---------------------------------------------------------------------------
// Proptest: arbitrary Config values round-trip.
// ---------------------------------------------------------------------------

/// Strategy for an arbitrary `CaseKind`.
fn case_kind_strategy() -> impl Strategy<Value = CaseKind> {
    prop_oneof![
        Just(CaseKind::Upper),
        Just(CaseKind::Lower),
        Just(CaseKind::Title),
        Just(CaseKind::Sentence),
    ]
}

/// Strategy for an arbitrary valid free-text operation parameter. Keep generated
/// params well inside the configured byte limit while still exercising JSON escaping.
fn valid_param_string() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just(' '),
            Just('\t'),
            Just('"'),
            Just('\\'),
            Just('\0'),
            any::<char>().prop_filter("no CR/LF", |c| *c != '\n' && *c != '\r'),
        ],
        0..32,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

/// Strategy for an arbitrary `Operation`, including arbitrary string parameters
/// (so the JSON escaping of separators/prefixes is exercised too).
fn operation_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        Just(Operation::StripHtml),
        Just(Operation::StripMarkdown),
        Just(Operation::HtmlToMarkdown),
        Just(Operation::CollapseWhitespace),
        Just(Operation::TrimTrailingWhitespace),
        Just(Operation::RemoveBlankLines),
        Just(Operation::UnwrapLines),
        case_kind_strategy().prop_map(|case| Operation::ChangeCase { case }),
        (any::<bool>(), any::<bool>()).prop_map(|(descending, case_insensitive)| {
            Operation::SortLines {
                descending,
                case_insensitive,
            }
        }),
        Just(Operation::DedupeLines),
        valid_param_string().prop_map(|prefix| Operation::PrefixLines { prefix }),
        valid_param_string().prop_map(|suffix| Operation::SuffixLines { suffix }),
        valid_param_string().prop_map(|separator| Operation::JoinWith { separator }),
        valid_param_string().prop_map(|delimiter| Operation::SplitOn { delimiter }),
        Just(Operation::ExtractEmails),
        Just(Operation::ExtractUrls),
        prop_oneof![Just(BracketStyle::Square), Just(BracketStyle::Round)]
            .prop_map(|style| Operation::Defang { style }),
        Just(Operation::Refang),
        Just(Operation::CleanUrls),
        (any::<bool>(), any::<bool>(), any::<bool>())
            .prop_map(|(emails, ipv4, ipv6)| { Operation::MaskIdentifiers { emails, ipv4, ipv6 } }),
    ]
}

/// Strategy for an arbitrary valid `Config` (always the supported version, either
/// ordering mode so both serialize/round-trip).
fn config_strategy() -> impl Strategy<Value = Config> {
    (
        prop::collection::vec(operation_strategy(), 0..=MAX_CONFIG_OPERATIONS),
        prop_oneof![Just(Ordering::Canonical), Just(Ordering::AsGiven)],
    )
        .prop_map(|(operations, ordering)| Config {
            version: CONFIG_VERSION,
            operations,
            ordering,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any valid Config survives a `to_string` -> `parse_config` round trip unchanged.
    /// The generator stays inside the op-count, param-length, and line-break envelopes,
    /// so the only rule it can trip is the pipeline growth bound — and that is a
    /// legitimate rejection, not an encoding failure, so the JSON must still decode
    /// back to `cfg` via serde in that case.
    #[test]
    fn arbitrary_config_round_trips(cfg in config_strategy()) {
        let json = serde_json::to_string(&cfg).expect("serialize");
        match parse_config(&json) {
            Ok(parsed) => prop_assert_eq!(parsed, cfg),
            Err(ConfigError::PipelineMayAmplify { .. }) => {
                let decoded: Config = serde_json::from_str(&json).expect("decode");
                prop_assert_eq!(decoded, cfg);
            }
            Err(e) => prop_assert!(false, "unexpected parse error: {:?}", e),
        }
    }

    /// Any version other than CONFIG_VERSION is rejected as UnsupportedVersion, for an
    /// otherwise-valid config body.
    #[test]
    fn arbitrary_wrong_version_rejected(
        version in any::<u32>().prop_filter("not the supported version", |v| *v != CONFIG_VERSION),
        operations in prop::collection::vec(operation_strategy(), 0..6),
    ) {
        let cfg = Config {
            version,
            operations,
            ordering: Ordering::AsGiven,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let is_version_error =
            matches!(parse_config(&json), Err(ConfigError::UnsupportedVersion { .. }));
        prop_assert!(is_version_error);
    }
}
