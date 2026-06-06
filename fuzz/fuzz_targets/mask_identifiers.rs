#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use safetystrip_core::ops::mask::mask_identifiers;

#[derive(Arbitrary, Debug)]
struct MaskInput {
    emails: bool,
    ipv4: bool,
    ipv6: bool,
    input: Vec<u8>,
}

fuzz_target!(|case: MaskInput| {
    let text = String::from_utf8_lossy(&case.input);
    let _ = mask_identifiers(&text, case.emails, case.ipv4, case.ipv6);
});
