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
/// real product use cases are tiny tokens — `"> "`, `", "`, `"|"` (1–2 bytes). `16`
/// is generous for those while keeping the only attacker-influenceable free-text data
/// that crosses the FFI small, and capping a single affix's per-line growth factor at
/// `1 + 16 = 17`. (Was `256`; tightened to shrink both the per-op growth factor and
/// the FFI free-text surface — see DESIGN.md D3 and `SECURITY.md`.)
pub const MAX_CONFIG_TEXT_PARAM_BYTES: usize = 16;

/// Maximum worst-case output-growth factor for a whole pipeline.
///
/// [`Config::validate`] rejects any config whose operations could, in the worst
/// case, multiply the input byte length by more than this. Each operation has a
/// conservative per-op growth bound ([`Operation::max_growth_factor`]); because
/// `transform` folds the operations in sequence (`out_i <= in_i * factor_i`), the
/// *product* of those bounds is a sound upper bound on the whole pipeline's growth.
///
/// Bounding that product — not just each operation in isolation — is what stops a
/// tiny config of individually envelope-legal operations from amplifying a small
/// input without bound. The per-op caps ([`MAX_CONFIG_OPERATIONS`],
/// [`MAX_CONFIG_TEXT_PARAM_BYTES`]) bound a single pass; they do **not** bound
/// composition, where a `SplitOn` re-maximizes the line count for a following
/// `PrefixLines`/`JoinWith` to re-amplify — the multiplicative blow-up the fuzzer
/// originally found (a ~1.7 KiB input that expanded past 2 GiB before the OS killed
/// the worker, back when this cap was `2^18`).
///
/// `2^12` (4096x) is deliberately conservative: a realistic sanitization pipeline
/// (e.g. `StripHtml → StripMarkdown → CollapseWhitespace → PrefixLines "> " →
/// JoinWith ", "`) has a growth product of ~6, so the cap sits hundreds of times
/// above anything real while bounding the worst accepted amplification far below any
/// out-of-memory threshold. Combined with the 16-byte param ceiling (per-op affix
/// factor ≤ 17), even three stacked max-length affixes (17^3 = 4913) exceed it. See
/// `SECURITY.md` ("Configs are envelope-bounded before transform"). The `kani_proofs`
/// module proves this gate cannot wrap a genuinely-amplifying product into acceptance.
pub const MAX_PIPELINE_GROWTH_FACTOR: u64 = 1 << 12;

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
        // Bound the pipeline's worst-case output growth. Each op's output is at most
        // `factor` times its input, so the product over the pipeline bounds total
        // growth; reject anything that could amplify past the cap. The product is
        // computed by [`saturating_growth_product`] — factored out so the saturating
        // arithmetic (the part that must never wrap a genuinely unbounded product down
        // to a small, falsely-accepted value) can be model-checked with Kani; see the
        // `kani_proofs` module.
        let growth =
            saturating_growth_product(self.operations.iter().map(Operation::max_growth_factor));
        if growth > MAX_PIPELINE_GROWTH_FACTOR {
            return Err(ConfigError::PipelineMayAmplify {
                factor: growth,
                max: MAX_PIPELINE_GROWTH_FACTOR,
            });
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

    /// Conservative upper bound on this operation's output-to-input byte ratio, over
    /// *all* inputs. [`Config::validate`] multiplies these across the pipeline to
    /// bound total amplification (see [`MAX_PIPELINE_GROWTH_FACTOR`]). Always rounded
    /// up, so the product can only over-estimate growth — it never lets an amplifying
    /// pipeline through.
    ///
    /// The amplifiers are the per-line/per-newline rewrites whose growth scales with a
    /// free-text parameter; their factor is derived from the operation's *actual*
    /// parameter length rather than the [`MAX_CONFIG_TEXT_PARAM_BYTES`] cap, so the
    /// short tokens real configs use (`"> "`, `", "`) stay tight against the bound.
    /// Every other operation either cannot grow its input (factor `1`) or grows it by
    /// a small, parameter-independent constant verified against its `ops/` source.
    fn max_growth_factor(&self) -> u64 {
        match self {
            // Per-line affixes: worst case is an all-single-byte-lines input, so each
            // of up to `input.len()` lines grows by the affix length -> 1 + affix_len.
            Operation::PrefixLines { prefix } => 1 + prefix.len() as u64,
            Operation::SuffixLines { suffix } => 1 + suffix.len() as u64,
            // JoinWith replaces every '\n' (at most `input.len()` of them) with the
            // separator; worst case every byte is a newline -> max(1, separator_len).
            Operation::JoinWith { separator } => (separator.len() as u64).max(1),
            // Bounded-constant expanders (max single-token expansion, verified against
            // the `ops/` sources; rounded up):
            //   HtmlToMarkdown — code-fence backtick sizing + special-char escaping.
            //   ChangeCase     — Unicode case mapping (e.g. `İ` -> `i̇`, 2 -> 3 bytes).
            //   Defang         — `.` -> `[.]`, `@` -> `[@]`, `:` -> `[:]` (1 -> 3 bytes).
            //   MaskIdentifiers— shortest token -> fixed placeholder (e.g. `[ipv6]`).
            Operation::HtmlToMarkdown => 3,
            Operation::ChangeCase { .. } => 3,
            Operation::Defang { .. } => 3,
            Operation::MaskIdentifiers { .. } => 2,
            // Shrink-or-equal: these never increase the byte length. `SplitOn` replaces
            // a >=1-byte delimiter with a single '\n', so it cannot grow either.
            Operation::StripHtml
            | Operation::StripMarkdown
            | Operation::CollapseWhitespace
            | Operation::TrimTrailingWhitespace
            | Operation::RemoveBlankLines
            | Operation::UnwrapLines
            | Operation::SortLines { .. }
            | Operation::DedupeLines
            | Operation::SplitOn { .. }
            | Operation::ExtractEmails
            | Operation::ExtractUrls
            | Operation::Refang
            | Operation::CleanUrls => 1,
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
    /// The pipeline's worst-case output-growth factor (the product of the operations'
    /// per-op growth bounds) exceeded [`MAX_PIPELINE_GROWTH_FACTOR`]. Such a config
    /// could turn a small input into an unbounded-size transform — a resource-
    /// exhaustion (DoS) vector — so it is rejected before the infallible transform.
    /// `factor` saturates at [`u64::MAX`] for pipelines whose product overflows.
    PipelineMayAmplify { factor: u64, max: u64 },
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
            ConfigError::PipelineMayAmplify { factor, max } => write!(
                f,
                "config pipeline could amplify output up to {factor}x, maximum is {max}x"
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
    config.validate()?;
    Ok(config)
}

/// Saturating product of a pipeline's per-op growth factors, starting from `1` (the
/// identity, so an empty pipeline yields factor `1`).
///
/// Factored out of [`Config::validate`] so the saturating arithmetic can be reasoned
/// about — and model-checked with Kani — independently of the `String`-bearing
/// `Config`. The load-bearing safety property is that this can only ever
/// *over*-estimate growth: a genuinely unbounded product saturates at [`u64::MAX`]
/// rather than wrapping to a small value that would slip past
/// [`MAX_PIPELINE_GROWTH_FACTOR`]. The `kani_proofs` module proves that the gate
/// `saturating_growth_product(..) <= MAX_PIPELINE_GROWTH_FACTOR` accepts a pipeline
/// *iff* its true (arbitrary-precision) worst-case growth is within the cap.
pub(crate) fn saturating_growth_product(factors: impl IntoIterator<Item = u64>) -> u64 {
    factors
        .into_iter()
        .fold(1u64, |acc, factor| acc.saturating_mul(factor))
}

// ---------------------------------------------------------------------------
// Bounded proofs (Kani) over the resource-envelope arithmetic.
// ---------------------------------------------------------------------------
//
// These compile ONLY under `cargo kani` (the `kani` cfg), so they are invisible to
// normal `cargo build` / `cargo test` and to `cargo metadata` — the `kani` crate
// never enters the dependency tree that `check-core-deps` guards. Run them with
// `cargo xtask check-kani`. They prove the crisp arithmetic only and deliberately do
// not attempt to model-check the `String`-bearing config or the text transformer.
//
// Design note — prove the STEP, not the unrolled fold. `saturating_growth_product`
// folds up to `MAX_CONFIG_OPERATIONS` saturating multiplies. Model-checking that fold
// directly (unrolling 32 symbolic 64-bit multiplies) is exactly what a SAT backend is
// worst at — multiplication bit-blasts heavily — and it conflates loop unwinding with
// the property. Instead we prove three per-step lemmas, each over a SINGLE symbolic
// multiply (tiny, fast, and immune to unwinding), and compose them by the documented
// induction below. The result is also *stronger*: it holds for pipelines of ANY
// length, not just up to `MAX_CONFIG_OPERATIONS`.
//
// Let `cap = MAX_PIPELINE_GROWTH_FACTOR` and fold the per-op factors (each `>= 1`,
// since `max_growth_factor` is — see the `max_growth_factor_is_always_at_least_one`
// unit test) with `saturating_mul`, starting at `1`. Write `sat_k` for the running
// saturating product after `k` factors and `true_k` for the exact (unbounded) one.
// Claim: `saturating_growth_product(..) <= cap` iff `true_n <= cap` (the gate accepts
// iff the real worst-case growth is within the cap). Proof by induction on the fold,
// using just two lemmas (each one symbolic multiply):
//
//   * Base: `sat_0 = true_0 = 1 <= cap`.
//   * `step_exact_below_cap`: while `sat_{k-1} <= cap`, the next saturating step equals
//     the exact step (`sat_{k-1} * f`, which cannot overflow because `cap * MAX_FACTOR
//     < u64::MAX`). So as long as the running product stays within the cap, `sat`
//     tracks `true` exactly — an accepted pipeline's gate value IS its true growth.
//   * `absorbing_above_cap`: once `sat_{k-1} > cap`, every further factor `>= 1` keeps
//     it `> cap` — a rejecting pipeline can never be "rescued" back into acceptance.
//
// Together: if `true_n <= cap`, every prefix is `<= cap` (the product is monotone, as
// every factor is `>= 1`), so by step_exact_below_cap, applied at each step, `sat_n =
// true_n <= cap` → accept. If `true_n > cap`, let `k` be the first prefix to exceed
// the cap; for `j < k` we have `true_j <= cap` so `sat_j = true_j`, and at step `k`,
// `sat_{k-1} = true_{k-1} <= cap`, so step_exact_below_cap gives `sat_k = sat_{k-1} *
// f_k = true_k > cap` (no over_estimate lemma needed — the crossing step is itself
// overflow-free and exact). Then absorbing_above_cap keeps `sat_n > cap` → reject.
// Hence the gate is exact: no saturation wrap can accept an amplifier.

/// Bounded proofs over the saturating growth-envelope arithmetic. See
/// [`saturating_growth_product`] and the induction in the section comment above.
#[cfg(kani)]
mod kani_proofs {
    use super::{MAX_CONFIG_TEXT_PARAM_BYTES, MAX_PIPELINE_GROWTH_FACTOR};

    /// The widest value a single per-op growth factor can take: `1 +
    /// MAX_CONFIG_TEXT_PARAM_BYTES` (a maximum-length affix), per
    /// `Operation::max_growth_factor`. The `max_growth_factor_never_exceeds_the_kani_proof_bound`
    /// unit test keeps this in sync with production.
    const MAX_FACTOR: u64 = 1 + MAX_CONFIG_TEXT_PARAM_BYTES as u64; // 17

    /// step_exact_below_cap: while the running product is within the cap, one more
    /// saturating multiply by a valid factor equals the exact product — and that exact
    /// product cannot overflow `u64` (`cap * MAX_FACTOR < u64::MAX`). So an
    /// in-envelope pipeline's gate value tracks its true growth exactly.
    #[kani::proof]
    fn step_exact_below_cap_matches_true_product() {
        let acc: u64 = kani::any();
        let f: u64 = kani::any();
        kani::assume(acc <= MAX_PIPELINE_GROWTH_FACTOR);
        kani::assume(f >= 1 && f <= MAX_FACTOR);
        // No overflow: acc * f <= cap * MAX_FACTOR, which is far below u64::MAX. Kani
        // checks the multiply for overflow here (nothing assumes it away).
        let exact = acc * f;
        assert!(acc.saturating_mul(f) == exact);
    }

    /// absorbing_above_cap: once the running product has exceeded the cap, any further
    /// factor `>= 1` keeps it above the cap. So a rejecting pipeline can never be
    /// "rescued" into acceptance by later operations.
    #[kani::proof]
    fn absorbing_above_cap_stays_above() {
        let acc: u64 = kani::any();
        let f: u64 = kani::any();
        kani::assume(acc > MAX_PIPELINE_GROWTH_FACTOR);
        kani::assume(f >= 1);
        assert!(acc.saturating_mul(f) > MAX_PIPELINE_GROWTH_FACTOR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One value of every [`Operation`] variant, for exhaustive per-variant checks.
    fn one_of_every_variant() -> Vec<Operation> {
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
            Operation::SortLines {
                descending: false,
                case_insensitive: false,
            },
            Operation::DedupeLines,
            Operation::PrefixLines {
                prefix: "> ".into(),
            },
            Operation::SuffixLines {
                suffix: " <".into(),
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
            Operation::Refang,
            Operation::CleanUrls,
            Operation::MaskIdentifiers {
                emails: true,
                ipv4: true,
                ipv6: true,
            },
        ]
    }

    #[test]
    fn saturating_growth_product_folds_and_saturates() {
        assert_eq!(
            saturating_growth_product([]),
            1,
            "empty pipeline is identity"
        );
        assert_eq!(saturating_growth_product([3, 2, 1]), 6);
        // A genuinely unbounded product saturates at u64::MAX instead of wrapping.
        assert_eq!(saturating_growth_product([u64::MAX, 2, 2]), u64::MAX);
    }

    #[test]
    fn canonical_rank_is_a_total_order_over_all_variants() {
        // Distinct rank per variant is what makes canonical output independent of the
        // order a UI listed the operations in (DESIGN.md D13).
        let ops = one_of_every_variant();
        let total = ops.len();
        let mut ranks: Vec<u16> = ops.iter().map(Operation::canonical_rank).collect();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(
            ranks.len(),
            total,
            "canonical_rank must assign a distinct rank to every Operation variant"
        );
    }

    #[test]
    fn max_growth_factor_is_always_at_least_one() {
        // The growth product's monotonicity — and the Kani proof's `f >= 1` assumption
        // on each symbolic factor — depend on no operation ever reporting a factor
        // below 1. A factor of 0 would also let the saturating product collapse to 0
        // and falsely accept an amplifying pipeline.
        for op in one_of_every_variant() {
            assert!(
                op.max_growth_factor() >= 1,
                "{op:?} reports a growth factor below 1"
            );
        }
    }

    #[test]
    fn max_growth_factor_never_exceeds_the_kani_proof_bound() {
        // The Kani harness constrains symbolic factors to `1..=1+MAX_CONFIG_TEXT_PARAM_BYTES`.
        // Keep that bound honest: no real operation may report a wider factor, or the
        // proof would cover a narrower range than production actually produces. A
        // max-length affix is the widest case.
        let widest = MAX_CONFIG_TEXT_PARAM_BYTES as u64 + 1;
        let max_len = "a".repeat(MAX_CONFIG_TEXT_PARAM_BYTES);
        for op in [
            Operation::PrefixLines {
                prefix: max_len.clone(),
            },
            Operation::SuffixLines {
                suffix: max_len.clone(),
            },
            Operation::JoinWith { separator: max_len },
        ] {
            assert!(
                op.max_growth_factor() <= widest,
                "{op:?} exceeds the proof bound"
            );
        }
        for op in one_of_every_variant() {
            assert!(
                op.max_growth_factor() <= widest,
                "{op:?} exceeds the Kani proof's factor bound {widest}"
            );
        }
    }
}
