//! Tests for the `clean_urls` text operation.
//!
//! `clean_urls` strips known tracking query parameters (UTM, click ids, etc.) from
//! URL-like tokens while preserving every non-URL token, all whitespace, and the
//! exact spelling (including percent-encoding) of every surviving parameter.
//!
//! These exercise the public function directly
//! ([`safetystrip_core::ops::urls::clean_urls`]) — the load-bearing contract is the
//! token-oriented, panic-free, idempotent transform documented on that function.
//!
//! NOTE: at the time these tests were written there is no `Operation::CleanUrls`
//! variant / pipeline dispatch arm in this worktree, so the tests call the free
//! function directly rather than routing through `transform`. The property
//! strategies mirror `core/tests/determinism.rs`.

use safetystrip_core::ops::urls::{clean_urls, TRACKING_PARAMS};

use proptest::prelude::*;

/// Golden table: `(input, expected)`. One row per documented behavior.
#[test]
fn golden_table() {
    let cases: &[(&str, &str)] = &[
        // A URL whose only params are trackers loses the entire query string,
        // including the leading '?'.
        (
            "https://example.com/path?utm_source=x&utm_medium=y",
            "https://example.com/path",
        ),
        // Mixed kept + tracker params: kept params survive in original order.
        (
            "https://example.com/?a=1&utm_source=x&b=2",
            "https://example.com/?a=1&b=2",
        ),
        // Tracker in the middle is removed; surrounding kept params keep order.
        (
            "https://example.com/?first=1&gclid=abc&second=2",
            "https://example.com/?first=1&second=2",
        ),
        // `utm_*` prefix matching: any utm_-prefixed key is dropped.
        (
            "https://example.com/?utm_campaign=spring&keep=ok",
            "https://example.com/?keep=ok",
        ),
        // Case-insensitive key match: UTM_Source matches utm_source/utm_*.
        (
            "https://example.com/?UTM_Source=Newsletter&id=42",
            "https://example.com/?id=42",
        ),
        // Fragment is preserved; trackers before '#' are removed.
        (
            "https://example.com/page?utm_source=x#section-2",
            "https://example.com/page#section-2",
        ),
        // Fragment with a surviving param.
        (
            "https://example.com/page?keep=1&utm_source=x#frag",
            "https://example.com/page?keep=1#frag",
        ),
        // Click-id trackers.
        ("https://example.com/?fbclid=AAA", "https://example.com/"),
        // openstat + Wicked Reports trackers are dropped; kept params survive.
        (
            "https://example.com/?_openstat=abc&keep=1",
            "https://example.com/?keep=1",
        ),
        ("https://example.com/?wickedid=zzz", "https://example.com/"),
        (
            "https://example.com/?gclid=BBB&msclkid=CCC",
            "https://example.com/",
        ),
        // No query string: returned unchanged.
        (
            "https://example.com/path/to/page",
            "https://example.com/path/to/page",
        ),
        // Plain http and www. prefixes are recognized.
        (
            "http://example.com/?utm_source=x&q=hello",
            "http://example.com/?q=hello",
        ),
        (
            "www.example.com/?utm_term=y&page=2",
            "www.example.com/?page=2",
        ),
        // Non-URL token containing '?' and '&' is returned byte-for-byte.
        ("what?is&this=thing", "what?is&this=thing"),
        // Prose with no URL is untouched.
        (
            "the quick brown fox jumps over the lazy dog",
            "the quick brown fox jumps over the lazy dog",
        ),
        // Empty string.
        ("", ""),
        // Percent-encoded surviving value is left intact.
        (
            "https://example.com/?q=a%20b%26c&utm_source=x",
            "https://example.com/?q=a%20b%26c",
        ),
        // `ref` is deliberately NOT a tracker and must survive.
        (
            "https://example.com/?ref=homepage&utm_source=x",
            "https://example.com/?ref=homepage",
        ),
        // `q`, `id`, `page` must survive (never tracker keys).
        (
            "https://example.com/?q=search&id=7&page=3&utm_source=x",
            "https://example.com/?q=search&id=7&page=3",
        ),
        // Surrounding punctuation: trailing ')' and comma are re-emitted around the
        // cleaned URL; the URL itself is cleaned.
        (
            "(https://example.com/?utm_source=x),",
            "(https://example.com/),",
        ),
        // A URL embedded in prose with whitespace preserved verbatim.
        (
            "see https://example.com/?utm_source=x   now",
            "see https://example.com/   now",
        ),
        // Bare '?' with no params: query is empty -> '?' dropped.
        ("https://example.com/?", "https://example.com/"),
        // '?' followed only by a tracker keeps nothing.
        ("https://example.com/?utm_source=", "https://example.com/"),
        // Empty-value kept pair `key=` survives spelled exactly.
        (
            "https://example.com/?keep=&utm_source=x",
            "https://example.com/?keep=",
        ),
        // Key with no '=' at all is a valid pair and survives if not a tracker.
        (
            "https://example.com/?flag&utm_source=x",
            "https://example.com/?flag",
        ),
        // Tracker with no '=' is still dropped (key-only form).
        (
            "https://example.com/?utm_source&keep=1",
            "https://example.com/?keep=1",
        ),
        // Empty pairs from '&&' are dropped (no spurious empty key survives).
        (
            "https://example.com/?a=1&&b=2",
            "https://example.com/?a=1&b=2",
        ),
        // Multiple '?': only the FIRST starts the query; later '?' are literal in
        // the query value and preserved.
        (
            "https://example.com/?keep=a?b&utm_source=x",
            "https://example.com/?keep=a?b",
        ),
        // The FIRST '#' after the query starts the fragment; later '#' stay in frag.
        (
            "https://example.com/?utm_source=x#a#b",
            "https://example.com/#a#b",
        ),
        // Fragment-only (no query) URL untouched.
        (
            "https://example.com/page#top",
            "https://example.com/page#top",
        ),
    ];

    for (input, expected) in cases {
        let got = clean_urls(input);
        assert_eq!(&got, expected, "clean_urls({input:?})");
    }
}

/// Idempotence on the golden inputs: a second pass changes nothing.
#[test]
fn golden_inputs_are_idempotent() {
    let inputs = [
        "https://example.com/path?utm_source=x&utm_medium=y",
        "https://example.com/?a=1&utm_source=x&b=2",
        "https://example.com/page?utm_source=x#section-2",
        "(https://example.com/?utm_source=x),",
        "see https://example.com/?utm_source=x   now",
        "https://example.com/?q=a%20b%26c&utm_source=x",
    ];
    for input in inputs {
        let once = clean_urls(input);
        let twice = clean_urls(&once);
        assert_eq!(once, twice, "clean_urls not idempotent on {input:?}");
    }
}

#[test]
fn every_configured_tracker_key_is_dropped() {
    for &entry in TRACKING_PARAMS {
        let concrete = match entry.strip_suffix('*') {
            Some(stem) => format!("{stem}campaign"),
            None => entry.to_string(),
        };
        let uppercase = concrete.to_ascii_uppercase();
        for key in [&concrete, &uppercase] {
            let input = format!("https://example.com/?keep=1&{key}=x&after=2");
            assert_eq!(
                clean_urls(&input),
                "https://example.com/?keep=1&after=2",
                "tracker key {key:?} from entry {entry:?} should be dropped",
            );
        }
    }
}

#[test]
fn tracker_prefix_stems_without_separator_are_kept() {
    assert_eq!(
        clean_urls("https://example.com/?utm=1&oly=2&utm-source=3&keep=4"),
        "https://example.com/?utm=1&oly=2&utm-source=3&keep=4"
    );
}

// --- Property tests (mirror determinism.rs strategies) ----------------------

/// A pool of "interesting" characters biased toward URL structure.
fn interesting_char() -> impl Strategy<Value = char> {
    prop_oneof![
        20 => prop_oneof![
            Just('\n'), Just('\r'), Just(' '), Just('\t'),
            Just('?'), Just('&'), Just('='), Just('#'),
            Just('/'), Just(':'), Just('.'), Just('%'),
            Just('<'), Just('>'), Just('"'), Just('\''),
            Just(','), Just(';'), Just('('), Just(')'),
        ],
        8 => prop::char::range('a', 'z'),
        4 => prop::char::range('A', 'Z'),
        2 => prop::char::range('0', '9'),
        3 => prop_oneof![
            Just('ß'), Just('İ'), Just('Σ'), Just('ﬁ'), Just('é'),
            Just('\u{00a0}'), Just('\u{0307}'), Just('🦀'),
        ],
        4 => any::<char>(),
    ]
}

fn interesting_string() -> impl Strategy<Value = String> {
    prop::collection::vec(interesting_char(), 0..80).prop_map(|chars| chars.into_iter().collect())
}

/// A token guaranteed NOT to start with a URL prefix (no leading whitespace, and we
/// reject the http/https/www prefixes), plus surrounding whitespace runs.
fn ws_run() -> impl Strategy<Value = String> {
    prop::collection::vec(prop_oneof![Just(' '), Just('\t'), Just('\n')], 0..5)
        .prop_map(|c| c.into_iter().collect::<String>())
}

fn non_url_embedded() -> impl Strategy<Value = (String, String, String)> {
    let token = prop::collection::vec(
        prop_oneof![
            prop::char::range('a', 'z'),
            prop::char::range('0', '9'),
            Just('?'),
            Just('&'),
            Just('='),
            Just('#'),
            Just('.'),
        ],
        1..30,
    )
    .prop_map(|c| c.into_iter().collect::<String>())
    .prop_filter("must not be a URL token", |t| {
        let trimmed = t.trim_matches(|c: char| {
            matches!(
                c,
                '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '"' | '\''
            )
        });
        !(trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("www."))
    });
    (ws_run(), token, ws_run())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Panic-free + deterministic on arbitrary input.
    #[test]
    fn never_panics_and_is_deterministic(input in interesting_string()) {
        let a = clean_urls(&input);
        let b = clean_urls(&input);
        prop_assert_eq!(a, b);
    }

    /// Idempotence: applying twice equals once.
    #[test]
    fn is_idempotent(input in interesting_string()) {
        let once = clean_urls(&input);
        let twice = clean_urls(&once);
        prop_assert_eq!(once, twice);
    }

    /// A token that is not a URL is returned byte-for-byte even when embedded in
    /// arbitrary whitespace.
    #[test]
    fn non_url_tokens_are_invariant((lead, token, trail) in non_url_embedded()) {
        let input = format!("{lead}{token}{trail}");
        prop_assert_eq!(clean_urls(&input), input);
    }
}
