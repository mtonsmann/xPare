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
    for &entry in TRACKING_PARAMS {
        if let Some(stem) = entry.strip_suffix('*') {
            if key.len() >= stem.len()
                && key.as_bytes()[..stem.len()].eq_ignore_ascii_case(stem.as_bytes())
            {
                return true;
            }
        } else if key.eq_ignore_ascii_case(entry) {
            return true;
        }
    }
    false
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
                out.push_str(&clean_token(&input[start..idx]));
            }
            out.push(ch);
        } else if token_start.is_none() {
            token_start = Some(idx);
        }
    }
    if let Some(start) = token_start {
        out.push_str(&clean_token(&input[start..]));
    }
    out
}

/// Clean a single non-whitespace token. Non-URL tokens are returned borrowed-as-is
/// (no allocation); URL tokens are rebuilt with trackers removed.
fn clean_token(token: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = trim_token_punct(token);
    if !is_url(trimmed) {
        return std::borrow::Cow::Borrowed(token);
    }
    // Recover the exact surrounding punctuation we trimmed so we can re-emit it.
    // `trimmed` is a sub-slice of `token`, so subtracting pointer offsets is the
    // byte range; this stays on char boundaries because `trim_matches` only ever
    // trims whole chars.
    let prefix_len = trimmed.as_ptr() as usize - token.as_ptr() as usize;
    let prefix = &token[..prefix_len];
    let suffix = &token[prefix_len + trimmed.len()..];

    let cleaned_core = clean_url_core(trimmed);
    // If nothing changed and there was no surrounding punctuation, borrow the input.
    if prefix.is_empty() && suffix.is_empty() && cleaned_core == trimmed {
        return std::borrow::Cow::Borrowed(token);
    }
    let mut s = String::with_capacity(prefix.len() + cleaned_core.len() + suffix.len());
    s.push_str(prefix);
    s.push_str(&cleaned_core);
    s.push_str(suffix);
    std::borrow::Cow::Owned(s)
}

/// Clean the URL "core" (punctuation already trimmed): split base/query/fragment,
/// drop tracker pairs, reassemble.
fn clean_url_core(core: &str) -> String {
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

    let mut out = String::with_capacity(core.len());
    out.push_str(base);

    if let Some(query) = query {
        // Keep surviving pairs in order, exact spelling.
        let mut survivors: Vec<&str> = Vec::new();
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
                survivors.push(pair);
            }
        }
        if !survivors.is_empty() {
            out.push('?');
            for (i, pair) in survivors.iter().enumerate() {
                if i > 0 {
                    out.push('&');
                }
                out.push_str(pair);
            }
        }
    }

    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }

    out
}
