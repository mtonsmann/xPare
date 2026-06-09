//! Large-input ("heavy log file work") benchmarks, up to 256 MB.
//!
//! Separate from `transform.rs` (the quick clipboard-scale benches) because these
//! allocate hundreds of MB and take minutes. Run explicitly:
//!
//! ```sh
//! cargo bench -p xpare-core --bench transform_large    # or: make bench-large
//! ```
//!
//! They report throughput (MB/s) for the line-oriented ops a log workflow leans on
//! (dedupe, sort, remove-blank, extract) plus a realistic cleanup pipeline across a
//! size sweep, and the strippers as a scaling reference. The pass/fail counterpart
//! is the `--ignored` 256 MB test in `core/tests/perf_guard.rs`.

use std::fmt::Write as _;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use xpare_core::ops::html::strip_html;
use xpare_core::{transform, Config, Operation};

const MB: usize = 1024 * 1024;

/// Build a deterministic synthetic log of about `target` bytes. Lines look like real
/// structured logs and include a controlled amount of duplication (the `user` field
/// repeats every 500k lines) so `dedupe`/`sort` do meaningful work. No RNG, so the
/// corpus is identical across runs.
fn synthetic_log(target: usize) -> String {
    let mut s = String::with_capacity(target + 256);
    let mut i: u64 = 0;
    while s.len() < target {
        let _ = writeln!(
            s,
            "2026-06-05T12:34:56.{:03}Z INFO  [auth.session] user=u{} action=login \
             ip=10.0.{}.{} req=abc{}def status=200 latency_ms={} url=https://example.com/p/{}",
            i % 1000,
            i % 500_000,
            (i / 256) % 256,
            i % 256,
            i,
            i % 900,
            i % 4096,
        );
        i += 1;
    }
    s
}

fn config(ops: Vec<Operation>) -> Config {
    Config::as_given(ops)
}

/// A realistic log-cleanup pipeline measured across a size sweep up to 256 MB.
fn bench_log_pipeline_scaling(c: &mut Criterion) {
    let mut g = c.benchmark_group("log_pipeline_scaling");
    g.sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(15));
    let cfg = config(vec![
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::DedupeLines,
    ]);
    for &mb in &[1usize, 16, 64, 256] {
        let input = synthetic_log(mb * MB);
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(
            BenchmarkId::from_parameter(format!("{mb}MB")),
            &input,
            |b, inp| {
                b.iter(|| transform(black_box(inp), &cfg));
            },
        );
    }
    g.finish();
}

/// Each line-oriented op at 256 MB, reusing one input.
fn bench_line_ops_256mb(c: &mut Criterion) {
    let input = synthetic_log(256 * MB);
    let mut g = c.benchmark_group("line_ops_256mb");
    g.sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(20))
        .throughput(Throughput::Bytes(input.len() as u64));
    let ops: [(&str, Vec<Operation>); 5] = [
        ("dedupe_lines", vec![Operation::DedupeLines]),
        (
            "sort_lines",
            vec![Operation::SortLines {
                descending: false,
                case_insensitive: false,
            }],
        ),
        (
            "sort_lines_insensitive",
            vec![Operation::SortLines {
                descending: false,
                case_insensitive: true,
            }],
        ),
        ("remove_blank_lines", vec![Operation::RemoveBlankLines]),
        ("extract_urls", vec![Operation::ExtractUrls]),
    ];
    for (name, op) in ops {
        let cfg = config(op);
        g.bench_function(name, |b| b.iter(|| transform(black_box(&input), &cfg)));
    }
    g.finish();
}

/// The HTML stripper at 256 MB — a scaling reference for the untrusted-input parser
/// (logs are not HTML, but this proves the workhorse stays linear at scale).
fn bench_strip_html_256mb(c: &mut Criterion) {
    let input = synthetic_log(256 * MB);
    let mut g = c.benchmark_group("strip_html_256mb");
    g.sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(20))
        .throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("plain_log", |b| b.iter(|| strip_html(black_box(&input))));
    g.finish();
}

criterion_group!(
    benches,
    bench_log_pipeline_scaling,
    bench_line_ops_256mb,
    bench_strip_html_256mb
);
criterion_main!(benches);
