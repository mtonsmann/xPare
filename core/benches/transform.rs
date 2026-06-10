//! Performance benchmarks for the transformation core.
//!
//! Run with `cargo bench -p xpare-core` (or `make bench`). These measure
//! throughput of the untrusted-input parsers (`strip_html` / `strip_markdown`), the
//! default rich→plain pipeline, and the case transforms across input sizes and
//! shapes — including the adversarial shapes whose *linear-time* behavior
//! `core/tests/perf_guard.rs` asserts in CI. Benchmarks measure; the guard fails.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use xpare_core::ops::html::strip_html;
use xpare_core::ops::markdown::strip_markdown;
use xpare_core::{transform, CaseKind, Config, Operation};

/// A representative rich HTML fragment (headings, inline marks, link, list).
fn html_block() -> &'static str {
    "<div class=\"post\"><h2>Title &amp; more</h2><p>Some <b>bold</b> and \
     <a href=\"https://example.com\">a link</a> with <code>code</code>.</p>\
     <ul><li>one</li><li>two</li></ul></div>"
}

/// A representative GitHub-flavored Markdown block.
fn markdown_block() -> &'static str {
    "# Heading\n\nSome **bold**, *italic*, `code`, and [a link](https://example.com).\n\n\
     - one\n- two\n\n| a | b |\n| - | - |\n| 1 | 2 |\n"
}

fn config(ops: Vec<Operation>) -> Config {
    Config::as_given(ops)
}

fn bench_strip_html(c: &mut Criterion) {
    let mut g = c.benchmark_group("strip_html");
    for &reps in &[1usize, 64, 1024] {
        let input = html_block().repeat(reps);
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("doc_bytes", input.len()),
            &input,
            |b, inp| {
                b.iter(|| strip_html(black_box(inp)));
            },
        );
    }
    // Adversarial shapes: must stay linear (perf_guard asserts this; here we measure).
    let adversarial = [
        ("nested_divs", "<div>".repeat(50_000)),
        (
            "spaces_then_breaks",
            " ".repeat(50_000) + &"<br>".repeat(50_000),
        ),
        ("entities", "&#x41;&amp;".repeat(50_000)),
        ("angle_flood", "<".repeat(50_000)),
    ];
    for (name, input) in &adversarial {
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(BenchmarkId::new("adversarial", name), input, |b, inp| {
            b.iter(|| strip_html(black_box(inp)));
        });
    }
    g.finish();
}

fn bench_strip_markdown(c: &mut Criterion) {
    let mut g = c.benchmark_group("strip_markdown");
    for &reps in &[1usize, 64, 1024] {
        let input = markdown_block().repeat(reps);
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("doc_bytes", input.len()),
            &input,
            |b, inp| {
                b.iter(|| strip_markdown(black_box(inp)));
            },
        );
    }
    g.finish();
}

fn bench_pipeline_default(c: &mut Criterion) {
    let mut g = c.benchmark_group("pipeline_default");
    let cfg = config(vec![
        Operation::StripHtml,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
    ]);
    for &reps in &[16usize, 256, 4096] {
        let input = html_block().repeat(reps);
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(
            BenchmarkId::from_parameter(input.len()),
            &input,
            |b, inp| {
                b.iter(|| transform(black_box(inp), &cfg));
            },
        );
    }
    g.finish();
}

fn bench_change_case(c: &mut Criterion) {
    let mut g = c.benchmark_group("change_case");
    // Mixed-script text so the full-Unicode case mapping is exercised, not just ASCII.
    let input = "The Quick Brown Fox jumps over the lazy dog. Grüße, straße! ".repeat(4096);
    g.throughput(Throughput::Bytes(input.len() as u64));
    for (name, kind) in [
        ("upper", CaseKind::Upper),
        ("title", CaseKind::Title),
        ("sentence", CaseKind::Sentence),
    ] {
        let cfg = config(vec![Operation::ChangeCase { case: kind }]);
        g.bench_with_input(BenchmarkId::from_parameter(name), &input, |b, inp| {
            b.iter(|| transform(black_box(inp), &cfg));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_strip_html,
    bench_strip_markdown,
    bench_pipeline_default,
    bench_change_case
);
criterion_main!(benches);
