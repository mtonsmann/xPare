//! Shared token and indicator heuristics.
//!
//! These helpers intentionally stay small and heuristic. xPare is a clipboard
//! utility, not an RFC-grade email/IP/URL validator, and several operations need to
//! agree on what a token edge or simple indicator is. Keeping the classifiers here
//! prevents extractors, defang, URL cleaning, and privacy masking from drifting.

/// Trim a small, fixed set of surrounding punctuation/brackets/quotes from a token.
/// Operates on `char` boundaries via `trim_matches`, so it is panic-free.
pub(crate) fn trim_token_punct(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '"' | '\''
        )
    })
}

/// Email heuristic used by extraction, defang, and masking.
///
/// A token is an email iff it contains exactly one `@`, has a non-empty local part,
/// and has a domain with an interior `.`. This is deliberately not RFC 5322.
pub(crate) fn is_email(token: &str) -> bool {
    let mut parts = token.split('@');
    let local = match parts.next() {
        Some(l) => l,
        None => return false,
    };
    let domain = match parts.next() {
        Some(d) => d,
        None => return false,
    };
    if parts.next().is_some() {
        return false;
    }
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    match domain.find('.') {
        Some(dot) => dot > 0 && dot < domain.len() - 1,
        None => false,
    }
}

/// URL heuristic used by extraction, defang, and URL cleaning.
///
/// A token is a URL iff it starts with `http://`, `https://`, or `www.` and has at
/// least one character after that prefix. Prefix matching is case-sensitive.
pub(crate) fn is_url(token: &str) -> bool {
    for prefix in ["http://", "https://", "www."] {
        if let Some(rest) = token.strip_prefix(prefix) {
            if !rest.is_empty() {
                return true;
            }
        }
    }
    false
}

/// IPv4 classifier: exactly four parts separated by `.`, each a 1-3 digit decimal
/// in 0..=255. Heuristic but strict enough to avoid version strings like `1.2.3`.
pub(crate) fn is_ipv4(s: &str) -> bool {
    let mut count = 0usize;
    for part in s.split('.') {
        count += 1;
        if count > 4 {
            return false;
        }
        let bytes = part.as_bytes();
        if bytes.is_empty() || bytes.len() > 3 {
            return false;
        }
        if !bytes.iter().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let mut val: u16 = 0;
        for &b in bytes {
            val = val * 10 + u16::from(b - b'0');
        }
        if val > 255 {
            return false;
        }
    }
    count == 4
}

/// IPv6 classifier (heuristic): a colon-grouped hex address.
///
/// We accept `s` iff it contains at least two `:` or one `::` compression, every
/// character is a hex digit or `:`, every group is at most 4 hex digits, and there
/// is at most one `::` compression.
pub(crate) fn is_ipv6(s: &str) -> bool {
    let colon_count = s.bytes().filter(|&b| b == b':').count();
    let has_double = s.contains("::");
    if colon_count < 2 && !has_double {
        return false;
    }
    if !s.chars().all(|c| c == ':' || c.is_ascii_hexdigit()) {
        return false;
    }
    for group in s.split(':') {
        if group.len() > 4 {
            return false;
        }
    }
    if s.matches("::").count() > 1 {
        return false;
    }
    true
}
