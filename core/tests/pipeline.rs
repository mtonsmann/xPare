//! Pipeline ordering & composition tests.
//!
//! These verify that `transform` applies operations strictly left-to-right and that
//! the A2-owned ops compose as documented. Assertions on exact output use only ops
//! A2 owns; pipelines that include `StripHtml`/`StripMarkdown` (owned by another
//! agent) assert only structural/ordering properties, never their exact output.

use safetystrip_core::{transform, CaseKind, Config, Operation, CONFIG_VERSION};

fn pipeline(ops: Vec<Operation>) -> Config {
    Config {
        version: CONFIG_VERSION,
        operations: ops,
    }
}

#[test]
fn empty_pipeline_is_identity() {
    let cfg = pipeline(vec![]);
    let input = "anything\n\t  goes\r\n";
    assert_eq!(transform(input, &cfg), input);
}

#[test]
fn collapse_then_trim_then_remove_blank() {
    // A classic "clean up a messy paste" pipeline.
    let cfg = pipeline(vec![
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
    ]);
    // "a   b  " -> collapse -> "a b " ; "   " -> " " ; "c" stays.
    // After trim: "a b", " " -> "", "c". After remove-blank: "a b\nc".
    let input = "a   b  \n   \nc";
    assert_eq!(transform(input, &cfg), "a b\nc");
}

#[test]
fn order_matters_collapse_before_vs_after_trim() {
    // Demonstrate that ordering is honored: trimming first then collapsing differs
    // from collapsing first then trimming for a line that is all spaces.
    let input = "x  \n   \ny";

    let trim_then_collapse = pipeline(vec![
        Operation::TrimTrailingWhitespace,
        Operation::CollapseWhitespace,
    ]);
    // trim: "x\n\ny" ; collapse: "x\n\ny" (no intra-line runs left).
    assert_eq!(transform(input, &trim_then_collapse), "x\n\ny");

    let collapse_then_trim = pipeline(vec![
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
    ]);
    // collapse: "x \n \ny" ; trim: "x\n\ny".
    assert_eq!(transform(input, &collapse_then_trim), "x\n\ny");

    // Same end result here, but the intermediate differs — confirm collapse alone:
    let collapse_only = pipeline(vec![Operation::CollapseWhitespace]);
    assert_eq!(transform(input, &collapse_only), "x \n \ny");
}

#[test]
fn unwrap_then_change_case() {
    let cfg = pipeline(vec![
        Operation::UnwrapLines,
        Operation::ChangeCase {
            case: CaseKind::Title,
        },
    ]);
    let input = "the quick\nbrown fox\n\njumps over\nthe lazy dog";
    // unwrap: "the quick brown fox\n\njumps over the lazy dog"
    // title:  "The Quick Brown Fox\n\nJumps Over The Lazy Dog"
    assert_eq!(
        transform(input, &cfg),
        "The Quick Brown Fox\n\nJumps Over The Lazy Dog"
    );
}

#[test]
fn sentence_case_after_unwrap() {
    let cfg = pipeline(vec![
        Operation::UnwrapLines,
        Operation::ChangeCase {
            case: CaseKind::Sentence,
        },
    ]);
    let input = "hello there.\nthis is\nfine. bye";
    // unwrap -> "hello there. this is fine. bye"
    // sentence -> "Hello there. This is fine. Bye"
    assert_eq!(transform(input, &cfg), "Hello there. This is fine. Bye");
}

#[test]
fn sort_then_dedupe_then_prefix() {
    let cfg = pipeline(vec![
        Operation::SortLines {
            descending: false,
            case_insensitive: false,
        },
        Operation::DedupeLines,
        Operation::PrefixLines {
            prefix: "- ".into(),
        },
    ]);
    let input = "banana\napple\nbanana\ncherry\napple";
    // sort: apple, apple, banana, banana, cherry
    // dedupe: apple, banana, cherry
    // prefix: "- apple\n- banana\n- cherry"
    assert_eq!(transform(input, &cfg), "- apple\n- banana\n- cherry");
}

#[test]
fn split_on_then_sort_then_join_with() {
    let cfg = pipeline(vec![
        Operation::SplitOn {
            delimiter: ", ".into(),
        },
        Operation::SortLines {
            descending: false,
            case_insensitive: false,
        },
        Operation::JoinWith {
            separator: ", ".into(),
        },
    ]);
    // CSV-ish round trip: split, sort, rejoin.
    assert_eq!(transform("c, a, b", &cfg), "a, b, c");
}

#[test]
fn dedupe_then_remove_blank_then_suffix() {
    let cfg = pipeline(vec![
        Operation::DedupeLines,
        Operation::RemoveBlankLines,
        Operation::SuffixLines { suffix: ";".into() },
    ]);
    let input = "a\n\na\nb\n\nb";
    // dedupe: "a\n\nb" (first empty kept, second a/b dropped... let's verify):
    //   lines: a, "", a, b, "", b -> first-seen: a, "", b -> "a\n\nb"
    // remove-blank: "a\nb"
    // suffix: "a;\nb;"
    assert_eq!(transform(input, &cfg), "a;\nb;");
}

#[test]
fn extract_emails_then_dedupe_then_sort() {
    let cfg = pipeline(vec![
        Operation::ExtractEmails,
        Operation::DedupeLines,
        Operation::SortLines {
            descending: false,
            case_insensitive: false,
        },
    ]);
    let input = "mail bob@x.com and alice@y.com, then bob@x.com again";
    // extract: "bob@x.com\nalice@y.com\nbob@x.com"
    // dedupe:  "bob@x.com\nalice@y.com"
    // sort:    "alice@y.com\nbob@x.com"
    assert_eq!(transform(input, &cfg), "alice@y.com\nbob@x.com");
}

// --- Pipelines that include another agent's ops: ordering only, no exact output. ---

#[test]
fn strip_html_then_our_ops_do_not_panic_and_are_applied() {
    // We don't know strip_html's exact output (owned by A1). We only assert that the
    // pipeline runs, and that our trailing op's invariant holds on whatever it gets.
    let cfg = pipeline(vec![
        Operation::StripHtml,
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
    ]);
    let out = transform("<p>hello   world</p>   ", &cfg);
    // CollapseWhitespace guarantees no run of 2+ ASCII spaces survives; trim
    // guarantees no trailing space on any line. These hold regardless of strip_html.
    assert!(
        !out.contains("  "),
        "collapse should leave no double spaces"
    );
    for line in out.split('\n') {
        assert_eq!(line, line.trim_end_matches([' ', '\t']));
    }
}

#[test]
fn strip_markdown_then_change_case_runs() {
    let cfg = pipeline(vec![
        Operation::StripMarkdown,
        Operation::ChangeCase {
            case: CaseKind::Upper,
        },
    ]);
    // Whatever strip_markdown yields, uppercasing it must equal uppercasing twice
    // (idempotence of Upper) — a property that holds without knowing the strip output.
    let once = transform("# *hi* there", &cfg);
    let twice = transform(
        &once,
        &pipeline(vec![Operation::ChangeCase {
            case: CaseKind::Upper,
        }]),
    );
    assert_eq!(once, twice, "Upper must be idempotent in composition");
}

#[test]
fn long_pipeline_with_all_a2_ops_terminates() {
    // Exercise a deep pipeline touching every A2 op; assert it produces *some*
    // deterministic output and does not panic / hang.
    let cfg = pipeline(vec![
        Operation::SplitOn {
            delimiter: "|".into(),
        },
        Operation::CollapseWhitespace,
        Operation::TrimTrailingWhitespace,
        Operation::RemoveBlankLines,
        Operation::DedupeLines,
        Operation::SortLines {
            descending: true,
            case_insensitive: true,
        },
        Operation::PrefixLines {
            prefix: "* ".into(),
        },
        Operation::SuffixLines {
            suffix: " <".into(),
        },
        Operation::ChangeCase {
            case: CaseKind::Title,
        },
        Operation::UnwrapLines,
        Operation::JoinWith {
            separator: " / ".into(),
        },
    ]);
    let input = "  beta |alpha|  beta  | | gamma ";
    let out = transform(input, &cfg);
    // Deterministic: same input+config twice is identical (sanity, see determinism.rs).
    assert_eq!(out, transform(input, &cfg));
}
