#![no_main]
//! Fuzz the full transformation pipeline over arbitrary inputs AND arbitrary
//! operation pipelines.
//!
//! This is the high-value target: rather than fuzz a fixed config, we let the
//! fuzzer synthesize the *config itself* — the ordering of operations and all of
//! their string/bool parameters — so it explores how ops compose, not just the
//! input bytes each op sees. The assertion is the same core invariant: `transform`
//! returns for every `(input, config)` and never panics (libFuzzer aborts on a
//! panic; returning normally is success).
//!
//! ## Why a local mirror enum
//!
//! `safetystrip_core::Operation` / `CaseKind` deliberately do not derive
//! `arbitrary::Arbitrary` (the core has no fuzz/test-only deps), and this target
//! may not edit the core. So we define [`LocalOp`] / [`LocalCase`] here — structural
//! mirrors that *do* derive `Arbitrary` — and a total `From` mapping into the real
//! `Operation`. Every real variant is covered exactly once (a missing arm is a
//! compile error in `From`), including `ChangeCase` with each `CaseKind`,
//! `SortLines` flag combinations, the affix/join/split ops with arbitrary strings,
//! both extractors, the two strippers, and the whitespace/line ops.
//!
//! Owner: fuzz stream (E).
//!
//! Run, seeding from the checked-in adversarial corpus. Copy the seeds into the
//! fuzzer's own (gitignored) corpus dir first — never point `fuzz run` directly at
//! `../core/tests/corpus`, as libFuzzer treats the positional dir as writable and
//! would litter that protected tree with discovered inputs:
//!   mkdir -p corpus/transform_pipeline && cp ../core/tests/corpus/pipeline/* corpus/transform_pipeline/
//!   cargo +nightly fuzz run transform_pipeline
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use safetystrip_core::{transform, BracketStyle, CaseKind, Config, Operation};

/// Cap on the number of operations applied in a single fuzz case. The pipeline is
/// linear per op, but each op reallocates the whole string, so an unbounded
/// pipeline of e.g. `PrefixLines` could make one case super-linear in wall-clock
/// time and starve the fuzzer. A generous-but-finite bound keeps each case fast
/// while still letting the fuzzer explore deep orderings. Enforced by truncation
/// in the target (see below).
const MAX_OPS: usize = 32;

/// Mirror of [`safetystrip_core::CaseKind`] that derives `Arbitrary`.
#[derive(Arbitrary, Debug)]
enum LocalCase {
    Upper,
    Lower,
    Title,
    Sentence,
}

impl From<LocalCase> for CaseKind {
    fn from(c: LocalCase) -> Self {
        match c {
            LocalCase::Upper => CaseKind::Upper,
            LocalCase::Lower => CaseKind::Lower,
            LocalCase::Title => CaseKind::Title,
            LocalCase::Sentence => CaseKind::Sentence,
        }
    }
}

/// Mirror of [`safetystrip_core::BracketStyle`] that derives `Arbitrary`.
#[derive(Arbitrary, Debug)]
enum LocalBracketStyle {
    Square,
    Round,
}

impl From<LocalBracketStyle> for BracketStyle {
    fn from(s: LocalBracketStyle) -> Self {
        match s {
            LocalBracketStyle::Square => BracketStyle::Square,
            LocalBracketStyle::Round => BracketStyle::Round,
        }
    }
}

/// Mirror of [`safetystrip_core::Operation`] that derives `Arbitrary`.
///
/// Structurally identical to the real enum (same variants, same payload shapes) so
/// the `From` below is a mechanical 1:1 map. Keep this in lockstep with the core
/// enum: adding an `Operation` variant should add one here, and the `From` arm will
/// fail to compile until it does.
#[derive(Arbitrary, Debug)]
enum LocalOp {
    StripHtml,
    StripMarkdown,
    CollapseWhitespace,
    TrimTrailingWhitespace,
    RemoveBlankLines,
    UnwrapLines,
    ChangeCase {
        case: LocalCase,
    },
    SortLines {
        descending: bool,
        case_insensitive: bool,
    },
    DedupeLines,
    PrefixLines {
        prefix: String,
    },
    SuffixLines {
        suffix: String,
    },
    JoinWith {
        separator: String,
    },
    SplitOn {
        delimiter: String,
    },
    ExtractEmails,
    ExtractUrls,
    Defang { style: LocalBracketStyle },
    Refang,
    CleanUrls,
}

impl From<LocalOp> for Operation {
    fn from(op: LocalOp) -> Self {
        match op {
            LocalOp::StripHtml => Operation::StripHtml,
            LocalOp::StripMarkdown => Operation::StripMarkdown,
            LocalOp::CollapseWhitespace => Operation::CollapseWhitespace,
            LocalOp::TrimTrailingWhitespace => Operation::TrimTrailingWhitespace,
            LocalOp::RemoveBlankLines => Operation::RemoveBlankLines,
            LocalOp::UnwrapLines => Operation::UnwrapLines,
            LocalOp::ChangeCase { case } => Operation::ChangeCase { case: case.into() },
            LocalOp::SortLines {
                descending,
                case_insensitive,
            } => Operation::SortLines {
                descending,
                case_insensitive,
            },
            LocalOp::DedupeLines => Operation::DedupeLines,
            LocalOp::PrefixLines { prefix } => Operation::PrefixLines { prefix },
            LocalOp::SuffixLines { suffix } => Operation::SuffixLines { suffix },
            LocalOp::JoinWith { separator } => Operation::JoinWith { separator },
            LocalOp::SplitOn { delimiter } => Operation::SplitOn { delimiter },
            LocalOp::ExtractEmails => Operation::ExtractEmails,
            LocalOp::ExtractUrls => Operation::ExtractUrls,
            LocalOp::Defang { style } => Operation::Defang {
                style: style.into(),
            },
            LocalOp::Refang => Operation::Refang,
            LocalOp::CleanUrls => Operation::CleanUrls,
        }
    }
}

/// One fuzz case: the operation pipeline plus the input text to run it over.
///
/// `ops` is decoded first so that mutating early bytes steers the *pipeline shape*
/// (the high-value axis); `input` is the last field, so under
/// `arbitrary_take_rest` (which `fuzz_target!` uses) it greedily absorbs the
/// remaining bytes. That also means a raw text corpus seed flows mostly into
/// `input`, exercising real adversarial text through whatever pipeline the leading
/// bytes encode.
#[derive(Arbitrary, Debug)]
struct PipelineInput {
    ops: Vec<LocalOp>,
    /// Fuzz both ordering modes — canonical reordering and as-given.
    canonical: bool,
    input: Vec<u8>,
}

fuzz_target!(|case: PipelineInput| {
    let PipelineInput {
        mut ops,
        canonical,
        input,
    } = case;
    // Hard-cap the pipeline length so a single case stays fast (see `MAX_OPS`).
    ops.truncate(MAX_OPS);

    // Lossy decode mirrors the FFI boundary: the core only ever sees valid UTF-8.
    let text = String::from_utf8_lossy(&input);
    let operations: Vec<Operation> = ops.into_iter().map(Operation::from).collect();
    let config = if canonical {
        Config::canonical(operations)
    } else {
        Config::as_given(operations)
    };
    // Invariant under test: infallible + panic-free for every (input, config).
    let _ = transform(&text, &config);
});
