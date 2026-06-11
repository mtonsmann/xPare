//! Opt-in, synthetic **throughput** harness — the same-machine measurement arm of
//! the performance methodology (see `docs/exec-plans/active/0002-performance-ceiling-and-optimization-loop.md`).
//!
//! This complements, and does not replace, the two layers already in the tree:
//!
//! * `core/benches/*` (criterion) — statistical micro/macro benchmarks with
//!   confidence intervals; the authoritative *measurement* tool.
//! * `core/tests/perf_guard.rs` — the always-on, CI-safe *complexity* gate
//!   (an order-of-magnitude linear-time budget that cannot flake).
//!
//! What this file adds is a single, quick `make perf` reporter that prints a
//! roofline-calibrated MiB/s table for a fast same-machine regression read, plus an
//! **optional** hard floor for calibrated-machine checks. It is `#[ignore]`d so it
//! never runs in the default `cargo test` gate — throughput numbers are noisy on
//! shared CI and belong in opt-in runs, exactly as the methodology prescribes.
//!
//! Invariants this harness upholds (mirroring the guardrails):
//! * **Synthetic input only** — it generates its own buffers and never reads, logs,
//!   or persists real clipboard content.
//! * **Deterministic generators** — no time/RNG in the data, so two runs on the same
//!   machine are comparable.
//! * `black_box` wraps inputs and results so the optimizer cannot elide the work.
//!
//! Run it via `make perf` (see the Makefile) or directly:
//! ```sh
//! SS_PERF_MIB=128 SS_PERF_SAMPLES=7 \
//!   cargo test -p safetystrip-core --release --test throughput -- --ignored --nocapture
//! ```
//! Env knobs: `SS_PERF_MIB` (input size, default 64), `SS_PERF_SAMPLES` (default 3),
//! `SS_PERF_MIN_MIB_PER_SEC` (optional; if set, the end-to-end scenarios must meet
//! this floor or the test fails — use only on a calibrated machine).

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

use safetystrip_core::{transform, BracketStyle, CaseKind, Config, Operation};

const MIB: usize = 1024 * 1024;

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

/// Optional MiB/s floor for the end-to-end scenarios. Absent (or unparseable) means
/// "report only, never fail" — the default, since absolute throughput is noisy.
fn env_floor() -> Option<f64> {
    match std::env::var("SS_PERF_MIN_MIB_PER_SEC") {
        Ok(v) if !v.trim().is_empty() => v.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn target_bytes() -> usize {
    env_usize("SS_PERF_MIB", 64).max(1) * MIB
}

fn samples() -> usize {
    env_usize("SS_PERF_SAMPLES", 3).max(1)
}

/// Repeat `unit` until the buffer reaches at least `target` bytes.
fn repeat_to(unit: &str, target: usize) -> String {
    let mut out = String::with_capacity(target + unit.len());
    while out.len() < target {
        out.push_str(unit);
    }
    out
}

// --- synthetic, deterministic generators -----------------------------------

/// Log-like text with periodic duplicate lines and occasional blank lines, so the
/// line ops (dedupe/sort/remove-blank/unwrap) all do real work.
fn build_log(target: usize) -> String {
    let mut out = String::with_capacity(target + 128);
    let mut n: u64 = 0;
    while out.len() < target {
        // A small rotating set of request ids, with every third line a duplicate of
        // the first, and a blank line every eighth, all without time/RNG.
        let id = ["alpha", "beta", "gamma", "delta"][(n % 4) as usize];
        let level = ["INFO", "WARN", "INFO", "DEBUG"][(n % 4) as usize];
        out.push_str("2026-06-05T00:00:00Z ");
        out.push_str(level);
        out.push_str(" request_id=");
        out.push_str(id);
        out.push_str(" service=api latency_ms=42 msg=request completed\n");
        if n % 3 == 0 {
            out.push_str("2026-06-05T00:00:00Z INFO request_id=alpha service=api latency_ms=42 msg=request completed\n");
        }
        if n % 8 == 0 {
            out.push('\n');
        }
        n += 1;
    }
    out
}

/// Log-like text with no duplicate lines, so `dedupe_lines` measures the expensive
/// "remember everything" path instead of the highly-collapsing repeated-line path.
fn build_unique_log(target: usize) -> String {
    let mut out = String::with_capacity(target + 128);
    let mut n: u64 = 0;
    while out.len() < target {
        out.push_str("2026-06-05T00:00:00Z INFO request_id=");
        let _ = write!(out, "{n:016x}");
        out.push_str(" service=api latency_ms=42 msg=request completed\n");
        n += 1;
    }
    out
}

/// Markup-heavy HTML (tags, quoted attributes, entities) — exercises the stripper.
fn build_html(target: usize) -> String {
    repeat_to(
        "<div class=\"row\"><p>Some <strong>bold</strong> and <a title=\"a&gt;b\" \
         href=\"https://example.com/path?q=1&amp;r=2\">a link</a> &amp; entities \
         &#169; &mdash; done.</p></div>\n",
        target,
    )
}

/// Plain ASCII with no `<` or `&`, to exercise the stripper's marker-free fast path.
fn build_html_plain(target: usize) -> String {
    repeat_to(
        "The quick brown fox jumps over the lazy dog while the sun sets slowly. ",
        target,
    )
}

/// Markdown-heavy text (headings, emphasis, links, lists, blockquotes, code).
fn build_markdown(target: usize) -> String {
    repeat_to(
        "# Heading\n\nSome **bold** and _italic_ and `code` plus a \
         [labelled link](https://example.com/path). \n\n- item one\n- item two\n\n\
         > a quoted line\n\n",
        target,
    )
}

/// Tabs, runs of spaces, trailing whitespace, and blank lines.
fn build_mixed_ws(target: usize) -> String {
    repeat_to("alpha\t\tbeta    gamma   \t\ndelta  \t \n\n\n", target)
}

/// Mixed-script text with case-expanding characters, for the Unicode case paths.
fn build_unicode(target: usize) -> String {
    repeat_to(
        "Hello WORLD. this is a TEST sentence! straße ärger Größe. другой язык здесь. ",
        target,
    )
}

/// Prose peppered with every indicator class `defang` rewrites — URLs, emails,
/// IPv4, IPv6, and bare domains — so the tokenizer + classifiers all do real work.
fn build_iocs(target: usize) -> String {
    repeat_to(
        "Contact ops@example.com or visit https://www.example.com/path?q=1 today. \
         Block 192.168.10.20 and 2001:db8::dead:beef at the edge, and avoid \
         evil-domain.example.org plus http://tracker.test/landing for now. ",
        target,
    )
}

/// URLs carrying a realistic mix of tracking and functional query params, so
/// `clean_urls` parses and rebuilds a non-trivial query string on every token.
fn build_tracker_urls(target: usize) -> String {
    repeat_to(
        "https://shop.example.com/p/12345?utm_source=newsletter&utm_medium=email&\
         utm_campaign=spring&fbclid=AbCdEf123&id=42&ref=home&page=3#reviews and \
         some prose between the links. ",
        target,
    )
}

/// Delimiter-heavy records for split/join/prefix/suffix line operations.
fn build_delimited_records(target: usize) -> String {
    repeat_to("alpha|beta|gamma|delta\none|two|three|four\n\n", target)
}

fn cfg(operations: Vec<Operation>) -> Config {
    Config::as_given(operations)
}

#[test]
fn operation_performance_coverage_map_covers_all_variants() {
    for op in operation_samples() {
        assert!(
            !operation_performance_scenarios(&op).is_empty(),
            "operation {:?} must have an explicit performance scenario",
            op
        );
    }
}

fn operation_samples() -> Vec<Operation> {
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
            case_insensitive: true,
        },
        Operation::DedupeLines,
        Operation::PrefixLines {
            prefix: "> ".to_string(),
        },
        Operation::SuffixLines {
            suffix: ";".to_string(),
        },
        Operation::JoinWith {
            separator: ", ".to_string(),
        },
        Operation::SplitOn {
            delimiter: "|".to_string(),
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
            ipv6: true,
        },
    ]
}

fn operation_performance_scenarios(op: &Operation) -> &'static str {
    match op {
        Operation::StripHtml => "throughput strip-html-*; criterion strip_html",
        Operation::StripMarkdown => "throughput strip-markdown-*; criterion strip_markdown",
        Operation::HtmlToMarkdown => "throughput html-to-markdown-heavy",
        Operation::CollapseWhitespace => "throughput collapse-whitespace",
        Operation::TrimTrailingWhitespace => "throughput trim-trailing",
        Operation::RemoveBlankLines => "throughput remove-blank-lines",
        Operation::UnwrapLines => "throughput unwrap-lines",
        Operation::ChangeCase { case } => match case {
            CaseKind::Upper => "criterion change_case/upper",
            CaseKind::Lower => "throughput case-lower-ascii",
            CaseKind::Title => "criterion change_case/title",
            CaseKind::Sentence => "throughput case-sentence-unicode",
        },
        Operation::SortLines { .. } => "throughput sort-lines-*; large bench sort_lines*",
        Operation::DedupeLines => "throughput dedupe-lines-*; large bench dedupe_lines",
        Operation::PrefixLines { .. } => "throughput prefix-lines",
        Operation::SuffixLines { .. } => "throughput suffix-lines",
        Operation::JoinWith { .. } => "throughput join-with",
        Operation::SplitOn { .. } => "throughput split-on",
        Operation::ExtractEmails => "throughput extract-emails",
        Operation::ExtractUrls => "throughput extract-urls; large bench extract_urls",
        Operation::Defang { style } => match style {
            BracketStyle::Square => "throughput defang-iocs",
            BracketStyle::Round => "throughput defang-iocs (same scanner, alternate style)",
        },
        Operation::Refang => "throughput refang-iocs",
        Operation::CleanUrls => "throughput clean-urls-trackers",
        Operation::MaskIdentifiers { .. } => "throughput mask-identifiers; perf_guard mask_*",
    }
}

// --- timing -----------------------------------------------------------------

struct Stat {
    median: Duration,
    min: Duration,
    max: Duration,
}

/// Time `f` `n` times and return median/min/max. The first run is included; for
/// same-machine comparison the median is the reported figure.
fn time_samples(n: usize, mut f: impl FnMut()) -> Stat {
    let mut durations: Vec<Duration> = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        f();
        durations.push(start.elapsed());
    }
    durations.sort_unstable();
    Stat {
        median: durations[durations.len() / 2],
        min: durations[0],
        max: durations[durations.len() - 1],
    }
}

fn mibps(bytes: usize, d: Duration) -> f64 {
    let secs = d.as_secs_f64();
    if secs <= 0.0 {
        return f64::INFINITY;
    }
    (bytes as f64 / MIB as f64) / secs
}

/// One reported row. `gated` rows are subject to the optional MiB/s floor.
struct Failure {
    name: String,
    got: f64,
    floor: f64,
}

#[allow(clippy::too_many_arguments)]
fn report(
    name: &str,
    in_bytes: usize,
    out_bytes: usize,
    stat: &Stat,
    gated: bool,
    floor: Option<f64>,
    failures: &mut Vec<Failure>,
) {
    let tput = mibps(in_bytes, stat.median);
    println!(
        "{name:<28} in={in_mib:>6.1}MiB out={out_mib:>6.1}MiB  median={median:>7.3}s  \
         min={min:>7.3}s  max={max:>7.3}s  {tput:>9.1} MiB/s",
        in_mib = in_bytes as f64 / MIB as f64,
        out_mib = out_bytes as f64 / MIB as f64,
        median = stat.median.as_secs_f64(),
        min = stat.min.as_secs_f64(),
        max = stat.max.as_secs_f64(),
    );
    if gated {
        if let Some(floor) = floor {
            if tput < floor {
                failures.push(Failure {
                    name: name.to_string(),
                    got: tput,
                    floor,
                });
            }
        }
    }
}

/// Measure a `transform` scenario: build output once for the byte count, then time.
fn bench_transform(
    name: &str,
    input: &str,
    config: &Config,
    n: usize,
    gated: bool,
    floor: Option<f64>,
    failures: &mut Vec<Failure>,
) {
    let out_bytes = transform(input, config).len();
    let stat = time_samples(n, || {
        black_box(transform(black_box(input), black_box(config)));
    });
    report(name, input.len(), out_bytes, &stat, gated, floor, failures);
}

#[test]
#[ignore = "opt-in throughput baseline; run via `make perf` (synthetic input only)"]
fn throughput_baseline() {
    let target = target_bytes();
    let n = samples();
    let floor = env_floor();
    let mut failures: Vec<Failure> = Vec::new();

    println!(
        "\nSafetyStrip throughput — {} MiB input, {} samples (median reported){}\n",
        target / MIB,
        n,
        match floor {
            Some(f) => format!(", floor={f} MiB/s on end-to-end scenarios"),
            None => String::new(),
        }
    );

    // --- roofline controls: calibrate the per-op numbers against these ---
    let raw = build_log(target);
    let bytes = raw.as_bytes();
    let scan = time_samples(n, || {
        let mut sum = 0u64;
        for &b in bytes {
            sum = sum.wrapping_add(u64::from(b));
        }
        black_box(sum);
    });
    report(
        "roofline-byte-scan",
        bytes.len(),
        bytes.len(),
        &scan,
        false,
        floor,
        &mut failures,
    );
    let copy = time_samples(n, || {
        black_box(black_box(bytes).to_vec());
    });
    report(
        "roofline-byte-copy",
        bytes.len(),
        bytes.len(),
        &copy,
        false,
        floor,
        &mut failures,
    );

    // --- per-operation scenarios (each on a tailored synthetic buffer) ---
    let html = build_html(target);
    let html_plain = build_html_plain(target);
    let markdown = build_markdown(target);
    let mixed_ws = build_mixed_ws(target);
    let unicode = build_unicode(target);
    let iocs = build_iocs(target);
    let tracker_urls = build_tracker_urls(target);
    let delimited_records = build_delimited_records(target);

    bench_transform(
        "strip-html-plain",
        &html_plain,
        &cfg(vec![Operation::StripHtml]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "strip-html-heavy",
        &html,
        &cfg(vec![Operation::StripHtml]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "strip-html-sparse-log",
        &raw,
        &cfg(vec![Operation::StripHtml]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "strip-markdown-heavy",
        &markdown,
        &cfg(vec![Operation::StripMarkdown]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "strip-markdown-sparse-log",
        &raw,
        &cfg(vec![Operation::StripMarkdown]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "html-to-markdown-heavy",
        &html,
        &cfg(vec![Operation::HtmlToMarkdown]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "collapse-whitespace",
        &mixed_ws,
        &cfg(vec![Operation::CollapseWhitespace]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "trim-trailing",
        &mixed_ws,
        &cfg(vec![Operation::TrimTrailingWhitespace]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "remove-blank-lines",
        &raw,
        &cfg(vec![Operation::RemoveBlankLines]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "unwrap-lines",
        &raw,
        &cfg(vec![Operation::UnwrapLines]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "case-lower-ascii",
        &raw,
        &cfg(vec![Operation::ChangeCase {
            case: CaseKind::Lower,
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "case-sentence-unicode",
        &unicode,
        &cfg(vec![Operation::ChangeCase {
            case: CaseKind::Sentence,
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "dedupe-lines-repeated",
        &raw,
        &cfg(vec![Operation::DedupeLines]),
        n,
        false,
        floor,
        &mut failures,
    );
    {
        let unique = build_unique_log(target);
        bench_transform(
            "dedupe-lines-unique",
            &unique,
            &cfg(vec![Operation::DedupeLines]),
            n,
            false,
            floor,
            &mut failures,
        );
    }
    bench_transform(
        "sort-lines",
        &raw,
        &cfg(vec![Operation::SortLines {
            descending: false,
            case_insensitive: false,
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "sort-lines-desc-ci",
        &raw,
        &cfg(vec![Operation::SortLines {
            descending: true,
            case_insensitive: true,
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "prefix-lines",
        &raw,
        &cfg(vec![Operation::PrefixLines {
            prefix: "> ".to_string(),
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "suffix-lines",
        &raw,
        &cfg(vec![Operation::SuffixLines {
            suffix: " ;".to_string(),
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "join-with",
        &raw,
        &cfg(vec![Operation::JoinWith {
            separator: ", ".to_string(),
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "split-on",
        &delimited_records,
        &cfg(vec![Operation::SplitOn {
            delimiter: "|".to_string(),
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "extract-emails",
        &iocs,
        &cfg(vec![Operation::ExtractEmails]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "extract-urls",
        &iocs,
        &cfg(vec![Operation::ExtractUrls]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "defang-iocs",
        &iocs,
        &cfg(vec![Operation::Defang {
            style: BracketStyle::Square,
        }]),
        n,
        false,
        floor,
        &mut failures,
    );
    {
        // Refang over already-defanged input (the realistic case): defang the IOC
        // buffer once, then measure the inverse pass.
        let defanged = transform(
            &iocs,
            &cfg(vec![Operation::Defang {
                style: BracketStyle::Square,
            }]),
        );
        bench_transform(
            "refang-iocs",
            &defanged,
            &cfg(vec![Operation::Refang]),
            n,
            false,
            floor,
            &mut failures,
        );
    }
    bench_transform(
        "clean-urls-trackers",
        &tracker_urls,
        &cfg(vec![Operation::CleanUrls]),
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "mask-identifiers",
        &iocs,
        &cfg(vec![Operation::MaskIdentifiers {
            emails: true,
            ipv4: true,
            ipv6: true,
        }]),
        n,
        false,
        floor,
        &mut failures,
    );

    // --- end-to-end pipelines (gated by the optional floor) ---
    let default_pipeline = cfg(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
    ]);
    let full_menu = cfg(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::DedupeLines,
        Operation::UnwrapLines,
        Operation::ChangeCase {
            case: CaseKind::Lower,
        },
    ]);
    let html_markdown_trim = cfg(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::TrimTrailingWhitespace,
    ]);
    let full_menu_without_markdown = cfg(vec![
        Operation::StripHtml,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::DedupeLines,
        Operation::UnwrapLines,
        Operation::ChangeCase {
            case: CaseKind::Lower,
        },
    ]);
    let full_menu_without_collapse = cfg(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::DedupeLines,
        Operation::UnwrapLines,
        Operation::ChangeCase {
            case: CaseKind::Lower,
        },
    ]);
    let full_menu_without_dedupe = cfg(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::UnwrapLines,
        Operation::ChangeCase {
            case: CaseKind::Lower,
        },
    ]);
    let full_menu_without_case = cfg(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::DedupeLines,
        Operation::UnwrapLines,
    ]);

    // Pipeline decomposition rows: not gated, but useful for choosing the next wave
    // after a baseline because they isolate which passes dominate the end-to-end
    // scenarios on the same generated log buffer.
    bench_transform(
        "html-markdown-trim-log",
        &raw,
        &html_markdown_trim,
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "full-menu-without-markdown",
        &raw,
        &full_menu_without_markdown,
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "full-menu-without-collapse",
        &raw,
        &full_menu_without_collapse,
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "full-menu-without-dedupe",
        &raw,
        &full_menu_without_dedupe,
        n,
        false,
        floor,
        &mut failures,
    );
    bench_transform(
        "full-menu-without-case",
        &raw,
        &full_menu_without_case,
        n,
        false,
        floor,
        &mut failures,
    );

    bench_transform(
        "default-log",
        &raw,
        &default_pipeline,
        n,
        true,
        floor,
        &mut failures,
    );
    bench_transform(
        "full-menu-log",
        &raw,
        &full_menu,
        n,
        true,
        floor,
        &mut failures,
    );

    // Lossy path: inject invalid UTF-8, then decode losslessly (mirrors the FFI/CLI).
    let mut lossy_bytes = raw.clone().into_bytes();
    for i in (0..lossy_bytes.len()).step_by(997) {
        lossy_bytes[i] = 0xFF;
    }
    let lossy = String::from_utf8_lossy(&lossy_bytes).into_owned();
    bench_transform(
        "lossy-utf8-log",
        &lossy,
        &default_pipeline,
        n,
        true,
        floor,
        &mut failures,
    );

    if !failures.is_empty() {
        let mut msg = String::from(
            "\nthroughput floor not met (SS_PERF_MIN_MIB_PER_SEC) on end-to-end scenarios:\n",
        );
        for f in &failures {
            msg.push_str(&format!(
                "  {}: {:.1} MiB/s < {:.1} MiB/s floor\n",
                f.name, f.got, f.floor
            ));
        }
        msg.push_str(
            "\nA floor is only meaningful on a calibrated machine; unset \
             SS_PERF_MIN_MIB_PER_SEC for report-only runs.\n",
        );
        panic!("{msg}");
    }
}
