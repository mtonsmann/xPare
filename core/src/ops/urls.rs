//! URL tracking-parameter removal.
//!
//! [`clean_urls`] strips well-known tracking query parameters (UTM tags, ad-click
//! ids, etc.) from URL-like tokens while leaving everything else byte-for-byte
//! untouched. It is part of the core's untrusted-input path, so it is
//! `#![forbid(unsafe_code)]`-clean, panic-free on every input, deterministic, and
//! runs in linear time. No dependencies: all parsing is hand-rolled.
//!
//! ## Frozen rule (the contract)
//!
//! * **Token-oriented.** Split `input` on whitespace (`char::is_whitespace`) and
//!   preserve *all* whitespace and *all* non-URL tokens verbatim. Each token is
//!   re-emitted in place; the whitespace between tokens is reproduced exactly.
//! * A token is a **candidate** iff, after trimming a fixed set of surrounding
//!   punctuation (`< > ( ) [ ] { } , ; : " '`), it begins with `http://`,
//!   `https://`, or `www.` (case-sensitive prefix, matching the rest of the core's
//!   URL heuristic) and has at least one more char after the prefix. The trimmed
//!   prefix/suffix punctuation is re-emitted unchanged around the cleaned core.
//! * The candidate core is split into `base`, `?query`, and `#fragment`. Per the
//!   URL grammar, the **first** `?` starts the query and the **first** `#` *after*
//!   that starts the fragment. A `#` before any `?` starts the fragment directly
//!   (there is then no query). Any later `?`/`#` are ordinary characters inside the
//!   query/fragment and are preserved.
//! * The query is parsed as `&`-separated pairs. Each pair is `key` or `key=value`
//!   (the value, if present, keeps its exact spelling including percent-encoding and
//!   any further `=`). A pair is **dropped** iff its key matches [`TRACKING_PARAMS`]
//!   case-insensitively; an entry ending in `*` matches by prefix (e.g. `utm_*`
//!   matches `utm_source`, `UTM_Campaign`). All other pairs are **kept** in original
//!   order and exact spelling. Empty pairs (from `&&` or a leading/trailing `&`) are
//!   dropped.
//! * Reassemble: `base` + (`?` + surviving query, only if ≥1 pair survived) +
//!   (`#` + fragment, iff the original had a `#`). If no pair survives, the `?` is
//!   dropped entirely. A fragment is preserved verbatim even when empty
//!   (`...#` round-trips).
//! * **Idempotent:** `clean_urls(clean_urls(x)) == clean_urls(x)`. Percent-encoding
//!   of surviving values is never altered.

// Token edges and the URL heuristic are shared with the extractors and defang so all
// agree on what a token (and a URL) is — single source of truth.
use crate::ops::lines::{is_url, trim_token_punct};

/// Query-parameter keys treated as trackers and removed. Matching is
/// case-insensitive; an entry ending in `*` matches by prefix.
///
/// Deliberately conservative: only well-known marketing/analytics/click-id params.
/// Generic keys like `ref`, `q`, `id`, `page`, `s`, `lang` are intentionally absent
/// so legitimate query state is never destroyed.
pub const TRACKING_PARAMS: &[&str] = &[
    // Google / Urchin analytics + the broader UTM convention (prefix match covers
    // utm_source, utm_medium, utm_campaign, utm_term, utm_content, utm_id, …).
    "utm_*",
    // Oracle/Eloqua (Oracle Marketing) tracking, prefix form.
    "oly_*",
    // Mailchimp campaign + member ids.
    "mc_cid",
    "mc_eid",
    // HubSpot.
    "_hsenc",
    "_hsmi",
    "hsctatracking",
    // Click identifiers from the major ad networks.
    "gclid",   // Google Ads
    "gclsrc",  // Google Ads click source
    "dclid",   // Google Display/DoubleClick
    "wbraid",  // Google web-to-app
    "gbraid",  // Google app-to-web
    "fbclid",  // Meta / Facebook
    "msclkid", // Microsoft Advertising / Bing
    "yclid",   // Yandex
    "twclid",  // Twitter/X
    "ttclid",  // TikTok
    "igshid",  // Instagram share id
    "mkt_tok", // Marketo
    "vero_id",
    "vero_conv",
    "oicd",
    "icid",
    "s_kwcid",   // Adobe / SEM keyword campaign id
    "ef_id",     // Adobe Advertising
    "_openstat", // Yandex / LiveInternet openstat tracker
    "wickedid",  // Wicked Reports attribution
];

/// True if `key` matches any [`TRACKING_PARAMS`] entry, case-insensitively. An entry
/// ending in `*` matches by prefix; otherwise it must match in full.
///
/// Case folding uses ASCII-only lowercasing: every tracker entry is ASCII, and a
/// non-ASCII key can never equal (or ASCII-prefix-match) an ASCII tracker, so ASCII
/// folding is exact here and avoids allocating a Unicode-folded copy of attacker
/// input.
fn is_tracker_key(key: &str) -> bool {
    let Some(first) = key.as_bytes().first().map(u8::to_ascii_lowercase) else {
        return false;
    };
    match first {
        b'_' => {
            key.eq_ignore_ascii_case("_hsenc")
                || key.eq_ignore_ascii_case("_hsmi")
                || key.eq_ignore_ascii_case("_openstat")
        }
        b'd' => key.eq_ignore_ascii_case("dclid"),
        b'e' => key.eq_ignore_ascii_case("ef_id"),
        b'f' => key.eq_ignore_ascii_case("fbclid"),
        b'g' => {
            key.eq_ignore_ascii_case("gclid")
                || key.eq_ignore_ascii_case("gclsrc")
                || key.eq_ignore_ascii_case("gbraid")
        }
        b'h' => key.eq_ignore_ascii_case("hsctatracking"),
        b'i' => key.eq_ignore_ascii_case("igshid") || key.eq_ignore_ascii_case("icid"),
        b'm' => {
            key.eq_ignore_ascii_case("mc_cid")
                || key.eq_ignore_ascii_case("mc_eid")
                || key.eq_ignore_ascii_case("msclkid")
                || key.eq_ignore_ascii_case("mkt_tok")
        }
        b'o' => key_has_tracker_prefix(key, "oly_") || key.eq_ignore_ascii_case("oicd"),
        b's' => key.eq_ignore_ascii_case("s_kwcid"),
        b't' => key.eq_ignore_ascii_case("twclid") || key.eq_ignore_ascii_case("ttclid"),
        b'u' => key_has_tracker_prefix(key, "utm_"),
        b'v' => key.eq_ignore_ascii_case("vero_id") || key.eq_ignore_ascii_case("vero_conv"),
        b'w' => key.eq_ignore_ascii_case("wbraid") || key.eq_ignore_ascii_case("wickedid"),
        b'y' => key.eq_ignore_ascii_case("yclid"),
        _ => false,
    }
}

fn key_has_tracker_prefix(key: &str, prefix: &str) -> bool {
    key.len() >= prefix.len()
        && key.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Clean tracking parameters from URL tokens in `input`. See the module-level frozen
/// rule for the exact contract.
///
/// The walk is a single linear pass over the bytes: we alternate between runs of
/// whitespace (copied verbatim) and non-whitespace tokens (cleaned if they are URL
/// candidates, else copied verbatim). All slicing happens on whitespace byte
/// boundaries (ASCII or the leading byte of a multi-byte char can never be
/// whitespace mid-codepoint, since `char::is_whitespace` is evaluated per char), so
/// no slice ever lands inside a UTF-8 sequence.
pub fn clean_urls(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    // Iterate char-by-char to find whitespace/non-whitespace boundaries without ever
    // slicing inside a codepoint.
    let mut token_start: Option<usize> = None;
    for (idx, ch) in input.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                push_clean_token(&mut out, &input[start..idx]);
            }
            out.push(ch);
        } else if token_start.is_none() {
            token_start = Some(idx);
        }
    }
    if let Some(start) = token_start {
        push_clean_token(&mut out, &input[start..]);
    }
    out
}

/// Append a single non-whitespace token, cleaning URL candidates in place.
fn push_clean_token(out: &mut String, token: &str) {
    if !can_trim_to_url_prefix(token) {
        out.push_str(token);
        return;
    }
    let trimmed = trim_token_punct(token);
    if !is_url(trimmed) {
        out.push_str(token);
        return;
    }
    // Recover the exact surrounding punctuation we trimmed so we can re-emit it.
    // `trimmed` is a sub-slice of `token`, so subtracting pointer offsets is the
    // byte range; this stays on char boundaries because `trim_matches` only ever
    // trims whole chars.
    let prefix_len = trimmed.as_ptr() as usize - token.as_ptr() as usize;
    let prefix = &token[..prefix_len];
    let suffix = &token[prefix_len + trimmed.len()..];

    out.push_str(prefix);
    push_clean_url_core(out, trimmed);
    out.push_str(suffix);
}

/// A URL candidate must start with lowercase `h` or `w` after trimming the fixed
/// ASCII edge punctuation set. Tokens that fail this necessary condition are exact
/// no-ops, so avoid the heavier trim/prefix work.
fn can_trim_to_url_prefix(token: &str) -> bool {
    let mut idx = 0usize;
    let bytes = token.as_bytes();
    while idx < bytes.len() && is_token_trim_byte(bytes[idx]) {
        idx += 1;
    }
    matches!(bytes.get(idx), Some(b'h' | b'w'))
}

fn is_token_trim_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'<' | b'>' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b':' | b'"' | b'\''
    )
}

/// Clean the URL "core" (punctuation already trimmed): split base/query/fragment,
/// drop tracker pairs, reassemble.
fn push_clean_url_core(out: &mut String, core: &str) {
    // Locate the first '?' and the first '#'. Per the URL grammar, a '#' before any
    // '?' means there is no query (the '?' would live inside the fragment).
    let hash = core.find('#');
    let query_region_end = hash.unwrap_or(core.len());
    let question = core[..query_region_end].find('?');

    let (base, query, fragment) = match (question, hash) {
        (Some(q), Some(h)) => (&core[..q], Some(&core[q + 1..h]), Some(&core[h + 1..])),
        (Some(q), None) => (&core[..q], Some(&core[q + 1..]), None),
        (None, Some(h)) => (&core[..h], None, Some(&core[h + 1..])),
        (None, None) => (core, None, None),
    };

    out.push_str(base);

    if let Some(query) = query {
        // Keep surviving pairs in order, exact spelling.
        let mut wrote_pair = false;
        for pair in query.split('&') {
            if pair.is_empty() {
                // Drop empty pairs from '&&' or leading/trailing '&'.
                continue;
            }
            // Key is everything before the first '='; the value (if any) keeps every
            // later '=' verbatim.
            let key = match pair.find('=') {
                Some(eq) => &pair[..eq],
                None => pair,
            };
            if !is_tracker_key(key) {
                if wrote_pair {
                    out.push('&');
                } else {
                    out.push('?');
                    wrote_pair = true;
                }
                out.push_str(pair);
            }
        }
    }

    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
}
