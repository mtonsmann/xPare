//! Golden + property tests for privacy masking.

use proptest::prelude::*;
use xpare_core::{ops::mask::mask_identifiers, transform, Config, Operation};

fn mask_all(input: &str) -> String {
    mask_identifiers(input, true, true, true)
}

#[test]
fn masks_email_tokens() {
    assert_eq!(
        mask_identifiers("Email me@a.test", true, false, false),
        "Email [email]"
    );
    assert_eq!(
        mask_identifiers("<me@a.test>, cc other@b.example", true, false, false),
        "<[email]>, cc [email]"
    );
}

#[test]
fn masks_ipv4_tokens() {
    assert_eq!(
        mask_identifiers("src=10.0.0.1", false, true, false),
        "src=10.0.0.1"
    );
    assert_eq!(mask_identifiers("10.0.0.1", false, true, false), "[ipv4]");
    assert_eq!(
        mask_identifiers("<192.168.1.5>,", false, true, false),
        "<[ipv4]>,"
    );
}

// Indicator-classifier boundary coverage (is_email / is_ipv4 / is_ipv6 mutation
// survivors). Each pins a comparison that otherwise flips silently.
#[test]
fn masks_indicator_classifier_boundaries() {
    // is_email: an empty local part ("@a.test") is rejected -> left verbatim. (L36 ||->&&)
    assert_eq!(mask_identifiers("@a.test", true, false, false), "@a.test");
    // is_email: a leading-dot domain ("a@.com", dot at index 0) is rejected. (L40 &&->||, >->>=)
    assert_eq!(mask_identifiers("a@.com", true, false, false), "a@.com");
    // is_ipv4: an octet of exactly 255 is valid (the bound is `> 255`, not `>= 255`). (L80 >->>=)
    assert_eq!(
        mask_identifiers("255.255.255.255", false, true, false),
        "[ipv4]"
    );
    // is_ipv6: a single colon is not enough to be IPv6 -> left verbatim. (L93 ==->!=)
    assert_eq!(mask_identifiers("1:2", false, false, true), "1:2");
    // is_ipv6: two colons with no "::" compression IS a valid IPv6. (L95 &&->||, <->==)
    assert_eq!(mask_identifiers("1:2:3", false, false, true), "[ipv6]");
}

#[test]
fn masks_ipv6_tokens() {
    assert_eq!(
        mask_identifiers("2001:db8::1", false, false, true),
        "[ipv6]"
    );
    assert_eq!(
        mask_identifiers("[2001:db8::1]", false, false, true),
        "[[ipv6]]"
    );
    assert_eq!(
        mask_identifiers("host:22 ::1 a::b::c", false, false, true),
        "host:22 ::1 a::b::c"
    );
}

#[test]
fn target_flags_are_independent() {
    let input = "me@a.test 10.0.0.1 2001:db8::1";
    assert_eq!(
        mask_identifiers(input, true, false, false),
        "[email] 10.0.0.1 2001:db8::1"
    );
    assert_eq!(
        mask_identifiers(input, false, true, false),
        "me@a.test [ipv4] 2001:db8::1"
    );
    assert_eq!(
        mask_identifiers(input, false, false, true),
        "me@a.test 10.0.0.1 [ipv6]"
    );
}

#[test]
fn no_targets_is_noop() {
    let input = "me@a.test 10.0.0.1 2001:db8::1";
    assert_eq!(mask_identifiers(input, false, false, false), input);
    assert_eq!(
        transform(
            input,
            &Config::as_given(vec![Operation::MaskIdentifiers {
                emails: false,
                ipv4: false,
                ipv6: false,
            }])
        ),
        input
    );
}

#[test]
fn preserves_whitespace_and_non_matches() {
    let input = "a\tme@a.test  \nnot-an-email 1.2.3 999.0.0.1\r\n";
    assert_eq!(
        mask_all(input),
        "a\t[email]  \nnot-an-email 1.2.3 999.0.0.1\r\n"
    );
}

#[test]
fn already_masked_placeholders_are_unchanged() {
    let input = "[email] [ipv4] [ipv6]";
    assert_eq!(mask_all(input), input);
}

#[test]
fn shortest_growing_cores_mask_correctly() {
    // The output buffer is pre-sized to `input.len() + 2*#'@' + #':'` so the
    // clipboard-derived accumulator never reallocates. These are the shortest
    // classifiable cores — the worst growth cases that bound rests on: email
    // `a@b.c` (5 bytes, one '@') -> `[email]` (7 bytes), and IPv6 `a::b` (4
    // bytes, two ':') -> `[ipv6]` (6 bytes).
    assert_eq!(mask_identifiers("a@b.c", true, false, false), "[email]");
    assert_eq!(mask_identifiers("a::b", false, false, true), "[ipv6]");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn deterministic_and_panic_free(input in ".*", emails in any::<bool>(), ipv4 in any::<bool>(), ipv6 in any::<bool>()) {
        let a = mask_identifiers(&input, emails, ipv4, ipv6);
        let b = mask_identifiers(&input, emails, ipv4, ipv6);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn idempotent(input in ".*", emails in any::<bool>(), ipv4 in any::<bool>(), ipv6 in any::<bool>()) {
        let once = mask_identifiers(&input, emails, ipv4, ipv6);
        let twice = mask_identifiers(&once, emails, ipv4, ipv6);
        prop_assert_eq!(once, twice);
    }

    /// Capacity-bound soundness: `mask_identifiers` pre-sizes its output to
    /// `input.len() + 2*#'@' + #':'` so the clipboard-derived buffer never
    /// reallocates mid-build (a reallocation frees the old block unwiped). Each
    /// growing placeholder is paid for by the `@`/`:` bytes of the core it
    /// replaces; if masking ever outgrows the bound, this fails before the
    /// hygiene regresses silently.
    #[test]
    fn mask_output_fits_byte_count_bound(input in ".*", emails in any::<bool>(), ipv4 in any::<bool>(), ipv6 in any::<bool>()) {
        let at_signs = input.bytes().filter(|&b| b == b'@').count();
        let colons = input.bytes().filter(|&b| b == b':').count();
        let out = mask_identifiers(&input, emails, ipv4, ipv6);
        prop_assert!(
            out.len() <= input.len() + 2 * at_signs + colons,
            "mask output {} bytes exceeds pre-sized bound {} (input {} bytes)",
            out.len(),
            input.len() + 2 * at_signs + colons,
            input.len()
        );
    }
}
