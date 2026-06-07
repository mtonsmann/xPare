//! Configuration schema — the data that crosses the FFI boundary.
//!
//! Feature selection is **data, not API**: a config is a versioned, ordered list
//! of [`Operation`]s. Adding or changing a transform means adding an enum variant
//! here and handling it in the pipeline — it never changes the C ABI, because the
//! ABI only ever passes this structure across as a JSON string.
//!
//! This file is part of the frozen interface contract. The schema is exercised by
//! round-trip and version tests; bump [`CONFIG_VERSION`] for incompatible changes.

use serde::{Deserialize, Serialize};

/// Current config schema version. `parse_config` rejects any other version.
///
/// **v2** added [`Config::ordering`] (canonical-by-default operation ordering); see
/// DESIGN.md decision D13. A v1 config is rejected and must add the version field's
/// new value — `ordering` itself is optional (defaults to `Canonical`).
pub const CONFIG_VERSION: u32 = 2;

/// Maximum number of operations accepted in a parsed config.
///
/// This is deliberately above the product UI's everyday operation count but low
/// enough that repeated whole-buffer transforms cannot turn a tiny config into an
/// unbounded resource request.
pub const MAX_CONFIG_OPERATIONS: usize = 32;

/// Maximum UTF-8 byte length for each free-text operation parameter.
///
/// Prefix/suffix/join/split parameters are configuration, not clipboard content, and
/// product use cases are short tokens such as `"> "`, `", "`, or `"|"`. Keeping
/// them bounded prevents tiny configs from requesting huge per-line output.
pub const MAX_CONFIG_TEXT_PARAM_BYTES: usize = 256;

/// A transformation request: a schema version, a pipeline of operations, and how that
/// pipeline is ordered.
///
/// By default ([`Ordering::Canonical`]) the core stable-sorts the operations into a
/// documented canonical order (see [`Operation::canonical_rank`] / DESIGN.md D13) so
/// the result is correct and efficient regardless of the order a UI assembled them.
/// [`Ordering::AsGiven`] runs them in the exact order provided — the explicit,
/// byte-for-byte contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Schema version. Must equal [`CONFIG_VERSION`].
    pub version: u32,
    /// Operations to apply. Interpreted per [`Config::ordering`].
    #[serde(default)]
    pub operations: Vec<Operation>,
    /// How `operations` is ordered before running. Optional; defaults to
    /// [`Ordering::Canonical`].
    #[serde(default)]
    pub ordering: Ordering,
}

impl Config {
    /// A config with a known version and no operations (the identity transform).
    pub fn empty() -> Self {
        Self {
            version: CONFIG_VERSION,
            operations: Vec::new(),
            ordering: Ordering::default(),
        }
    }

    /// A current-version config that runs `operations` in the documented canonical
    /// order (the default ordering).
    pub fn canonical(operations: Vec<Operation>) -> Self {
        Self {
            version: CONFIG_VERSION,
            operations,
            ordering: Ordering::Canonical,
        }
    }

    /// A current-version config that runs `operations` in exactly the given order.
    pub fn as_given(operations: Vec<Operation>) -> Self {
        Self {
            version: CONFIG_VERSION,
            operations,
            ordering: Ordering::AsGiven,
        }
    }

    /// Validate the product resource envelope for a config.
    ///
    /// `transform` remains infallible once handed a [`Config`], so callers that build
    /// configs programmatically should use this before running untrusted clipboard
    /// content. [`parse_config`] calls it automatically for the JSON/FFI/CLI path.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.operations.len() > MAX_CONFIG_OPERATIONS {
            return Err(ConfigError::TooManyOperations {
                found: self.operations.len(),
                max: MAX_CONFIG_OPERATIONS,
            });
        }
        for op in &self.operations {
            op.validate_resource_envelope()?;
        }
        Ok(())
    }
}

/// How the pipeline order is interpreted.
///
/// `Canonical` (the default) stable-sorts operations by their documented
/// [`Operation::canonical_rank`], so a caller never has to reason about order;
/// `AsGiven` runs them exactly as provided (what the CLI and order-sensitive callers
/// want). Stable sort means any operations sharing a rank keep their input order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ordering {
    /// Reorder into the documented canonical order (correct + efficient by default).
    #[default]
    Canonical,
    /// Run operations in the exact order given.
    AsGiven,
}

/// One transformation step. Serialized as an internally-tagged object keyed on `op`,
/// e.g. `{"op":"change_case","case":"title"}` or `{"op":"strip_html"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    // --- Must (common baseline) ---
    /// Strip HTML tags and decode common entities → plain text.
    StripHtml,
    /// Strip Markdown formatting → plain text.
    StripMarkdown,
    /// Convert common copied-web HTML structure → Markdown plain text.
    HtmlToMarkdown,
    /// Collapse runs of spaces/tabs to a single space.
    CollapseWhitespace,
    /// Trim trailing whitespace from each line.
    TrimTrailingWhitespace,
    /// Remove blank/empty lines.
    RemoveBlankLines,
    /// Join wrapped lines into paragraphs, preserving paragraph breaks.
    /// (Exact rule documented on `ops::lines::unwrap_lines`.)
    UnwrapLines,
    /// Recase the whole text.
    ChangeCase { case: CaseKind },

    // --- Stretch (implemented; see capabilities()) ---
    /// Sort lines.
    SortLines {
        #[serde(default)]
        descending: bool,
        #[serde(default)]
        case_insensitive: bool,
    },
    /// Remove duplicate lines, keeping first occurrence and original order.
    DedupeLines,
    /// Prefix every (non-empty) line with `prefix`.
    PrefixLines { prefix: String },
    /// Suffix every (non-empty) line with `suffix`.
    SuffixLines { suffix: String },
    /// Join all lines into one, separated by `separator`.
    JoinWith { separator: String },
    /// Split on a custom delimiter: replace each `delimiter` with a newline.
    SplitOn { delimiter: String },
    /// Extract email-like tokens, one per line (best-effort, documented heuristic).
    ExtractEmails,
    /// Extract URL-like tokens, one per line (best-effort, documented heuristic).
    ExtractUrls,

    // --- IOC handling (rewrites; see DESIGN.md D12 + the IOC contract) ---
    /// Defang network indicators (URLs, hosts, IPv4/IPv6, emails) so they are inert
    /// but human-readable and reversible. Idempotent. (Rule on `ops::defang::defang`.)
    Defang {
        #[serde(default)]
        style: BracketStyle,
    },
    /// Reverse `Defang` — re-activate received IOCs. The textual inverse of the
    /// defang substitution set. (Rule on `ops::defang::refang`.)
    Refang,
    /// Strip tracking/analytics query parameters from URL tokens, preserving every
    /// other parameter, their order, and any fragment. Idempotent; baked-in
    /// denylist (no network). (Rule on `ops::urls::clean_urls`.)
    CleanUrls,
    /// Replace selected email/IP tokens with fixed placeholders. Heuristic,
    /// token-oriented, and idempotent. (Rule on `ops::mask::mask_identifiers`.)
    MaskIdentifiers {
        #[serde(default)]
        emails: bool,
        #[serde(default)]
        ipv4: bool,
        #[serde(default)]
        ipv6: bool,
    },
}

impl Operation {
    /// Canonical execution rank — lower runs first. Under [`Ordering::Canonical`] the
    /// pipeline is stable-sorted by this rank. The ranking is a documented total
    /// order (DESIGN.md D13) encoding correctness constraints (e.g. `StripHtml`
    /// before `StripMarkdown`; `CleanUrls` before `Defang`; `TrimTrailingWhitespace`
    /// before `DedupeLines`; `JoinWith` last) and efficiency ones (`DedupeLines`
    /// before `SortLines` — output-identical but cheaper). Distinct ranks per variant
    /// keep canonical output independent of input order; genuinely free choices
    /// (e.g. prefix vs suffix) fall back to the stable sort's input order.
    pub fn canonical_rank(&self) -> u16 {
        match self {
            Operation::StripHtml => 1,
            Operation::StripMarkdown => 2,
            Operation::HtmlToMarkdown => 3,
            Operation::SplitOn { .. } => 4,
            Operation::UnwrapLines => 5,
            Operation::CollapseWhitespace => 6,
            Operation::TrimTrailingWhitespace => 7,
            Operation::CleanUrls => 8,
            Operation::MaskIdentifiers { .. } => 9,
            Operation::Defang { .. } => 10,
            Operation::Refang => 11,
            Operation::ExtractEmails => 12,
            Operation::ExtractUrls => 13,
            Operation::RemoveBlankLines => 14,
            Operation::DedupeLines => 15,
            Operation::SortLines { .. } => 16,
            Operation::ChangeCase { .. } => 17,
            Operation::PrefixLines { .. } => 18,
            Operation::SuffixLines { .. } => 19,
            Operation::JoinWith { .. } => 20,
        }
    }

    fn validate_resource_envelope(&self) -> Result<(), ConfigError> {
        match self {
            Operation::PrefixLines { prefix } => {
                validate_text_param("prefix_lines", "prefix", prefix)
            }
            Operation::SuffixLines { suffix } => {
                validate_text_param("suffix_lines", "suffix", suffix)
            }
            Operation::JoinWith { separator } => {
                validate_text_param("join_with", "separator", separator)
            }
            Operation::SplitOn { delimiter } => {
                validate_text_param("split_on", "delimiter", delimiter)
            }
            Operation::StripHtml
            | Operation::StripMarkdown
            | Operation::HtmlToMarkdown
            | Operation::CollapseWhitespace
            | Operation::TrimTrailingWhitespace
            | Operation::RemoveBlankLines
            | Operation::UnwrapLines
            | Operation::ChangeCase { .. }
            | Operation::SortLines { .. }
            | Operation::DedupeLines
            | Operation::ExtractEmails
            | Operation::ExtractUrls
            | Operation::Defang { .. }
            | Operation::Refang
            | Operation::CleanUrls
            | Operation::MaskIdentifiers { .. } => Ok(()),
        }
    }
}

fn validate_text_param(
    op: &'static str,
    param: &'static str,
    value: &str,
) -> Result<(), ConfigError> {
    if value.len() > MAX_CONFIG_TEXT_PARAM_BYTES {
        return Err(ConfigError::TextParamTooLong {
            op,
            param,
            found: value.len(),
            max: MAX_CONFIG_TEXT_PARAM_BYTES,
        });
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(ConfigError::TextParamContainsLineBreak { op, param });
    }
    Ok(())
}

/// Bracket convention used by [`Operation::Defang`]. The default (`Square`, `[.]`)
/// is the de-facto infosec standard; `Round` (`(.)`) is offered as an alternative.
/// `Refang` reverses **both** styles, so it needs no parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BracketStyle {
    /// `[.]`, `[@]`, `[://]`, `[:]` — the default.
    #[default]
    Square,
    /// `(.)`, `(@)`, `(://)`, `(:)`.
    Round,
}

/// Case transformation kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    /// UPPERCASE.
    Upper,
    /// lowercase.
    Lower,
    /// Title Case (capitalize the first letter of each word).
    Title,
    /// Sentence case (capitalize the first letter of each sentence).
    Sentence,
}

/// Errors from parsing a config string. Deliberately small and FFI-friendly:
/// every variant maps to a stable status code at the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The JSON was malformed or did not match the schema.
    Json(String),
    /// The `version` field is not [`CONFIG_VERSION`].
    UnsupportedVersion { found: u32, supported: u32 },
    /// The config listed too many operations.
    TooManyOperations { found: usize, max: usize },
    /// A free-text operation parameter exceeded [`MAX_CONFIG_TEXT_PARAM_BYTES`].
    TextParamTooLong {
        op: &'static str,
        param: &'static str,
        found: usize,
        max: usize,
    },
    /// A free-text operation parameter contained `\r` or `\n`.
    TextParamContainsLineBreak {
        op: &'static str,
        param: &'static str,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Json(msg) => write!(f, "invalid config json: {msg}"),
            ConfigError::UnsupportedVersion { found, supported } => write!(
                f,
                "unsupported config version {found} (this core supports {supported})"
            ),
            ConfigError::TooManyOperations { found, max } => {
                write!(f, "config has {found} operations, maximum is {max}")
            }
            ConfigError::TextParamTooLong {
                op,
                param,
                found,
                max,
            } => write!(f, "{op}.{param} is {found} bytes, maximum is {max}"),
            ConfigError::TextParamContainsLineBreak { op, param } => {
                write!(f, "{op}.{param} must be a single line")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parse and validate a JSON config string.
///
/// Returns [`ConfigError::UnsupportedVersion`] if the version does not match this
/// build, so a shell can detect a capability mismatch deterministically.
pub fn parse_config(json: &str) -> Result<Config, ConfigError> {
    let config: Config =
        serde_json::from_str(json).map_err(|e| ConfigError::Json(e.to_string()))?;
    if config.version != CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion {
            found: config.version,
            supported: CONFIG_VERSION,
        });
    }
    config.validate()?;
    Ok(config)
}
