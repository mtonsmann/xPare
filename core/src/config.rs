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
pub const CONFIG_VERSION: u32 = 1;

/// A transformation request: a schema version plus an ordered pipeline of operations.
///
/// Operations run in the exact order given. Order is significant and always
/// explicit — the core never reorders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Schema version. Must equal [`CONFIG_VERSION`].
    pub version: u32,
    /// Operations applied left-to-right.
    #[serde(default)]
    pub operations: Vec<Operation>,
}

impl Config {
    /// A config with a known version and no operations (the identity transform).
    pub fn empty() -> Self {
        Self {
            version: CONFIG_VERSION,
            operations: Vec::new(),
        }
    }
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
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Json(msg) => write!(f, "invalid config json: {msg}"),
            ConfigError::UnsupportedVersion { found, supported } => write!(
                f,
                "unsupported config version {found} (this core supports {supported})"
            ),
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
    Ok(config)
}
