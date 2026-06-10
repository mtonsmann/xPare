//! Privacy masking for selected identifiers.
//!
//! [`mask_identifiers`] replaces selected email/IP tokens with fixed placeholders.
//! This is a local clipboard rewrite, not full anonymization or DLP: it uses the
//! same whitespace-token model and heuristic classifiers as the extractor/IOC ops.

use crate::ops::indicators::{is_email, is_ipv4, is_ipv6, trim_token_punct};

/// Replace selected identifier tokens with fixed placeholders.
///
/// ## Frozen rule (the contract)
///
/// * **Token-oriented.** Split `input` on whitespace (`char::is_whitespace`) and
///   preserve all whitespace and every non-matching token verbatim.
/// * For each token, trim the shared fixed surrounding punctuation set
///   (`< > ( ) [ ] { } , ; : " '`) before classifying. The trimmed prefix/suffix are
///   re-emitted around any placeholder.
/// * If `emails` is true, email-like tokens become `[email]`.
/// * If `ipv4` is true, standalone IPv4 tokens become `[ipv4]`.
/// * If `ipv6` is true, standalone IPv6 tokens become `[ipv6]`.
/// * Classification priority is email, IPv4, IPv6. Anything else is emitted
///   unchanged. If every target flag is false, the transform is an exact no-op.
/// * Placeholders are fixed, deterministic, and idempotent: applying the same mask
///   twice produces the same output.
///
/// The classifiers are deliberately heuristic. This op masks common clipboard/log
/// shapes; it does not promise comprehensive PII detection.
pub fn mask_identifiers(input: &str, emails: bool, ipv4: bool, ipv6: bool) -> String {
    if !(emails || ipv4 || ipv6) {
        return input.to_string();
    }

    // Provably sufficient output capacity, so the clipboard-derived accumulator
    // never reallocates (a reallocation frees the old block unwiped). Only two
    // placeholder classes can be longer than the core they replace:
    // * email — `[email]` (7 bytes) over a core of >= 5 bytes (`a@b.c` is the
    //   shortest the classifier accepts) containing exactly one `@`: growth <= 2
    //   per `@` byte;
    // * IPv6 — `[ipv6]` (6 bytes) over a core of >= 4 bytes (`a::b`) containing
    //   >= 2 `:` bytes: growth <= 2 <= 1 per `:` byte.
    // IPv4 cores are >= 7 bytes (`1.1.1.1`) and shrink to `[ipv4]` (6 bytes).
    // The `mask_output_fits_byte_count_bound` property test pins this bound.
    let at_signs = input.bytes().filter(|&b| b == b'@').count();
    let colons = input.bytes().filter(|&b| b == b':').count();
    let mut out = String::with_capacity(input.len() + 2 * at_signs + colons);
    let mut token_start: Option<usize> = None;
    for (i, c) in input.char_indices() {
        if c.is_whitespace() {
            if let Some(start) = token_start.take() {
                push_masked_token(&mut out, &input[start..i], emails, ipv4, ipv6);
            }
            out.push(c);
        } else if token_start.is_none() {
            token_start = Some(i);
        }
    }
    if let Some(start) = token_start {
        push_masked_token(&mut out, &input[start..], emails, ipv4, ipv6);
    }
    out
}

fn push_masked_token(out: &mut String, token: &str, emails: bool, ipv4: bool, ipv6: bool) {
    let core = trim_token_punct(token);
    let Some(placeholder) = placeholder_for(core, emails, ipv4, ipv6) else {
        out.push_str(token);
        return;
    };

    let core_off = core.as_ptr() as usize - token.as_ptr() as usize;
    out.push_str(&token[..core_off]);
    out.push_str(placeholder);
    out.push_str(&token[core_off + core.len()..]);
}

fn placeholder_for(core: &str, emails: bool, ipv4: bool, ipv6: bool) -> Option<&'static str> {
    if core.is_empty() || !has_relevant_byte(core, emails, ipv4, ipv6) {
        return None;
    }
    if emails && is_email(core) {
        return Some("[email]");
    }
    if ipv4 && is_ipv4(core) {
        return Some("[ipv4]");
    }
    if ipv6 && is_ipv6(core) {
        return Some("[ipv6]");
    }
    None
}

fn has_relevant_byte(core: &str, emails: bool, ipv4: bool, ipv6: bool) -> bool {
    core.as_bytes()
        .iter()
        .any(|&b| (emails && b == b'@') || (ipv4 && b == b'.') || (ipv6 && b == b':'))
}
