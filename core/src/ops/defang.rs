//! Defang / refang text operations.
//!
//! **Implementation owner: indicators stream (B-defang).**
//!
//! "Defanging" rewrites network indicators (URLs, emails, IPs, domains) into a
//! visually-similar but inert form so they can be pasted into chat/tickets/docs
//! without becoming clickable links or being auto-fetched by link unfurlers. The
//! canonical transforms are scheme mangling (`http` → `hxxp`) and bracketing the
//! structural punctuation (`.` → `[.]`, `://` → `[://]`, `@` → `[@]`, `:` → `[:]`).
//! "Refanging" is the inverse — turning a defanged indicator back into the live
//! form — and is style-agnostic (accepts both bracket styles plus `hxxp`).
//!
//! Both functions are pure, deterministic, linear-time, panic-free on any input
//! (including invalid-looking or non-ASCII text), and allocate only the output.
//! They never index into byte offsets in a way that could split a UTF-8 char.

/// Bracket style for defanging — **re-exported from `crate::config`** so the wire
/// schema (`Operation::Defang { style }`, the Swift mirror, the capabilities JSON)
/// and this implementation share a single `BracketStyle` type rather than two
/// look-alikes. `Square` (`[.]`, the default) is the most common; `Round` (`(.)`)
/// is for tools/forums that strip or mangle square brackets. `refang` is
/// style-agnostic and reverses both regardless of which produced the input.
pub use crate::config::BracketStyle;

// Token edges and the URL/email heuristics are shared with the extractors so all
// three agree on what a token (and a URL/email) is — single source of truth.
use crate::ops::lines::{is_email, is_url, trim_token_punct};
use std::borrow::Cow;

// Inherent helpers on the shared type (same crate, so this is allowed): the bracket
// chars for a style. Kept here next to the defang logic that uses them.
impl BracketStyle {
    /// The opening bracket char for this style.
    fn open(self) -> char {
        match self {
            BracketStyle::Square => '[',
            BracketStyle::Round => '(',
        }
    }

    /// The closing bracket char for this style.
    fn close(self) -> char {
        match self {
            BracketStyle::Square => ']',
            BracketStyle::Round => ')',
        }
    }
}

/// Defang the network indicators in `input`, leaving everything else verbatim.
///
/// ## Token model
///
/// The input is treated as a sequence of whitespace-delimited tokens with the
/// whitespace **and** every non-indicator token preserved byte-for-byte. Tokens are
/// found by scanning for maximal runs of non-whitespace (`char::is_whitespace`);
/// the intervening whitespace runs are copied through unchanged, so the exact
/// spacing, newlines, tabs, and CRLF of the input round-trip.
///
/// For each token:
/// 1. A small set of **surrounding punctuation** (`< > ( ) [ ] { } , ; : " '`) is
///    trimmed off both ends. This is the same fixed set as `ops::lines`'s token
///    trimming (kept in sync deliberately). The trimmed prefix and suffix are
///    remembered and re-emitted around the transformed core. Note `.` is **not** in
///    the trim set, so e.g. `"<http://a.b>"` strips the angle brackets and defangs
///    the core (`"<hxxp[://]a[.]b>"`), whereas a trailing `.` stays attached to the
///    core and is bracketed as part of the indicator (`"(http://c.d)."` →
///    `"(hxxp[://]c[.]d)[.]"`): the leading `(` is trimmed but the trailing `).`
///    is not, since trimming stops at the non-trim-set `.`.
/// 2. The **core** is classified (in priority order) as: already-defanged, URL,
///    email, IPv4, IPv6, or bare domain. The first match wins. Anything that matches
///    none is emitted unchanged.
///
/// ## Already-defanged guard (idempotence)
///
/// If the core already contains any defang marker — the substrings `[.]` `[@]`
/// `[://]` `[:]`, the round variants `(.)` `(@)` `(://)` `(:)`, or `hxxp` — the token
/// is emitted unchanged (after re-attaching its trimmed punctuation). This makes
/// `defang(defang(x, s), s) == defang(x, s)` for every input and both styles: a
/// second pass sees the markers and is a no-op.
///
/// ## Substitutions (applied to the core, using `style`'s brackets `B`/`b`)
///
/// * **URL** (`http://`, `https://`, or `www.` prefix, matching the `ops::lines` URL
///   heuristic): a leading lowercase scheme `https://` → `hxxps://` and `http://` →
///   `hxxp://` (case-sensitive — only the lowercase scheme is mangled); then the
///   **first** `://` → `B://b`; then every remaining `.` → `B.b`. A `www.`-prefixed
///   URL has no scheme to mangle, so only its dots are bracketed.
/// * **Email** (one `@`, non-empty local part, domain with an interior `.`, matching
///   the `ops::lines` email heuristic): `@` → `B@b` and every `.` → `B.b`.
/// * **IPv4** (four 1–3 digit decimal octets separated by `.`): every `.` → `B.b`.
/// * **IPv6** (a colon-grouped hex address; see the classifier): every `:` → `B:b`.
///   Only IPv6 brackets colons — a URL port like `:8080` is left intact because URL
///   defanging never touches a bare `:`.
/// * **Bare domain** (a dotted host label sequence with a non-numeric TLD): every
///   `.` → `B.b`.
///
/// The classifiers are deliberately heuristic (this is a clipboard convenience, not
/// a validator); they are conservative enough that ordinary prose words are left
/// untouched. Mixed prose with one indicator defangs only that indicator.
///
/// Empty input yields the empty string. Input with no indicators is returned
/// verbatim.
pub fn defang(input: &str, style: BracketStyle) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 8 + 8);
    // Walk the input as alternating whitespace runs / non-whitespace tokens, copying
    // whitespace verbatim and transforming each token. char_indices keeps us on UTF-8
    // boundaries; we only ever slice on indices that came from char_indices.
    let mut token_start: Option<usize> = None;
    for (i, c) in input.char_indices() {
        if c.is_whitespace() {
            if let Some(start) = token_start.take() {
                out.push_str(&defang_token(&input[start..i], style));
            }
            out.push(c);
        } else if token_start.is_none() {
            token_start = Some(i);
        }
    }
    if let Some(start) = token_start {
        out.push_str(&defang_token(&input[start..], style));
    }
    out
}

/// Defang a single whitespace-free token: trim surrounding punctuation, transform the
/// core, and re-emit `prefix + core' + suffix`.
///
/// Returns `Cow::Borrowed(token)` when the core is not an indicator — `prefix + core
/// + suffix` is byte-identical to the original token, so an unchanged token needs no
/// allocation (matching `clean_urls`'s allocation discipline).
fn defang_token(token: &str, style: BracketStyle) -> Cow<'_, str> {
    let core = trim_token_punct(token);
    match transform_core(core, style) {
        None => Cow::Borrowed(token),
        Some(new_core) => {
            // The trimmed core is a contiguous slice of `token`; recover the
            // surrounding prefix/suffix by byte offset. Both lie on char boundaries
            // because `trim_matches` only ever trims whole chars.
            let core_off = core.as_ptr() as usize - token.as_ptr() as usize;
            let prefix = &token[..core_off];
            let suffix = &token[core_off + core.len()..];
            let mut s = String::with_capacity(prefix.len() + new_core.len() + suffix.len());
            s.push_str(prefix);
            s.push_str(&new_core);
            s.push_str(suffix);
            Cow::Owned(s)
        }
    }
}

/// Classify and transform a token core. Returns `Some(new_core)` if a substitution
/// was made, or `None` if the core is not an indicator (or is already defanged) and
/// should be emitted unchanged.
fn transform_core(core: &str, style: BracketStyle) -> Option<String> {
    if core.is_empty() || already_defanged(core) {
        return None;
    }
    if is_url(core) {
        return Some(defang_url(core, style));
    }
    if is_email(core) {
        return Some(replace_dots_and(core, '@', style));
    }
    if is_ipv4(core) {
        return Some(bracket_char(core, '.', style));
    }
    if is_ipv6(core) {
        return Some(bracket_char(core, ':', style));
    }
    if is_bare_domain(core) {
        return Some(bracket_char(core, '.', style));
    }
    None
}

/// True if `core` already carries any defang marker, for either bracket style, or the
/// `hxxp` scheme mangle. Used as the idempotence guard.
fn already_defanged(core: &str) -> bool {
    const MARKERS: [&str; 8] = ["[.]", "[@]", "[://]", "[:]", "(.)", "(@)", "(://)", "(:)"];
    if core.contains("hxxp") {
        return true;
    }
    MARKERS.iter().any(|m| core.contains(m))
}

/// Defang a URL core: mangle a leading lowercase scheme, bracket the first `://`,
/// then bracket every remaining `.`.
fn defang_url(core: &str, style: BracketStyle) -> String {
    // 1. Scheme mangle (lowercase only — matches the case-sensitive URL heuristic).
    //    Note "https" is handled before "http" is irrelevant here since we match the
    //    full "://"-bearing prefixes; do the longer first regardless for clarity.
    let (scheme_out, rest) = if let Some(r) = core.strip_prefix("https://") {
        ("hxxps".to_string(), Some(("://", r)))
    } else if let Some(r) = core.strip_prefix("http://") {
        ("hxxp".to_string(), Some(("://", r)))
    } else {
        (String::new(), None)
    };

    let (o, cl) = (style.open(), style.close());
    let mut out = String::with_capacity(core.len() + 16);

    match rest {
        Some((_sep, after)) => {
            // We consumed "<scheme>://"; emit mangled scheme + bracketed separator,
            // then the rest with its dots bracketed.
            out.push_str(&scheme_out);
            out.push(o);
            out.push_str("://");
            out.push(cl);
            push_dots_bracketed(&mut out, after, o, cl);
        }
        None => {
            // No scheme (e.g. "www.example.com"): bracket the first "://" if present
            // anywhere, then bracket the remaining dots. In practice www.* has no
            // "://", so this just brackets every dot.
            if let Some(pos) = core.find("://") {
                out.push_str(&core[..pos]);
                out.push(o);
                out.push_str("://");
                out.push(cl);
                push_dots_bracketed(&mut out, &core[pos + 3..], o, cl);
            } else {
                push_dots_bracketed(&mut out, core, o, cl);
            }
        }
    }
    out
}

/// Append `s` to `out`, replacing every `.` with `<o>.<cl>`.
fn push_dots_bracketed(out: &mut String, s: &str, o: char, cl: char) {
    for c in s.chars() {
        if c == '.' {
            out.push(o);
            out.push('.');
            out.push(cl);
        } else {
            out.push(c);
        }
    }
}

/// Bracket every occurrence of `target` in `s` with the style brackets.
fn bracket_char(s: &str, target: char, style: BracketStyle) -> String {
    let (o, cl) = (style.open(), style.close());
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c == target {
            out.push(o);
            out.push(c);
            out.push(cl);
        } else {
            out.push(c);
        }
    }
    out
}

/// Email helper: bracket every `.` and the single `@`.
fn replace_dots_and(s: &str, at: char, style: BracketStyle) -> String {
    let (o, cl) = (style.open(), style.close());
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c == '.' || c == at {
            out.push(o);
            out.push(c);
            out.push(cl);
        } else {
            out.push(c);
        }
    }
    out
}

/// IPv4 classifier: exactly four parts separated by `.`, each a 1–3 digit decimal in
/// 0..=255. Heuristic but strict enough to avoid matching version strings like
/// "1.2.3" (only three parts) or "1.2.3.4.5".
fn is_ipv4(s: &str) -> bool {
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
        // Parse without panicking; all-digit and <=3 chars fits in u16.
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

/// IPv6 classifier (heuristic): a colon-grouped hex address. We accept `s` iff:
/// * it contains at least two `:` **or** one `::` compression (a single `:` is a
///   port separator, never IPv6 — this keeps `example.com:8080` and `host:22` out),
/// * every character is a hex digit or `:` (no dots, no `g`-`z`, no other punct), so
///   IPv4-mapped forms with embedded dots are intentionally *not* recognized here,
/// * every `:`-separated group is at most 4 hex digits, and
/// * there is at most one `::` compression (reject `a::b::c` and `:::`).
fn is_ipv6(s: &str) -> bool {
    let colon_count = s.bytes().filter(|&b| b == b':').count();
    let has_double = s.contains("::");
    if colon_count < 2 && !has_double {
        return false;
    }
    // Only hex digits and ':' allowed.
    if !s.chars().all(|c| c == ':' || c.is_ascii_hexdigit()) {
        return false;
    }
    // Each group is <= 4 hex digits (empty groups come from the allowed "::").
    for group in s.split(':') {
        if group.len() > 4 {
            return false;
        }
    }
    // At most one "::" compression.
    if s.matches("::").count() > 1 {
        return false;
    }
    true
}

/// Bare-domain classifier (heuristic): a dotted host with at least two labels, every
/// label non-empty and made of ASCII alphanumerics or `-` (not starting/ending with
/// `-`), and a final label (TLD) that is all ASCII letters and at least two chars.
/// This deliberately rejects pure-numeric dotted strings (handled by IPv4), version
/// numbers ("1.2"), and prose words (no dot).
fn is_bare_domain(s: &str) -> bool {
    // Split off the final label (TLD) without allocating. No dot at all -> not a
    // domain; `host` non-empty then guarantees the >= 2-label requirement.
    let (host, tld) = match s.rsplit_once('.') {
        Some(parts) => parts,
        None => return false,
    };
    // TLD: all ASCII letters, length >= 2.
    if tld.len() < 2 || !tld.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    if host.is_empty() {
        return false;
    }
    // Every preceding label: non-empty, ASCII alphanumeric or '-', no edge '-'.
    for label in host.split('.') {
        let bytes = label.as_bytes();
        if bytes.is_empty() || bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return false;
        }
        if !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        {
            return false;
        }
    }
    true
}

/// Refang `input`: globally reverse every defang substitution, accepting **both**
/// bracket styles. This is the inverse of [`defang`] for any input that contained no
/// pre-existing defang markers.
///
/// Reversals, applied left-to-right over the whole string (not token-oriented, since
/// markers are unambiguous):
/// * `[://]` and `(://)` → `://`
/// * `[.]` and `(.)` → `.`
/// * `[@]` and `(@)` → `@`
/// * `[:]` and `(:)` → `:`
/// * `hxxp` → `http` (this also restores `hxxps` → `https`, since only the `hxxp`
///   prefix was mangled).
///
/// **Caveat (documented):** refang is a pure marker-substitution and does not know
/// whether a bracketed marker was produced by defanging or was literally present in
/// the source. So `refang(defang(x, s)) == x` holds only for inputs `x` that contain
/// none of the defang markers (`[` `]` `(` `)` sequences forming a marker, or
/// `hxxp`). The round-trip property test constrains its generator accordingly.
pub fn refang(input: &str) -> String {
    // Single linear left-to-right scan with bounded lookahead: at each position, try
    // to match the longest marker; on a match, emit its replacement and advance past
    // it; otherwise copy one char. This avoids the O(n*k) repeated-`replace` chain and
    // any double-rewriting hazards, and never slices off a char boundary because we
    // only advance by whole matched ASCII markers or one char.
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let n = bytes.len();
    while i < n {
        // Ordered longest-first so "[://]" is tried before "[" prefixes etc. All
        // markers are pure ASCII, so byte matching is safe and stays on char
        // boundaries (an ASCII byte can never be a UTF-8 continuation byte).
        if let Some((repl, len)) = match_marker(&bytes[i..]) {
            out.push_str(repl);
            i += len;
        } else {
            // Copy the next whole UTF-8 char. The byte at i is a char boundary because
            // every marker we skip is whole-ASCII; otherwise i advanced by one char.
            let ch_len = utf8_char_len(bytes[i]);
            // Defensive clamp so a truncated/invalid lead byte can never slice past the
            // end (input is valid UTF-8, but stay panic-free regardless).
            let end = (i + ch_len).min(n);
            // Slice the original validated &str on a char boundary (no unsafe).
            out.push_str(&input[i..end]);
            i = end;
        }
    }
    out
}

/// If `b` starts with a defang marker, return its replacement and the marker's byte
/// length. Longest markers are checked first so `[://]` wins over any shorter prefix.
fn match_marker(b: &[u8]) -> Option<(&'static str, usize)> {
    // (marker, replacement). Ordered longest-first within each starting bracket.
    const TABLE: [(&[u8], &str); 9] = [
        (b"[://]", "://"),
        (b"(://)", "://"),
        (b"[.]", "."),
        (b"(.)", "."),
        (b"[@]", "@"),
        (b"(@)", "@"),
        (b"[:]", ":"),
        (b"(:)", ":"),
        (b"hxxp", "http"),
    ];
    for (marker, repl) in TABLE {
        if b.len() >= marker.len() && &b[..marker.len()] == marker {
            return Some((repl, marker.len()));
        }
    }
    None
}

/// UTF-8 length (1..=4) implied by a leading byte. Used only to copy a whole char in
/// `refang`'s fallback path; never panics and never needs the byte to be valid.
fn utf8_char_len(lead: u8) -> usize {
    if lead < 0x80 {
        1
    } else if lead >> 5 == 0b110 {
        2
    } else if lead >> 4 == 0b1110 {
        3
    } else if lead >> 3 == 0b11110 {
        4
    } else {
        // Continuation or invalid lead byte: advance one to make progress.
        1
    }
}
