//! Performance regression guard (CI-safe).
//!
//! The strippers run on untrusted clipboard input, so super-linear time is a
//! denial-of-service vector. These tests feed LARGE pathological inputs through the
//! strippers and the full pipeline and assert each finishes within a **generous**
//! wall-clock budget. Linear work at these sizes is single/low-double-digit
//! milliseconds even unoptimized; a true `O(n^2)` regression is seconds-to-minutes.
//! The budget sits orders of magnitude between the two, so it catches a catastrophic
//! regression decisively without flaking on a slow/loaded CI runner.
//!
//! The `spaces_then_breaks` case is the exact shape that once exposed an `O(n^2)` in
//! `strip_html`'s newline collapsing (the backward whitespace scan was unbounded);
//! it stays here permanently as a guard. The fuzzer's default 4 KB input cap means
//! scaling bugs like that do not surface there — this test is where they do.

use std::time::{Duration, Instant};

use safetystrip_core::ops::defang::{defang, refang};
use safetystrip_core::ops::html::strip_html;
use safetystrip_core::ops::markdown::strip_markdown;
use safetystrip_core::ops::urls::clean_urls;
use safetystrip_core::{transform, BracketStyle, CaseKind, Config, Operation};

/// Generous per-case ceiling (see module docs for why this can't reasonably flake).
const BUDGET: Duration = Duration::from_secs(8);

#[track_caller]
fn assert_fast(label: &str, input: &str, f: impl Fn(&str) -> String) {
    let start = Instant::now();
    // `black_box` on both ends so the optimizer can neither precompute the call nor
    // discard its result, which would make the timing meaningless.
    let out = f(std::hint::black_box(input));
    let elapsed = start.elapsed();
    std::hint::black_box(&out);
    assert!(
        elapsed < BUDGET,
        "{label}: {} bytes took {elapsed:?} (budget {BUDGET:?}) — likely a super-linear regression",
        input.len()
    );
}

#[test]
fn html_adversarial_inputs_stay_linear() {
    let n = 200_000;
    // The shape that caught the original O(n^2): a long whitespace run followed by
    // many block breaks (each break re-scanned the whole run).
    assert_fast(
        "spaces_then_breaks",
        &(" ".repeat(n) + &"<br>".repeat(n)),
        strip_html,
    );
    assert_fast("nested_divs", &"<div>".repeat(n), strip_html);
    assert_fast("angle_flood", &"<".repeat(n), strip_html);
    assert_fast("entity_flood", &"&#x41;".repeat(n), strip_html);
    assert_fast("amp_flood", &"&".repeat(n), strip_html);
    assert_fast(
        "unterminated_script",
        &("<script>".to_string() + &"a".repeat(n)),
        strip_html,
    );
    assert_fast(
        "text_then_block_pairs",
        &("x ".repeat(n) + &"<p></p>".repeat(n / 10)),
        strip_html,
    );
}

#[test]
fn markdown_adversarial_inputs_stay_linear() {
    let n = 200_000;
    assert_fast("backtick_storm", &"`".repeat(n), strip_markdown);
    assert_fast("blockquote_depth", &">".repeat(n), strip_markdown);
    assert_fast("emphasis_storm", &"*".repeat(n), strip_markdown);
    assert_fast("heading_spam", &"# h\n".repeat(n / 4), strip_markdown);
    assert_fast(
        "codeblock_spaces_then_rules",
        &markdown_codeblock_spaces_then_rules(n),
        strip_markdown,
    );
}

fn markdown_codeblock_spaces_then_rules(n: usize) -> String {
    let mut input = String::with_capacity(n + (n * 4) + 16);
    input.push_str("```\n");
    input.push_str(&" ".repeat(n));
    input.push_str("\n```\n");
    input.push_str(&"---\n".repeat(n));
    input
}

#[test]
fn ioc_ops_stay_linear() {
    let n = 200_000;
    // Token floods: many tiny indicators, one giant token, and adversarial bracket/
    // dot/colon runs — each must stay linear in the hand-rolled scanners.
    assert_fast("defang_url_flood", &"http://a.b/x ".repeat(n), |s| {
        defang(s, BracketStyle::Square)
    });
    assert_fast("defang_ip_flood", &"1.2.3.4 ".repeat(n), |s| {
        defang(s, BracketStyle::Square)
    });
    assert_fast("defang_dot_flood", &".".repeat(n), |s| {
        defang(s, BracketStyle::Square)
    });
    assert_fast("defang_colon_flood", &":".repeat(n), |s| {
        defang(s, BracketStyle::Square)
    });
    assert_fast("refang_marker_flood", &"[.]".repeat(n), refang);
    assert_fast("refang_bracket_flood", &"[".repeat(n), refang);
    assert_fast(
        "clean_urls_param_flood",
        &("https://e.com/?".to_string() + &"utm_source=x&".repeat(n)),
        clean_urls,
    );
    assert_fast(
        "clean_urls_token_flood",
        &"https://e.com/?a=1 ".repeat(n),
        clean_urls,
    );
}

#[test]
fn full_pipeline_on_large_rich_input_stays_linear() {
    let input = "<p>Hello <b>world</b> &amp; friends</p>\n  spaced   out  \n".repeat(2_000);
    let config = Config::as_given(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::ChangeCase {
            case: CaseKind::Title,
        },
    ]);
    let start = Instant::now();
    let out = transform(std::hint::black_box(&input), &config);
    let elapsed = start.elapsed();
    std::hint::black_box(&out);
    assert!(
        elapsed < BUDGET,
        "full pipeline: {} bytes took {elapsed:?} (budget {BUDGET:?})",
        input.len()
    );
}

/// Heavy-input scaling check for log-file work. **Ignored by default**: it allocates
/// ~256 MB (peak working set ~1 GB) and is not something every CI run should pay for.
/// Run explicitly:
///
/// ```sh
/// cargo test -p safetystrip-core --test perf_guard -- --ignored
/// ```
///
/// Asserts a realistic 256 MB log-cleanup pipeline (collapse → trim → dedupe → sort)
/// completes well within a generous linear-time budget. Measured baseline is a few
/// seconds; a super-linear regression at this size would be minutes. Benchmarks for
/// throughput live in `core/benches/transform_large.rs`.
#[test]
#[ignore = "allocates ~256 MB; run with `--ignored`"]
fn handles_256mb_log_pipeline() {
    const TARGET: usize = 256 * 1024 * 1024;
    let mut input = String::with_capacity(TARGET + 256);
    let mut i: u64 = 0;
    while input.len() < TARGET {
        use std::fmt::Write as _;
        let _ = writeln!(
            input,
            "2026-06-05T12:34:56.{:03}Z INFO [svc] user=u{} ip=10.0.{}.{} status=200 latency_ms={}",
            i % 1000,
            i % 500_000,
            (i / 256) % 256,
            i % 256,
            i % 900
        );
        i += 1;
    }
    let config = Config::as_given(vec![
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::DedupeLines,
        Operation::SortLines {
            descending: false,
            case_insensitive: false,
        },
    ]);
    // Generous: this test runs in debug by default (use `--release` for realistic
    // timing); the budget only needs to catch a catastrophic super-linear regression,
    // which at 256 MB would be minutes, not the few seconds this takes.
    let budget = Duration::from_secs(60);
    let start = Instant::now();
    let out = transform(std::hint::black_box(&input), &config);
    let elapsed = start.elapsed();
    std::hint::black_box(&out);
    assert!(
        elapsed < budget,
        "256 MB log pipeline took {elapsed:?} (budget {budget:?}) — possible super-linear regression"
    );
}
