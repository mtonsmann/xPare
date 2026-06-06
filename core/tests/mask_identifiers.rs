//! Golden + property tests for privacy masking.

use proptest::prelude::*;
use safetystrip_core::{ops::mask::mask_identifiers, transform, Config, Operation};

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
}
