//! Golden + property tests for the `defang` / `refang` indicator operations.
//!
//! These call the op functions directly (`xpare_core::ops::defang::{defang,
//! refang, BracketStyle}`) — the house style mirrors `golden.rs` (exact-output table
//! tests pinning the documented behavior) and `determinism.rs` (proptest strategies
//! biased toward the interesting bytes, asserting panic-freedom, determinism, and the
//! op-specific algebraic laws: idempotence of `defang` and the `refang ∘ defang`
//! round-trip).

use xpare_core::ops::defang::{defang, refang, BracketStyle};

use proptest::prelude::*;

const SQ: BracketStyle = BracketStyle::Square;
const RD: BracketStyle = BracketStyle::Round;

// ---------------------------------------------------------------------------
// Golden / table tests — exact output for every indicator class.
// ---------------------------------------------------------------------------

#[test]
fn defang_full_url_with_path_and_query() {
    assert_eq!(
        defang("http://example.com/path?q=1", SQ),
        "hxxp[://]example[.]com/path?q=1"
    );
}

#[test]
fn defang_https_url() {
    assert_eq!(
        defang("https://sub.example.com", SQ),
        "hxxps[://]sub[.]example[.]com"
    );
}

#[test]
fn defang_www_url_has_no_scheme_to_mangle() {
    // www. is a URL by the heuristic but has no scheme and no "://", so only its dots
    // are bracketed.
    assert_eq!(defang("www.example.org", SQ), "www[.]example[.]org");
}

#[test]
fn defang_www_url_with_scheme_separator_brackets_only_what_follows_it() {
    // A "www."-classified URL carrying a "://" later in the token exercises the
    // no-scheme branch's separator arithmetic: bytes before the marker are copied
    // verbatim (the leading dot stays live — documented "remaining dots" rule),
    // the marker is bracketed, and exactly the bytes after the three-byte "://"
    // are dot-bracketed. An offset error would drop or re-emit bytes around it.
    assert_eq!(defang("www.://abcdefg.h", SQ), "www.[://]abcdefg[.]h");
}

#[test]
fn defang_email() {
    assert_eq!(defang("user@example.com", SQ), "user[@]example[.]com");
}

#[test]
fn defang_ipv4() {
    assert_eq!(defang("192.168.0.1", SQ), "192[.]168[.]0[.]1");
}

#[test]
fn defang_ipv6() {
    // Every colon is bracketed; the "::" compression yields two adjacent brackets.
    assert_eq!(defang("2001:db8::1", SQ), "2001[:]db8[:][:]1");
}

#[test]
fn defang_bare_domain() {
    assert_eq!(defang("example.com", SQ), "example[.]com");
}

// is_bare_domain boundary coverage (mutation-survivor regressions): the TLD-length,
// edge-hyphen, and allowed-label-byte rules each gate whether a token is treated as a
// bare domain at all. Without these a `<`->`<=`, `||`->`&&`, or `==`->`!=` slip silently.
#[test]
fn defang_bare_domain_two_char_tld() {
    // TLD length must be >= 2: a 2-char TLD is still a domain and must be defanged.
    assert_eq!(defang("example.io", SQ), "example[.]io");
}

#[test]
fn defang_label_with_edge_hyphen_is_not_a_domain() {
    // A label may not start or end with '-', so this is not a bare domain -> left verbatim.
    assert_eq!(defang("bad-.example.com", SQ), "bad-.example.com");
}

#[test]
fn defang_label_with_non_alnum_byte_is_not_a_domain() {
    // Labels are ASCII alphanumeric or '-' only; an '_' disqualifies it -> left verbatim.
    assert_eq!(defang("ex_ample.com", SQ), "ex_ample.com");
}

#[test]
fn defang_mixed_prose_touches_only_the_indicator() {
    assert_eq!(
        defang("Go to http://evil.test now please", SQ),
        "Go to hxxp[://]evil[.]test now please"
    );
}

#[test]
fn defang_prose_words_are_left_alone() {
    // "now." has a trailing-punct '.' stripped as surrounding punctuation, and the
    // core "now" is not an indicator. "please" / "etc" have no dot. Sentences with a
    // word like "Mr." likewise: core "Mr" is not a domain. Nothing changes.
    assert_eq!(
        defang("Hello there, this is fine. Really.", SQ),
        "Hello there, this is fine. Really."
    );
}

#[test]
fn defang_empty_string() {
    assert_eq!(defang("", SQ), "");
}

#[test]
fn defang_no_indicator_text_verbatim() {
    let s = "just some words 12345 and-symbols !@#$ but no full indicator";
    // Note: "!@#$" trims to "@#$" which is not a valid email (no domain dot), and the
    // bare numbers/words are not indicators, so the whole thing is returned verbatim.
    assert_eq!(defang(s, SQ), s);
}

#[test]
fn defang_url_with_port_keeps_the_port_colon() {
    // Only IPv6 brackets colons. A URL's ":8080" port must survive untouched.
    assert_eq!(defang("https://a.b:8080/x", SQ), "hxxps[://]a[.]b:8080/x");
}

#[test]
fn defang_preserves_surrounding_punctuation() {
    // Angle-bracket wrapping (both ends in the trim set) is stripped, the core
    // defanged, and the wrapper re-emitted verbatim around it.
    assert_eq!(defang("<http://c.d>", SQ), "<hxxp[://]c[.]d>");
    // A trailing comma (in the trim set) is preserved; the domain core is defanged.
    assert_eq!(defang("see example.com,", SQ), "see example[.]com,");
    // A trailing '.' is NOT in the trim set, so it stays attached to the core and is
    // bracketed as part of the (URL) indicator — documented trim-set behavior.
    assert_eq!(defang("(http://c.d).", SQ), "(hxxp[://]c[.]d)[.]");
}

#[test]
fn defang_already_defanged_is_a_noop() {
    let already = "hxxps://example[.]com user[@]corp[.]net 192[.]168[.]0[.]1";
    assert_eq!(defang(already, SQ), already);
}

#[test]
fn defang_half_defanged_token_is_left_alone() {
    // The idempotence guard is load-bearing, not belt-and-suspenders: every token
    // below carries a defang marker yet would STILL classify (the "www." URL
    // heuristic or the email heuristic matches it), so only the guard keeps a
    // second pass from double-bracketing. One case per marker, for both styles —
    // each marker alone must trip the guard, not just marker combinations.
    for token in [
        "www.example[.]com",      // [.] alone
        "user[@]corp.net",        // [@] alone
        "www.example.com[://]x",  // [://] alone
        "www.example.com[:]8080", // [:] alone
        "www.example(.)com",      // (.) alone
        "user(@)corp.net",        // (@) alone
        "www.example.com(://)x",  // (://) alone
        "www.example.com(:)8080", // (:) alone
        "user(@)corp(.)net",      // several round markers
    ] {
        assert_eq!(
            defang(token, SQ),
            token,
            "half-defanged {token:?} must be a no-op"
        );
    }
}

#[test]
fn defang_url_with_parenthesized_path_is_still_defanged() {
    // A bare '(' is not a defang marker: the guard requires a complete round marker
    // ("(.)", "(@)", "(://)", "(:)"), so parentheses in a URL path must not make the
    // token read as already-defanged. The trailing ')' is trimmed as surrounding
    // punctuation and re-emitted verbatim.
    assert_eq!(
        defang("http://example.com/page_(1)", SQ),
        "hxxp[://]example[.]com/page_(1)"
    );
}

#[test]
fn defang_url_with_square_bracketed_path_is_still_defanged() {
    // The square-arm twin of the parenthesized-path case above: a bare '[' is not a
    // defang marker either — the guard requires a complete square marker ("[.]",
    // "[@]", "[://]", "[:]"), so brackets in a URL path must not make the token read
    // as already-defanged. Each arm needs its own bracket flavor: a '('-bearing core
    // never exercises the square arm's conjunction, and vice versa.
    assert_eq!(
        defang("http://example.com/page_[1]", SQ),
        "hxxp[://]example[.]com/page_[1]"
    );
}

#[test]
fn defang_url_with_hxxp_in_path_is_left_alone() {
    // The idempotence guard keys on the `hxxp` marker, so a genuine URL whose path
    // already contains "hxxp" is intentionally NOT defanged (documented tradeoff —
    // pinning it here so the no-op is deliberate, not accidental).
    let url = "http://example.com/hxxp";
    assert_eq!(defang(url, SQ), url);
}

#[test]
fn defang_leading_colon_ipv6_is_skipped() {
    // Leading/trailing colons are trimmed as surrounding punctuation, so compressed
    // forms like "::1" fall out of IPv6 classification and are left unchanged
    // (documented heuristic edge).
    assert_eq!(defang("::1", SQ), "::1");
}

#[test]
fn defang_round_style() {
    assert_eq!(defang("http://a.b", RD), "hxxp(://)a(.)b");
    assert_eq!(defang("user@a.b", RD), "user(@)a(.)b");
    assert_eq!(defang("10.0.0.1", RD), "10(.)0(.)0(.)1");
}

#[test]
fn defang_preserves_whitespace_exactly() {
    let s = "a\tb  c\r\nhttp://x.y\n";
    assert_eq!(defang(s, SQ), "a\tb  c\r\nhxxp[://]x[.]y\n");
}

#[test]
fn defang_capacity_bound_is_exact_for_ipv4_worst_case() {
    // The output buffer is pre-sized to `input.len() + 2 * #separators` so it never
    // reallocates (no clipboard-derived bytes left in freed memory). An IPv4 hits
    // that bound exactly: every separator byte gains two bracket bytes.
    let input = "1.1.1.1";
    let out = defang(input, SQ);
    assert_eq!(out, "1[.]1[.]1[.]1");
    assert_eq!(out.len(), input.len() + 2 * 3);
}

// ---------------------------------------------------------------------------
// refang — exact reversals, both styles.
// ---------------------------------------------------------------------------

#[test]
fn refang_square_markers() {
    assert_eq!(
        refang("hxxp[://]example[.]com/path"),
        "http://example.com/path"
    );
    assert_eq!(refang("user[@]example[.]com"), "user@example.com");
    assert_eq!(refang("fe80[:][:]1"), "fe80::1");
}

#[test]
fn refang_round_markers() {
    assert_eq!(refang("hxxps(://)a(.)b"), "https://a.b");
    assert_eq!(refang("user(@)a(.)b"), "user@a.b");
}

#[test]
fn refang_hxxps_restores_https() {
    // Only "hxxp" is mangled, so reversing "hxxp"->"http" also fixes "hxxps".
    assert_eq!(refang("hxxps://x"), "https://x");
}

#[test]
fn refang_empty_string() {
    assert_eq!(refang(""), "");
}

#[test]
fn refang_no_markers_verbatim() {
    let s = "plain text, no markers here at all 1234";
    assert_eq!(refang(s), s);
}

#[test]
fn refang_preserves_long_literal_spans_and_near_misses() {
    let input = "héllo hxx nope [x] (/) 🚀 hxxp[://]x[.]y then hxxps(://)a(.)b";
    let expected = "héllo hxx nope [x] (/) 🚀 http://x.y then https://a.b";
    assert_eq!(refang(input), expected);
}

// ---------------------------------------------------------------------------
// Property tests (mirroring determinism.rs strategies).
// ---------------------------------------------------------------------------

/// Interesting chars, biased toward defang-relevant bytes, plus arbitrary chars.
fn interesting_char() -> impl Strategy<Value = char> {
    prop_oneof![
        20 => prop_oneof![
            Just('\n'), Just('\r'), Just(' '), Just('\t'),
            Just('.'), Just('@'), Just(':'), Just('/'),
            Just('['), Just(']'), Just('('), Just(')'),
            Just('<'), Just('>'), Just('"'), Just('\''),
            Just(','), Just(';'),
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

/// A "marker-free" alphabet for the round-trip property: no `[ ] ( )` and no source
/// that could spell `hxxp`. We build tokens from a safe set so the generated `x`
/// can never already contain a defang marker (the documented round-trip caveat).
fn safe_char() -> impl Strategy<Value = char> {
    prop_oneof![
        Just(' '),
        Just('\n'),
        Just('\t'),
        Just('.'),
        Just('@'),
        Just(':'),
        Just('/'),
        prop::char::range('a', 'z'),
        prop::char::range('0', '9'),
    ]
}

/// Strings over the safe alphabet, additionally filtered so they cannot contain the
/// `hxxp` substring (which `refang` would rewrite, breaking the round-trip).
fn safe_string() -> impl Strategy<Value = String> {
    prop::collection::vec(safe_char(), 0..60)
        .prop_map(|chars| chars.into_iter().collect::<String>())
        .prop_filter("must not contain a pre-existing defang marker", |s| {
            !s.contains("hxxp")
                && !s.contains('[')
                && !s.contains(']')
                && !s.contains('(')
                && !s.contains(')')
        })
}

fn style() -> impl Strategy<Value = BracketStyle> {
    prop_oneof![Just(BracketStyle::Square), Just(BracketStyle::Round)]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// `defang` and `refang` never panic on arbitrary input, for both styles.
    #[test]
    fn never_panics(input in interesting_string(), s in style()) {
        let _ = defang(&input, s);
        let _ = refang(&input);
    }

    /// Both ops are deterministic.
    #[test]
    fn deterministic(input in interesting_string(), s in style()) {
        prop_assert_eq!(defang(&input, s), defang(&input, s));
        prop_assert_eq!(refang(&input), refang(&input));
    }

    /// Idempotence: defanging a defanged string is a no-op (the already-defanged
    /// guard catches every marker), for both styles.
    #[test]
    fn defang_is_idempotent(input in interesting_string(), s in style()) {
        let once = defang(&input, s);
        let twice = defang(&once, s);
        prop_assert_eq!(once, twice);
    }

    /// Round-trip: for inputs free of pre-existing markers, refang inverts defang
    /// exactly, for both styles.
    #[test]
    fn refang_inverts_defang(input in safe_string(), s in style()) {
        let restored = refang(&defang(&input, s));
        prop_assert_eq!(restored, input);
    }

    /// Capacity-bound soundness: `defang` pre-sizes its output to `input.len() +
    /// 2 * #('.'|'@'|':')` so the clipboard-derived buffer never reallocates
    /// mid-build (a reallocation frees the old block unwiped). Every substitution
    /// brackets one of those bytes for +2; if the implementation ever grows past
    /// the bound, this property fails before the hygiene regresses silently.
    #[test]
    fn defang_output_fits_separator_bound(input in interesting_string(), s in style()) {
        let separators = input
            .bytes()
            .filter(|b| matches!(b, b'.' | b'@' | b':'))
            .count();
        let out = defang(&input, s);
        prop_assert!(
            out.len() <= input.len() + 2 * separators,
            "defang output {} bytes exceeds pre-sized bound {} (input {} bytes)",
            out.len(),
            input.len() + 2 * separators,
            input.len()
        );
    }
}
