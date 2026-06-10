//! HTML → plain text extraction.
//!
//! **Implementation owner: strippers stream (A1).** This is the shared rich→plain
//! workhorse: the shell hands the core an HTML string and the core extracts text.
//! It is hand-rolled, pure-safe-Rust on purpose (no opaque upstream HTML parser),
//! so memory-unsafety is impossible and the only residual risks — panics and hangs
//! on adversarial input — are pinned down by the corpus, property tests, and fuzzer.
//!
//! # Memory-safety / robustness contract
//!
//! * No `unsafe` (the crate forbids it).
//! * Never panics on any input. The scanner iterates by **`char` and char-aligned
//!   byte offsets only** (via [`str::char_indices`]); it never slices a `&str` at a
//!   byte offset that could land off a UTF-8 boundary, and never `[]`-indexes a
//!   collection by an input-derived value without a checked lookup.
//! * **Linear time, O(n).** Every byte of the input is consumed by exactly one
//!   forward pass. The only lookahead (entity decoding, raw-text close-tag search)
//!   is *bounded*: entity scans stop after a small fixed budget, and the raw-text
//!   close-tag search is a single forward `find` that never rescans.
//!
//! # Documented stripping rules
//!
//! ## Tags & structure
//! A `<` that begins a recognizable construct opens a state:
//! * `<!-- ... -->` — an HTML comment; dropped entirely.
//! * `<!doctype ...>` / other `<! ... >` declarations — dropped.
//! * `<? ... ?>` (also `<? ... >`) — processing instructions; dropped.
//! * `<tag ...>` / `</tag>` — a tag; the markup is dropped, attribute values in
//!   single or double quotes are respected so a `>` *inside* a quoted attribute
//!   does **not** close the tag (`<a title="a>b">x</a>` → `x`).
//!
//! A `<` **not** followed by something tag-like (a letter, `/`, `!`, or `?`) is a
//! stray `<` and is emitted literally, browser-style. A `>` outside any tag is also
//! emitted literally.
//!
//! ## Raw-text elements
//! `<script>…</script>` and `<style>…</style>` have their entire contents dropped.
//! The matching close tag is found case-insensitively (`</ScRiPt>` closes
//! `<script>`); if there is no close tag the rest of the input is dropped (mirrors a
//! browser treating it as unterminated raw text).
//!
//! ## Whitespace / newlines
//! Elements are partitioned into a curated **block** set and everything else
//! (**inline**). A block element emits a `\n` at **both** its start and end tag
//! boundary; `<br>` and `<hr>` emit a single `\n`. Inline elements emit nothing.
//! Runs of emitted/again-encountered newlines are collapsed so the output is never
//! littered with blank lines: **at most one blank line** (i.e. `\n\n`) survives, so
//! paragraph separation is preserved without runaway vertical whitespace. Leading
//! and trailing whitespace of the whole document is trimmed.
//!
//! Block set (case-insensitive): `address article aside blockquote br dd details
//! div dl dt fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 header hgroup
//! hr li main nav ol p pre section summary table tbody td tfoot th thead tr ul`.
//! Everything else (`a b i span em strong code small u s sub sup mark abbr cite
//! …`) is inline and contributes no whitespace of its own.
//!
//! ## Entities
//! * Numeric: `&#DDD;` (decimal) and `&#xHH;` / `&#XHH;` (hex) decode to the
//!   Unicode scalar value. An out-of-range value (`> 0x10FFFF`) or a surrogate
//!   (`0xD800..=0xDFFF`) decodes to **U+FFFD** (the replacement character) rather
//!   than being rejected, so adversarial numeric escapes can never panic and never
//!   leak the raw digits.
//! * Named: a curated table (see `named_entity`) covering the common set. An
//!   unknown name is left **verbatim** (the literal `&name;` text is emitted).
//! * Malformed entities (`&`, `&;`, `&#;`, `&#x;`, `&#xZZ;`, an unterminated
//!   `&amp` with no `;`) are emitted **literally**, never panicking. Decoding is
//!   bounded: a numeric entity accumulates at most a small fixed number of
//!   *significant* digits (leading zeros are skipped) and a named entity reads at
//!   most a small fixed number of name characters before giving up and emitting the
//!   `&` literally.

/// Maximum number of *significant* digits accumulated for a numeric character
/// reference (`0x10FFFF` is 7 hex / 7 decimal digits, so 8 is generous). A value
/// that exceeds U+10FFFF — or names a surrogate — decodes to U+FFFD. Leading zeros
/// are skipped (consumed but not counted), so a zero-padded but in-range reference
/// such as `&#000000065;` still decodes correctly.
const MAX_NUMERIC_DIGITS: usize = 8;

/// Maximum length of a named-entity *name* (between `&` and `;`) we will attempt to
/// match. The longest entity we recognize is well under this; the bound keeps a
/// stray `&` followed by a huge run of letters from being scanned superlinearly.
const MAX_ENTITY_NAME_LEN: usize = 12;

/// Strip HTML tags and decode common entities, producing plain text.
///
/// See the module documentation for the exact, frozen stripping rules. Pure,
/// deterministic, panic-free, and linear in the length of `input`.
pub fn strip_html(input: &str) -> String {
    let bytes = input.as_bytes();
    if !matches!(bytes.first(), Some(b'<' | b'&'))
        && !bytes.contains(&b'<')
        && !bytes.contains(&b'&')
    {
        return strip_html_plain_text(input);
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            '<' => {
                // Decide what kind of construct (if any) this `<` opens by peeking
                // at the next char. `char_indices` keeps us byte-boundary-safe.
                match chars.peek().map(|&(_, nc)| nc) {
                    Some('!') => {
                        // Comment `<!-- ... -->` or other `<! ... >` declaration.
                        if input[i..].starts_with("<!--") {
                            skip_comment(input, &mut chars);
                        } else {
                            skip_to_gt_no_quotes(&mut chars);
                        }
                    }
                    Some('?') => {
                        // Processing instruction `<? ... ?>` (tolerate `... >`).
                        skip_to_gt_no_quotes(&mut chars);
                    }
                    Some('/') => {
                        // End tag `</name>`.
                        let name = read_tag_name_after_slash(input, &mut chars);
                        skip_tag_rest(&mut chars);
                        if is_block_tag(name) {
                            push_newline(&mut out);
                        }
                    }
                    Some(nc) if nc.is_ascii_alphabetic() => {
                        // Start tag `<name ...>`.
                        let name = read_tag_name(input, &mut chars);
                        if eq_ignore_ascii_case(name, "script")
                            || eq_ignore_ascii_case(name, "style")
                        {
                            // Raw-text element: finish this start tag, then drop
                            // everything up to and including the close tag.
                            let self_closed = skip_tag_rest_detect_self_close(&mut chars);
                            if !self_closed {
                                skip_raw_text_to_close(input, bytes, &mut chars, name);
                            }
                        } else {
                            skip_tag_rest(&mut chars);
                            if is_void_newline_tag(name) || is_block_tag(name) {
                                push_newline(&mut out);
                            }
                        }
                    }
                    _ => {
                        // Stray `<` (e.g. `<<<`, `< `, `<3`, end of input): literal.
                        out.push('<');
                    }
                }
            }
            '&' => {
                decode_entity_at(input, &mut chars, &mut out);
            }
            // A literal newline in a text node is structural whitespace: route it
            // through the same collapser as tag-emitted breaks, so the "at most one
            // blank line" guarantee holds for source newlines too — not just tags.
            '\n' => push_newline(&mut out),
            _ => out.push(c),
        }
    }

    normalize_trailing(out)
}

/// Decode the same curated HTML entity set used by [`strip_html`] without treating
/// tags or structural newlines specially.
///
/// This is shared by text-preserving converters that still need xPare's
/// bounded, panic-free entity behavior. Unknown or malformed entities are emitted
/// literally, matching the stripper contract.
pub(crate) fn decode_entities(input: &str) -> String {
    if !input.as_bytes().contains(&b'&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '&' {
            decode_entity_at(input, &mut chars, &mut out);
        } else {
            out.push(c);
        }
    }
    out
}

#[inline(never)]
fn strip_html_plain_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut start = 0usize;
    for (i, &byte) in input.as_bytes().iter().enumerate() {
        if byte == b'\n' {
            out.push_str(&input[start..i]);
            push_newline(&mut out);
            start = i + 1;
        }
    }
    out.push_str(&input[start..]);
    normalize_trailing(out)
}

/// A char-indices iterator we can advance and peek without re-borrowing the string.
type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

/// How far back [`push_newline`] looks past spaces/tabs to find existing newlines.
/// Bounding this keeps the scan O(1) per call: without a cap, input like
/// `"   …   <br><br>…"` (a long whitespace run followed by many block breaks) would
/// re-scan the whole run on every break — overall O(n²), a denial-of-service vector
/// on adversarial input. A small window covers any realistic gap between a newline
/// and the buffer end; past it we treat the buffer as not newline-terminated and
/// emit one (the safe direction — at worst an extra newline a later op can collapse).
const NEWLINE_LOOKBACK: usize = 64;

/// Push a `\n`, collapsing runs so at most one blank line (`"\n\n"`) ever forms.
fn push_newline(out: &mut String) {
    // Count trailing newlines, skipping intervening spaces/tabs (spaces before a
    // forced break are noise), but BOUNDED so a pathological whitespace run cannot
    // make this scan super-linear. Stop early once we have the two we cap at.
    let mut newlines = 0usize;
    for &b in out.as_bytes().iter().rev().take(NEWLINE_LOOKBACK) {
        match b {
            b'\n' => {
                newlines += 1;
                if newlines >= 2 {
                    break;
                }
            }
            b' ' | b'\t' => {}
            _ => break,
        }
    }
    if newlines < 2 {
        out.push('\n');
    }
}

/// Trim leading/trailing ASCII structural whitespace of the whole document in place.
fn normalize_trailing(mut s: String) -> String {
    let (start, end) = {
        let bytes = s.as_bytes();
        let start = match bytes.iter().position(|&b| !is_edge_trim_byte(b)) {
            Some(pos) => pos,
            None => {
                s.clear();
                return s;
            }
        };
        let end = match bytes.iter().rposition(|&b| !is_edge_trim_byte(b)) {
            Some(pos) => pos + 1,
            None => {
                s.clear();
                return s;
            }
        };
        (start, end)
    };
    if end < s.len() {
        s.truncate(end);
    }
    if start > 0 {
        s.drain(..start);
    }
    s
}

fn is_edge_trim_byte(byte: u8) -> bool {
    matches!(byte, b'\n' | b' ' | b'\t' | b'\r')
}

/// Skip an HTML comment. We are positioned just after the opening `<`; the `!--`
/// prefix is still in the iterator (the call site confirmed it via `starts_with`).
/// We simply scan forward to the closing `-->`. The leading `!--` feeds through the
/// dash counter harmlessly: `!`(reset)`-`(1)`-`(2)`>`(close) treats `<!-->` and
/// `<!--->` as abrupt-closed empty comments, matching browser leniency. If no `-->`
/// exists, the rest of the input is consumed (unterminated comment is dropped).
fn skip_comment(_input: &str, chars: &mut Chars<'_>) {
    let mut dashes = 0usize;
    for (_, c) in chars.by_ref() {
        match c {
            '-' => dashes += 1,
            '>' if dashes >= 2 => return,
            _ => dashes = 0,
        }
    }
}

/// Skip to the next `>` **without** honoring quotes (used for declarations and
/// processing instructions, where browsers terminate at the first `>`). Consumes
/// the `>`.
fn skip_to_gt_no_quotes(chars: &mut Chars<'_>) {
    for (_, c) in chars.by_ref() {
        if c == '>' {
            return;
        }
    }
}

/// Read an ASCII tag name starting at the current position (we have already peeked
/// an alphabetic first char but not consumed it). Returns a slice of `input`.
///
/// Stops at the first non-name char (whitespace, `>`, `/`, etc.) **without**
/// consuming it, so the caller's `skip_tag_rest` handles the remainder. The
/// returned slice is always on char boundaries because tag-name chars are ASCII.
fn read_tag_name<'a>(input: &'a str, chars: &mut Chars<'a>) -> &'a str {
    // Current peek is the first name char. Find its byte offset.
    let start = match chars.peek() {
        Some(&(idx, _)) => idx,
        None => return "",
    };
    let mut end = start;
    while let Some(&(idx, c)) = chars.peek() {
        if is_tag_name_char(c) {
            end = idx + c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    slice(input, start, end)
}

/// Like [`read_tag_name`] but for an end tag: the `/` has not been consumed yet.
fn read_tag_name_after_slash<'a>(input: &'a str, chars: &mut Chars<'a>) -> &'a str {
    // Consume the `/`.
    chars.next();
    // Skip any whitespace between `/` and the name (lenient: `</ p>`).
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    read_tag_name(input, chars)
}

/// Consume the remainder of a tag up to and including the closing `>`, honoring
/// single- and double-quoted attribute values (a `>` inside quotes does not close
/// the tag). Used for ordinary tags.
fn skip_tag_rest(chars: &mut Chars<'_>) {
    let mut quote: Option<char> = None;
    for (_, c) in chars.by_ref() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '>' => return,
                _ => {}
            },
        }
    }
}

/// Like [`skip_tag_rest`] but reports whether the tag was self-closing (`... />`).
/// Used for `<script>`/`<style>` so `<script/>` does not swallow following text.
fn skip_tag_rest_detect_self_close(chars: &mut Chars<'_>) -> bool {
    let mut quote: Option<char> = None;
    let mut last_was_slash = false;
    for (_, c) in chars.by_ref() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
                last_was_slash = false;
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    last_was_slash = false;
                }
                '>' => return last_was_slash,
                '/' => last_was_slash = true,
                c if c.is_whitespace() => {}
                _ => last_was_slash = false,
            },
        }
    }
    false
}

/// Drop raw-text content of `<script>`/`<style>` up to and including the matching
/// close tag (`</script>` / `</style>`, case-insensitive). If no close tag exists,
/// the rest of the input is dropped.
///
/// Linear: one forward `find` from the current byte position locates `</`; we never
/// rescan already-consumed bytes because the iterator only moves forward.
fn skip_raw_text_to_close<'a>(input: &'a str, bytes: &[u8], chars: &mut Chars<'a>, name: &str) {
    loop {
        // Where are we now (byte offset of the next char, or end of input)?
        let pos = match chars.peek() {
            Some(&(idx, _)) => idx,
            None => return,
        };
        // Find the next `<` at or after `pos`. memchr-free but linear.
        let rel = match find_byte(&bytes[pos..], b'<') {
            Some(r) => r,
            None => {
                // No more `<`: consume to end.
                for _ in chars.by_ref() {}
                return;
            }
        };
        let lt = pos + rel;
        // Advance the iterator up to (and including) this `<`.
        advance_to_byte(chars, lt);
        // Consume the `<`.
        chars.next();
        // Check for `/name` (case-insensitive) right after.
        if matches_close_tag(input, chars, name) {
            // Consume up to and including `>`.
            skip_to_gt_no_quotes(chars);
            return;
        }
        // Not our close tag — keep scanning (the `<` is already consumed).
    }
}

/// After consuming a `<` inside raw text, check whether what follows is `/name`
/// (optionally followed by whitespace or `>`), case-insensitive, **without**
/// consuming on a non-match beyond what is needed. On a match the iterator is left
/// positioned just before the rest-of-tag so the caller skips to `>`.
fn matches_close_tag<'a>(input: &'a str, chars: &mut Chars<'a>, name: &str) -> bool {
    // Must start with `/`.
    match chars.peek() {
        Some(&(_, '/')) => {
            chars.next();
        }
        _ => return false,
    }
    let candidate = read_tag_name(input, chars);
    eq_ignore_ascii_case(candidate, name)
}

/// Decode an entity beginning at a `&` already consumed by the caller. Emits the
/// decoded character(s) into `out`, or — on anything malformed/unknown — the
/// literal source text (`&`, `&#x;`, `&unknown;`, …). Bounded lookahead.
fn decode_entity_at(input: &str, chars: &mut Chars<'_>, out: &mut String) {
    // Numeric: `&#...`
    if let Some(&(_, '#')) = chars.peek() {
        chars.next(); // consume '#'
        decode_numeric_entity(chars, out);
        return;
    }
    // Named: `&name;`
    let name_start = match chars.peek() {
        Some(&(idx, c)) if c.is_ascii_alphanumeric() => idx,
        _ => {
            // Bare `&` (e.g. `&`, `& `, `&;`): literal.
            out.push('&');
            return;
        }
    };
    let mut end = name_start;
    let mut count = 0usize;
    let mut saw_semi = false;
    while let Some(&(idx, c)) = chars.peek() {
        if c == ';' {
            saw_semi = true;
            chars.next(); // consume ';'
            break;
        }
        if c.is_ascii_alphanumeric() && count < MAX_ENTITY_NAME_LEN {
            end = idx + c.len_utf8();
            count += 1;
            chars.next();
        } else {
            break;
        }
    }
    let name = slice(input, name_start, end);
    if saw_semi {
        if let Some(decoded) = named_entity(name) {
            out.push_str(decoded);
            return;
        }
        // Unknown but well-formed `&name;`: emit verbatim.
        out.push('&');
        out.push_str(name);
        out.push(';');
    } else {
        // No terminating `;`: emit `&name` verbatim (lenient).
        out.push('&');
        out.push_str(name);
    }
}

/// Decode the body of a numeric reference. The `&#` has been consumed; the iterator
/// is positioned at the first body char. Emits the decoded scalar, or literal text
/// on malformed input.
fn decode_numeric_entity(chars: &mut Chars<'_>, out: &mut String) {
    let hex = matches!(chars.peek(), Some(&(_, 'x' | 'X')));
    if hex {
        chars.next(); // consume 'x'/'X'
    }
    let mut value: u32 = 0;
    let mut digits = 0usize; // significant digits (leading zeros excluded)
    let mut saw_digit = false; // any digit at all, including leading zeros
    let mut overflow = false;
    while let Some(&(_, c)) = chars.peek() {
        let d = if hex { c.to_digit(16) } else { c.to_digit(10) };
        match d {
            Some(0) if value == 0 => {
                // Leading zero: consume it but do not count it toward the budget, so
                // a zero-padded yet in-range reference like `&#000000065;` decodes to
                // `A` instead of tripping the "too many digits" overflow guard.
                saw_digit = true;
                chars.next();
            }
            Some(d) if digits < MAX_NUMERIC_DIGITS => {
                value = value
                    .saturating_mul(if hex { 16 } else { 10 })
                    .saturating_add(d);
                if value > 0x10_FFFF {
                    overflow = true;
                }
                digits += 1;
                saw_digit = true;
                chars.next();
            }
            Some(_) => {
                // More significant digits than any scalar can have: out of range.
                overflow = true;
                saw_digit = true;
                chars.next();
                // Keep consuming remaining digits so we land on the `;`/non-digit.
            }
            None => break,
        }
    }
    let had_semi = matches!(chars.peek(), Some(&(_, ';')));
    if had_semi {
        chars.next(); // consume ';'
    }
    if !saw_digit {
        // `&#;`, `&#x;`, `&#`: malformed → literal.
        out.push('&');
        out.push('#');
        if hex {
            out.push('x');
        }
        if had_semi {
            out.push(';');
        }
        return;
    }
    let ch = if overflow || value > 0x10_FFFF {
        '\u{FFFD}'
    } else {
        char::from_u32(value).unwrap_or('\u{FFFD}')
    };
    out.push(ch);
}

// --- pure helpers (no input-derived indexing without checked lookup) ---

/// Byte-boundary-safe slice of `input[start..end]`. `start`/`end` always come from
/// `char_indices`, so they are on char boundaries; `get` keeps it panic-free even
/// if a future change feeds an unaligned offset.
fn slice(input: &str, start: usize, end: usize) -> &str {
    input.get(start..end).unwrap_or("")
}

/// Advance `chars` until the next char's byte offset is `>= target`.
fn advance_to_byte(chars: &mut Chars<'_>, target: usize) {
    while let Some(&(idx, _)) = chars.peek() {
        if idx >= target {
            return;
        }
        chars.next();
    }
}

/// Find the first occurrence of `needle` in `haystack`, returning its index.
fn find_byte(haystack: &[u8], needle: u8) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// ASCII-case-insensitive equality (avoids allocating a lowercased copy).
fn eq_ignore_ascii_case(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(x, y)| x.eq_ignore_ascii_case(&y))
}

/// Characters allowed in a tag name (HTML names are ASCII; we also accept `-` and
/// `:` for custom/namespaced elements). The first char is validated by the caller.
fn is_tag_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_'
}

/// `<br>` / `<hr>`: void elements that contribute a single newline.
fn is_void_newline_tag(name: &str) -> bool {
    eq_ignore_ascii_case(name, "br") || eq_ignore_ascii_case(name, "hr")
}

/// Curated block-level element set (see module docs). Case-insensitive.
fn is_block_tag(name: &str) -> bool {
    // Match on the lowercased ASCII bytes without allocating.
    const BLOCKS: &[&str] = &[
        "address",
        "article",
        "aside",
        "blockquote",
        "br",
        "dd",
        "details",
        "div",
        "dl",
        "dt",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "header",
        "hgroup",
        "hr",
        "li",
        "main",
        "nav",
        "ol",
        "p",
        "pre",
        "section",
        "summary",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "tr",
        "ul",
    ];
    BLOCKS.iter().any(|b| eq_ignore_ascii_case(name, b))
}

/// Curated named-entity table. Returns the decoded string for a recognized name
/// (the name *without* `&`/`;`), or `None` for an unknown name (left verbatim).
fn named_entity(name: &str) -> Option<&'static str> {
    // Exact-match (case-sensitive: HTML named entities are case-sensitive, e.g.
    // `&AMP;` is technically distinct, but we accept the canonical lower forms plus
    // the few traditionally-uppercase ones). Linear scan over a small table — O(1)
    // in input size.
    const TABLE: &[(&str, &str)] = &[
        ("amp", "&"),
        ("lt", "<"),
        ("gt", ">"),
        ("quot", "\""),
        ("apos", "'"),
        ("nbsp", "\u{00A0}"),
        ("copy", "\u{00A9}"),
        ("reg", "\u{00AE}"),
        ("trade", "\u{2122}"),
        ("mdash", "\u{2014}"),
        ("ndash", "\u{2013}"),
        ("hellip", "\u{2026}"),
        ("lsquo", "\u{2018}"),
        ("rsquo", "\u{2019}"),
        ("ldquo", "\u{201C}"),
        ("rdquo", "\u{201D}"),
        ("laquo", "\u{00AB}"),
        ("raquo", "\u{00BB}"),
        ("deg", "\u{00B0}"),
        ("plusmn", "\u{00B1}"),
        ("times", "\u{00D7}"),
        ("divide", "\u{00F7}"),
        ("euro", "\u{20AC}"),
        ("pound", "\u{00A3}"),
        ("cent", "\u{00A2}"),
        ("sect", "\u{00A7}"),
        ("para", "\u{00B6}"),
        ("middot", "\u{00B7}"),
        ("bull", "\u{2022}"),
        // A few traditionally-capitalized aliases people rely on.
        ("AMP", "&"),
        ("LT", "<"),
        ("GT", ">"),
        ("QUOT", "\""),
        ("COPY", "\u{00A9}"),
        ("REG", "\u{00AE}"),
    ];
    TABLE.iter().find(|(k, _)| *k == name).map(|&(_, v)| v)
}
