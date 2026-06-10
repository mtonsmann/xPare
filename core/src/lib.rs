//! xPare transformation core.
//!
//! Pure, deterministic text transforms: `String` in, `String` out, selected by a
//! [`Config`]. This crate is the untrusted-input path — it is fed arbitrary,
//! possibly adversarial text — so two invariants dominate and are enforced
//! mechanically (see `docs/guardrails/`):
//!
//! * **Memory safety:** `#![forbid(unsafe_code)]` — there is no `unsafe` here, so
//!   memory-unsafety is impossible by construction. The fuzz/property suites then
//!   prove the remaining risk for hand-rolled parsers — panics and hangs — absent.
//! * **No side effects:** no OS, I/O, network, logging, or global mutable state.
//!   The `print`/`dbg` lints below make "a clipboard string can never reach a log
//!   sink" a compile error rather than a promise.
//!
//! Determinism: the same `(input, config)` always yields the same output.
#![forbid(unsafe_code)]
// Mechanical "no log sink in the core": these are compile errors, not warnings.
// (`clippy::dbg_macro` is denied workspace-wide; `print_*` stay core-specific here.)
#![deny(clippy::print_stdout, clippy::print_stderr)]
// The public API is the FFI's data contract; every exported item must be documented.
#![deny(missing_docs)]

mod config;
pub mod ops;
mod pipeline;

pub use config::{
    parse_config, BracketStyle, CaseKind, Config, ConfigError, Operation, Ordering, CONFIG_VERSION,
    MAX_CONFIG_OPERATIONS, MAX_CONFIG_TEXT_PARAM_BYTES, MAX_PIPELINE_GROWTH_FACTOR,
};
pub use pipeline::transform;

/// Static JSON describing this core build: name, version, the config schema
/// version, and every supported operation. A shell queries this (via the FFI
/// `ss_capabilities_json`) to discover what the core can do without hardcoding it.
///
/// It is a compile-time constant: returning it across the FFI needs no allocation
/// and no matching free. `config_version` is asserted against [`CONFIG_VERSION`]
/// by a unit test so the two cannot silently diverge.
pub const CAPABILITIES_JSON: &str = concat!(
    r#"{"name":"xpare-core","version":""#,
    env!("CARGO_PKG_VERSION"),
    r#"","config_version":2,"ordering":["canonical","as_given"],"operations":["#,
    r#"{"op":"strip_html"},"#,
    r#"{"op":"strip_markdown"},"#,
    r#"{"op":"html_to_markdown"},"#,
    r#"{"op":"collapse_whitespace"},"#,
    r#"{"op":"trim_trailing_whitespace"},"#,
    r#"{"op":"remove_blank_lines"},"#,
    r#"{"op":"unwrap_lines"},"#,
    r#"{"op":"change_case","cases":["upper","lower","title","sentence"]},"#,
    r#"{"op":"sort_lines","params":["descending","case_insensitive"]},"#,
    r#"{"op":"dedupe_lines"},"#,
    r#"{"op":"prefix_lines","params":["prefix"]},"#,
    r#"{"op":"suffix_lines","params":["suffix"]},"#,
    r#"{"op":"join_with","params":["separator"]},"#,
    r#"{"op":"split_on","params":["delimiter"]},"#,
    r#"{"op":"extract_emails"},"#,
    r#"{"op":"extract_urls"},"#,
    r#"{"op":"defang","params":["style"],"styles":["square","round"]},"#,
    r#"{"op":"refang"},"#,
    r#"{"op":"clean_urls"},"#,
    r#"{"op":"mask_identifiers","params":["emails","ipv4","ipv6"]}"#,
    r#"]}"#,
);

/// Returns [`CAPABILITIES_JSON`], the core's self-description.
pub fn capabilities() -> &'static str {
    CAPABILITIES_JSON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_is_valid_json_and_version_consistent() {
        let value: serde_json::Value =
            serde_json::from_str(capabilities()).expect("capabilities must be valid JSON");
        assert_eq!(
            value["config_version"].as_u64(),
            Some(u64::from(CONFIG_VERSION)),
            "capabilities config_version must match CONFIG_VERSION"
        );
        assert_eq!(value["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
        assert!(value["operations"].is_array());
    }
}
