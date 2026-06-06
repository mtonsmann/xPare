//! Config serde round-trip + version tests.
//!
//! The config is the only data that crosses the FFI boundary, so its JSON encoding
//! must round-trip exactly and its version gate must be enforced. We test both with
//! representative fixtures and a proptest over arbitrary `Config` values.

use proptest::prelude::*;
use safetystrip_core::{
    parse_config, BracketStyle, CaseKind, Config, ConfigError, Operation, Ordering, CONFIG_VERSION,
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

/// Strategy for an arbitrary `Operation`, including arbitrary string parameters
/// (so the JSON escaping of separators/prefixes is exercised too).
fn operation_strategy() -> impl Strategy<Value = Operation> {
    // Arbitrary UTF-8 strings, including control chars and quotes, to stress escaping.
    let s = ".*";
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
        s.prop_map(|prefix| Operation::PrefixLines { prefix }),
        s.prop_map(|suffix| Operation::SuffixLines { suffix }),
        s.prop_map(|separator| Operation::JoinWith { separator }),
        s.prop_map(|delimiter| Operation::SplitOn { delimiter }),
        Just(Operation::ExtractEmails),
        Just(Operation::ExtractUrls),
    ]
}

/// Strategy for an arbitrary valid `Config` (always the supported version, either
/// ordering mode so both serialize/round-trip).
fn config_strategy() -> impl Strategy<Value = Config> {
    (
        prop::collection::vec(operation_strategy(), 0..12),
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
    #[test]
    fn arbitrary_config_round_trips(cfg in config_strategy()) {
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed = parse_config(&json).expect("parse round-tripped config");
        prop_assert_eq!(parsed, cfg);
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
