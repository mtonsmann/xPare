//! Adversarial corpus replay (owner: strippers stream, A1).
//!
//! Recursively reads every file under `core/tests/corpus/`, lossy-decodes its bytes
//! to a `String` (so invalid-UTF-8 fixtures are handled exactly like the FFI
//! boundary handles untrusted clipboard bytes), and runs each through:
//!
//! * [`safetystrip_core::ops::html::strip_html`],
//! * [`safetystrip_core::ops::markdown::strip_markdown`], and
//! * a representative full pipeline via [`safetystrip_core::transform`].
//!
//! The assertion is twofold: **no panic** on any fixture (a panic fails the test
//! process), and a **wall-clock sanity bound** per file to catch accidental
//! superlinear blow-ups in the hand-rolled HTML scanner or our event handling.
//!
//! `std::fs`/`std::time` are used here freely: this is test code, not part of the
//! shipped `#![forbid(unsafe_code)]`, no-I/O library.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use safetystrip_core::ops::{html::strip_html, markdown::strip_markdown};
use safetystrip_core::{transform, CaseKind, Config, Operation};

/// Per-file wall-clock budget. Generous (CI machines vary), but any genuinely
/// quadratic/exponential regression on the multi-MB fixtures blows past it.
const PER_FILE_BUDGET: Duration = Duration::from_secs(5);

/// Directory holding the corpus, resolved relative to this crate's manifest so the
/// test is runnable from any working directory.
fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
}

/// Collect every regular file under `dir`, recursively, in sorted order for
/// deterministic test output.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read corpus dir {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

/// A representative pipeline that exercises both strippers plus the whitespace/line
/// ops in sequence — the realistic "coerce rich → clean plain text" path.
fn representative_pipeline() -> Config {
    Config::as_given(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
    ])
}

/// Run `f` and assert it finishes within [`PER_FILE_BUDGET`]; return its output.
fn timed<T>(label: &str, path: &Path, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    assert!(
        elapsed < PER_FILE_BUDGET,
        "{label} took {elapsed:?} on {} (budget {PER_FILE_BUDGET:?}) — possible superlinear blow-up",
        path.display()
    );
    result
}

#[test]
fn corpus_replays_without_panic_or_blowup() {
    let root = corpus_root();
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    assert!(
        !files.is_empty(),
        "corpus is empty under {} — fixtures missing?",
        root.display()
    );

    let pipeline = representative_pipeline();

    for path in &files {
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        // Lossy decode mirrors the FFI boundary's handling of untrusted bytes.
        let text = String::from_utf8_lossy(&bytes);

        // Each stripper, independently, must complete in time and not panic.
        let _ = timed("strip_html", path, || strip_html(&text));
        let _ = timed("strip_markdown", path, || strip_markdown(&text));
        // The full representative pipeline must too.
        let _ = timed("transform", path, || transform(&text, &pipeline));
    }
}

/// Determinism over the corpus: each stripper yields identical output when run
/// twice on the same fixture (no hashing-order/locale/time dependence).
#[test]
fn corpus_results_are_deterministic() {
    let root = corpus_root();
    let mut files = Vec::new();
    collect_files(&root, &mut files);

    let pipeline = representative_pipeline();

    for path in &files {
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let text = String::from_utf8_lossy(&bytes);

        assert_eq!(
            strip_html(&text),
            strip_html(&text),
            "strip_html nondeterministic on {}",
            path.display()
        );
        assert_eq!(
            strip_markdown(&text),
            strip_markdown(&text),
            "strip_markdown nondeterministic on {}",
            path.display()
        );
        assert_eq!(
            transform(&text, &pipeline),
            transform(&text, &pipeline),
            "transform nondeterministic on {}",
            path.display()
        );
    }
}

/// A second pipeline shape that includes `UnwrapLines` and `ChangeCase`, ensuring
/// the strippers compose with the rest of the op set on adversarial input too.
#[test]
fn corpus_alt_pipeline_completes() {
    let root = corpus_root();
    let mut files = Vec::new();
    collect_files(&root, &mut files);

    let config = Config::as_given(vec![
        Operation::StripHtml,
        Operation::StripMarkdown,
        Operation::UnwrapLines,
        Operation::CollapseWhitespace,
        Operation::ChangeCase {
            case: CaseKind::Lower,
        },
    ]);

    for path in &files {
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let text = String::from_utf8_lossy(&bytes);
        let _ = timed("alt transform", path, || transform(&text, &config));
    }
}
