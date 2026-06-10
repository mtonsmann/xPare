//! HTML -> Markdown conversion for common copied-web fragments.
//!
//! This is a lightweight, dependency-free converter for clipboard HTML, not a
//! browser-grade HTML5 renderer. It preserves the structures users commonly want
//! when copying from the web into notes/docs: headings, paragraphs, links, lists,
//! blockquotes, inline emphasis/code, preformatted code blocks, line breaks, and
//! simple table rows.
//!
//! # Robustness / safety contract
//!
//! * Pure safe Rust; no OS, IO, network, logging, or global state.
//! * Panic-free on arbitrary input: all input-derived offsets are checked and tag
//!   names are ASCII-only slices.
//! * Linear in the input size. The scanner moves forward, and raw-text skipping
//!   searches for the next close tag without backtracking. Rendered list
//!   indentation is clamped at `MAX_RENDERED_LIST_INDENT_DEPTH` levels: the
//!   list stack still tracks structure and ordered numbering at any depth, but
//!   two indent spaces per unbounded level would let deeply nested adversarial
//!   lists (thousands of open `<ul>`) grow the output quadratically, past the
//!   `Operation::max_growth_factor` envelope that `Config::validate` relies on.
//! * `<script>` / `<style>` raw-text bodies, comments, declarations, and processing
//!   instructions are dropped.
//! * Link destinations are emitted only for inert schemes (`http`, `https`,
//!   `mailto`) and relative/hash URLs. Unsafe schemes such as `javascript:`,
//!   `data:`, `vbscript:`, and `file:` are dropped while keeping link text.
//! * HTML entities are decoded with the same bounded curated decoder used by
//!   `strip_html`.
//! * Entity-decoded Markdown text escapes raw HTML delimiters, and code/pre
//!   delimiters are chosen longer than any copied backtick run, so inert copied
//!   content cannot break out as active Markdown HTML.
//! * Clipboard-derived working buffers — the output accumulator, the pre/code
//!   side buffers, decoded entity/attribute copies, and link destinations — live
//!   in `Zeroizing` storage, and accumulator appends go through `ops::wipe` so a
//!   capacity-growing reallocation wipes the superseded block first. Only the
//!   returned (trimmed) result leaves unwiped: it is the op's output, which the
//!   pipeline/FFI wipe after use.
//!
//! The output is Markdown plain text. It is intentionally suitable for a one-shot
//! "convert clipboard to Markdown" command, not a persistent cleanup toggle.

use super::html;
use super::wipe::{push_char_wiping, push_str_wiping};
use zeroize::Zeroizing;

/// Convert a common copied-web HTML fragment to Markdown plain text.
pub fn html_to_markdown(input: &str) -> String {
    let mut out = MarkdownOut::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut pos = 0usize;

    while pos < input.len() {
        let lt = match find_byte(&bytes[pos..], b'<') {
            Some(rel) => pos + rel,
            None => {
                out.push_text(slice(input, pos, input.len()));
                break;
            }
        };
        if lt > pos {
            out.push_text(slice(input, pos, lt));
        }

        match parse_tag(input, lt) {
            Some((Tag::Drop, next)) => pos = next,
            Some((
                Tag::Start {
                    name,
                    attrs,
                    self_closing,
                },
                next,
            )) => {
                if eq_ignore_ascii_case(name, "script") || eq_ignore_ascii_case(name, "style") {
                    pos = if self_closing {
                        next
                    } else {
                        skip_raw_text_to_close(input, next, name)
                    };
                    continue;
                }
                out.start_tag(name, attrs);
                if self_closing {
                    out.end_tag(name);
                }
                pos = next;
            }
            Some((Tag::End { name }, next)) => {
                out.end_tag(name);
                pos = next;
            }
            None => {
                out.push_text("<");
                pos = lt + 1;
            }
        }
    }

    out.finish()
}

enum Tag<'a> {
    Start {
        name: &'a str,
        attrs: &'a str,
        self_closing: bool,
    },
    End {
        name: &'a str,
    },
    Drop,
}

fn parse_tag(input: &str, lt: usize) -> Option<(Tag<'_>, usize)> {
    let rest = input.get(lt..)?;
    if rest.starts_with("<!--") {
        return Some((Tag::Drop, skip_comment(input, lt + 4)));
    }

    let after_lt = lt + 1;
    let next = *input.as_bytes().get(after_lt)?;
    match next {
        b'!' | b'?' => Some((Tag::Drop, skip_to_gt_no_quotes(input, after_lt + 1))),
        b'/' => parse_end_tag(input, after_lt + 1),
        b if b.is_ascii_alphabetic() => parse_start_tag(input, after_lt),
        _ => None,
    }
}

fn parse_start_tag(input: &str, name_start: usize) -> Option<(Tag<'_>, usize)> {
    let (name, name_end) = read_tag_name(input, name_start);
    if name.is_empty() {
        return None;
    }
    let (attrs_end, next, self_closing) = find_tag_end(input, name_end);
    let attrs = slice(input, name_end, attrs_end);
    Some((
        Tag::Start {
            name,
            attrs,
            self_closing,
        },
        next,
    ))
}

fn parse_end_tag(input: &str, mut pos: usize) -> Option<(Tag<'_>, usize)> {
    pos = skip_ascii_whitespace(input, pos);
    let (name, name_end) = read_tag_name(input, pos);
    if name.is_empty() {
        return None;
    }
    let (_, next, _) = find_tag_end(input, name_end);
    Some((Tag::End { name }, next))
}

fn read_tag_name(input: &str, mut pos: usize) -> (&str, usize) {
    let start = pos;
    while matches!(input.as_bytes().get(pos), Some(b) if is_tag_name_byte(*b)) {
        pos += 1;
    }
    (slice(input, start, pos), pos)
}

fn find_tag_end(input: &str, pos: usize) -> (usize, usize, bool) {
    let mut quote: Option<char> = None;
    let mut last_non_ws = None;
    for (rel, c) in slice(input, pos, input.len()).char_indices() {
        let idx = pos + rel;
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    last_non_ws = Some(c);
                }
                '>' => return (idx, idx + 1, last_non_ws == Some('/')),
                c if c.is_whitespace() => {}
                _ => last_non_ws = Some(c),
            },
        }
    }
    (input.len(), input.len(), false)
}

fn skip_comment(input: &str, pos: usize) -> usize {
    match slice(input, pos, input.len()).find("-->") {
        Some(rel) => pos + rel + 3,
        None => input.len(),
    }
}

fn skip_to_gt_no_quotes(input: &str, pos: usize) -> usize {
    match slice(input, pos, input.len()).find('>') {
        Some(rel) => pos + rel + 1,
        None => input.len(),
    }
}

fn skip_raw_text_to_close(input: &str, pos: usize, name: &str) -> usize {
    let bytes = input.as_bytes();
    let mut search = pos;
    while search < input.len() {
        let lt = match find_byte(&bytes[search..], b'<') {
            Some(rel) => search + rel,
            None => return input.len(),
        };
        if let Some((Tag::End { name: end_name }, next)) = parse_tag(input, lt) {
            if eq_ignore_ascii_case(end_name, name) {
                return next;
            }
        }
        search = lt + 1;
    }
    input.len()
}

#[derive(Clone, Copy)]
enum ListKind {
    Unordered,
    Ordered { next: usize },
}

/// Deepest list nesting that still adds rendered indentation; items nested
/// deeper render at this depth. The clamp bounds each item's indent at a small
/// constant, which keeps the op inside its documented
/// `Operation::max_growth_factor` envelope — two spaces per unbounded level is
/// quadratic in the number of open lists. Structure tracking and ordered-list
/// numbering use the unbounded `list_stack` and are unaffected.
const MAX_RENDERED_LIST_INDENT_DEPTH: usize = 4;

struct MarkdownOut {
    // The accumulator (and the pre/code side buffers) hold clipboard-derived
    // bytes, so all three live in `Zeroizing` storage — wiped on drop — and every
    // append goes through the `wipe` helpers because Markdown output can outgrow
    // the input (escaping, fences, list markers), and a plain reallocation would
    // free the old block unwiped.
    text: Zeroizing<String>,
    pending_space: bool,
    pre_depth: usize,
    code_depth: usize,
    pre_buffer: Zeroizing<String>,
    code_buffer: Zeroizing<String>,
    list_stack: Vec<ListKind>,
    // Link destinations come from `href` attribute values (clipboard-derived),
    // so they are wiped once popped/dropped.
    link_stack: Vec<Option<Zeroizing<String>>>,
    first_cell_in_row: bool,
}

impl MarkdownOut {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            text: Zeroizing::new(String::with_capacity(capacity)),
            pending_space: false,
            pre_depth: 0,
            code_depth: 0,
            pre_buffer: Zeroizing::new(String::new()),
            code_buffer: Zeroizing::new(String::new()),
            list_stack: Vec::new(),
            link_stack: Vec::new(),
            first_cell_in_row: true,
        }
    }

    fn start_tag(&mut self, name: &str, attrs: &str) {
        if heading_level(name).is_some() {
            self.ensure_blank_line();
            let level = heading_level(name).unwrap_or(1);
            for _ in 0..level {
                push_char_wiping(&mut self.text, '#');
            }
            push_char_wiping(&mut self.text, ' ');
            return;
        }

        if is_paragraph_block(name) {
            self.ensure_blank_line();
            return;
        }

        if eq_ignore_ascii_case(name, "br") {
            self.ensure_newline();
        } else if eq_ignore_ascii_case(name, "hr") {
            self.ensure_blank_line();
            push_str_wiping(&mut self.text, "---");
            self.ensure_blank_line();
        } else if eq_ignore_ascii_case(name, "ul") {
            self.ensure_blank_line();
            self.list_stack.push(ListKind::Unordered);
        } else if eq_ignore_ascii_case(name, "ol") {
            self.ensure_blank_line();
            self.list_stack.push(ListKind::Ordered { next: 1 });
        } else if eq_ignore_ascii_case(name, "li") {
            self.start_list_item();
        } else if eq_ignore_ascii_case(name, "a") {
            self.start_link(attrs);
        } else if eq_ignore_ascii_case(name, "strong") || eq_ignore_ascii_case(name, "b") {
            self.flush_pending_space();
            push_str_wiping(&mut self.text, "**");
        } else if eq_ignore_ascii_case(name, "em") || eq_ignore_ascii_case(name, "i") {
            self.flush_pending_space();
            push_char_wiping(&mut self.text, '_');
        } else if eq_ignore_ascii_case(name, "code") {
            if self.pre_depth == 0 {
                self.flush_pending_space();
                if self.code_depth == 0 {
                    self.code_buffer.clear();
                }
                self.code_depth += 1;
            }
        } else if eq_ignore_ascii_case(name, "pre") {
            self.ensure_blank_line();
            if self.pre_depth == 0 {
                self.pre_buffer.clear();
            }
            self.pre_depth += 1;
        } else if eq_ignore_ascii_case(name, "blockquote") {
            self.ensure_blank_line();
            push_str_wiping(&mut self.text, "> ");
        } else if eq_ignore_ascii_case(name, "table") {
            self.ensure_blank_line();
        } else if eq_ignore_ascii_case(name, "tr") {
            self.ensure_newline();
            self.first_cell_in_row = true;
        } else if eq_ignore_ascii_case(name, "td") || eq_ignore_ascii_case(name, "th") {
            if !self.first_cell_in_row {
                push_char_wiping(&mut self.text, '\t');
            }
            self.first_cell_in_row = false;
        } else if eq_ignore_ascii_case(name, "img") {
            // The alt text is clipboard-derived; wipe the transient copy on drop.
            if let Some(alt) = attr_value(attrs, "alt").map(Zeroizing::new) {
                self.push_text(&alt);
            }
        }
    }

    fn end_tag(&mut self, name: &str) {
        if heading_level(name).is_some() || is_paragraph_block(name) {
            self.ensure_blank_line();
        } else if eq_ignore_ascii_case(name, "ul") || eq_ignore_ascii_case(name, "ol") {
            self.list_stack.pop();
            self.ensure_blank_line();
        } else if eq_ignore_ascii_case(name, "li") {
            self.ensure_newline();
        } else if eq_ignore_ascii_case(name, "a") {
            if let Some(Some(dest)) = self.link_stack.pop() {
                self.trim_trailing_inline();
                push_str_wiping(&mut self.text, "](");
                push_str_wiping(&mut self.text, &dest);
                push_char_wiping(&mut self.text, ')');
            }
        } else if eq_ignore_ascii_case(name, "strong") || eq_ignore_ascii_case(name, "b") {
            self.trim_trailing_inline();
            push_str_wiping(&mut self.text, "**");
        } else if eq_ignore_ascii_case(name, "em") || eq_ignore_ascii_case(name, "i") {
            self.trim_trailing_inline();
            push_char_wiping(&mut self.text, '_');
        } else if eq_ignore_ascii_case(name, "code") {
            if self.pre_depth == 0 && self.code_depth > 0 {
                self.code_depth -= 1;
                if self.code_depth == 0 {
                    self.flush_inline_code();
                }
            }
        } else if eq_ignore_ascii_case(name, "pre") {
            // Only flush when a <pre> was actually open and is now closing; an unmatched
            // </pre> is a no-op (mirrors the <code> handling above). Flushing on a
            // never-opened buffer would emit a spurious empty ``` fence.
            if self.pre_depth > 0 {
                self.pre_depth -= 1;
                if self.pre_depth == 0 {
                    self.flush_pre_block();
                }
            }
        } else if eq_ignore_ascii_case(name, "blockquote") || eq_ignore_ascii_case(name, "table") {
            self.ensure_blank_line();
        } else if eq_ignore_ascii_case(name, "tr") {
            self.ensure_newline();
            self.first_cell_in_row = true;
        }
    }

    fn start_list_item(&mut self) {
        self.ensure_newline();
        // The rendered indent depth is clamped; the stack length is not. See
        // `MAX_RENDERED_LIST_INDENT_DEPTH` for the growth-envelope constraint.
        let depth = self
            .list_stack
            .len()
            .saturating_sub(1)
            .min(MAX_RENDERED_LIST_INDENT_DEPTH);
        for _ in 0..depth {
            push_str_wiping(&mut self.text, "  ");
        }
        match self.list_stack.last_mut() {
            Some(ListKind::Ordered { next }) => {
                let value = *next;
                *next = (*next).saturating_add(1);
                push_str_wiping(&mut self.text, &value.to_string());
                push_str_wiping(&mut self.text, ". ");
            }
            _ => push_str_wiping(&mut self.text, "- "),
        }
    }

    fn start_link(&mut self, attrs: &str) {
        // The raw `href` copy is clipboard-derived: wipe it once the escaped
        // destination (itself wiped via the stack) has been derived from it.
        let dest = attr_value(attrs, "href")
            .map(Zeroizing::new)
            .and_then(|href| safe_link_destination(&href))
            .map(Zeroizing::new);
        if dest.is_some() {
            self.flush_pending_space();
            push_char_wiping(&mut self.text, '[');
        }
        self.link_stack.push(dest);
    }

    fn push_text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        // The decoded copy is a clipboard-derived intermediate; `decode_entities`
        // pre-sizes it exactly (decoding is shrink-or-equal), so wrapping the
        // returned buffer wipes every byte it ever held.
        let decoded = Zeroizing::new(html::decode_entities(raw));
        if self.pre_depth > 0 {
            push_str_wiping(&mut self.pre_buffer, &decoded);
            self.pending_space = false;
            return;
        }
        if self.code_depth > 0 {
            for c in decoded.chars() {
                if c == '\n' || c == '\r' {
                    push_char_wiping(&mut self.code_buffer, ' ');
                } else {
                    push_char_wiping(&mut self.code_buffer, c);
                }
            }
            return;
        }
        for c in decoded.chars() {
            if c.is_whitespace() {
                self.pending_space = true;
            } else {
                self.flush_pending_space();
                push_escaped_text_char(&mut self.text, c);
            }
        }
    }

    fn flush_pending_space(&mut self) {
        if self.pending_space && needs_space_before(&self.text) {
            push_char_wiping(&mut self.text, ' ');
        }
        self.pending_space = false;
    }

    fn ensure_newline(&mut self) {
        self.pending_space = false;
        self.trim_trailing_inline();
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            push_char_wiping(&mut self.text, '\n');
        }
    }

    fn ensure_blank_line(&mut self) {
        self.pending_space = false;
        self.trim_trailing_inline();
        if self.text.is_empty() {
            return;
        }
        let newlines = trailing_newlines(&self.text);
        for _ in newlines..2 {
            push_char_wiping(&mut self.text, '\n');
        }
    }

    fn trim_trailing_inline(&mut self) {
        while matches!(self.text.as_bytes().last(), Some(b' ' | b'\t')) {
            self.text.pop();
        }
    }

    fn flush_inline_code(&mut self) {
        let delimiter = backtick_delimiter(&self.code_buffer, 1);
        push_str_wiping(&mut self.text, &delimiter);
        let needs_edge_space = self.code_buffer.starts_with('`') || self.code_buffer.ends_with('`');
        if needs_edge_space {
            push_char_wiping(&mut self.text, ' ');
        }
        push_str_wiping(&mut self.text, &self.code_buffer);
        if needs_edge_space {
            push_char_wiping(&mut self.text, ' ');
        }
        push_str_wiping(&mut self.text, &delimiter);
        // `clear` keeps the allocation owned by this op; the surrounding
        // `Zeroizing` still wipes its full capacity on drop.
        self.code_buffer.clear();
        self.pending_space = false;
    }

    fn flush_pre_block(&mut self) {
        let delimiter = backtick_delimiter(&self.pre_buffer, 3);
        push_str_wiping(&mut self.text, &delimiter);
        push_char_wiping(&mut self.text, '\n');
        push_str_wiping(&mut self.text, &self.pre_buffer);
        if !self.pre_buffer.ends_with('\n') {
            push_char_wiping(&mut self.text, '\n');
        }
        push_str_wiping(&mut self.text, &delimiter);
        push_str_wiping(&mut self.text, "\n\n");
        self.pre_buffer.clear();
        self.pending_space = false;
    }

    fn finish(self) -> String {
        // The trimmed copy is the op's return value (the pipeline wraps it in
        // `Zeroizing` if it feeds another pass; the FFI wipes the final output on
        // free). The accumulator itself — `self.text`, plus the pre/code/link
        // buffers — is `Zeroizing` storage and is wiped when `self` drops here.
        self.text
            .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
            .to_string()
    }
}

/// `h1`..`h6` (ASCII-case-insensitive) to its level. Byte-wise so no lowercased
/// copy of the (input-derived) tag name is ever allocated.
fn heading_level(name: &str) -> Option<usize> {
    match name.as_bytes() {
        [b'h' | b'H', digit @ b'1'..=b'6'] => Some(usize::from(digit - b'0')),
        _ => None,
    }
}

fn is_paragraph_block(name: &str) -> bool {
    const BLOCKS: &[&str] = &[
        "address",
        "article",
        "aside",
        "div",
        "figcaption",
        "figure",
        "footer",
        "form",
        "header",
        "main",
        "nav",
        "p",
        "section",
        "summary",
    ];
    BLOCKS.iter().any(|b| eq_ignore_ascii_case(name, b))
}

fn attr_value(attrs: &str, wanted: &str) -> Option<String> {
    let mut pos = 0usize;
    while pos < attrs.len() {
        pos = skip_attr_noise(attrs, pos);
        let name_start = pos;
        while matches!(attrs.as_bytes().get(pos), Some(b) if is_tag_name_byte(*b)) {
            pos += 1;
        }
        if name_start == pos {
            pos = advance_one_char(attrs, pos);
            continue;
        }
        let name = slice(attrs, name_start, pos);
        pos = skip_ascii_whitespace(attrs, pos);
        if !matches!(attrs.as_bytes().get(pos), Some(b'=')) {
            continue;
        }
        pos += 1;
        pos = skip_ascii_whitespace(attrs, pos);
        let (value, next) = read_attr_value(attrs, pos);
        pos = next;
        if eq_ignore_ascii_case(name, wanted) {
            // Wipe the full decoded copy; the trimmed copy we hand back is the
            // caller's to wipe (both call sites wrap it in `Zeroizing`).
            let decoded = Zeroizing::new(html::decode_entities(value));
            return Some(decoded.trim().to_string());
        }
    }
    None
}

fn read_attr_value(attrs: &str, pos: usize) -> (&str, usize) {
    match attrs.as_bytes().get(pos) {
        Some(&quote @ (b'"' | b'\'')) => {
            let start = pos + 1;
            let mut end = start;
            while let Some(&b) = attrs.as_bytes().get(end) {
                if b == quote {
                    return (slice(attrs, start, end), end + 1);
                }
                end += 1;
            }
            (slice(attrs, start, attrs.len()), attrs.len())
        }
        Some(_) => {
            let start = pos;
            let mut end = pos;
            while let Some(&b) = attrs.as_bytes().get(end) {
                if b.is_ascii_whitespace() || b == b'/' {
                    break;
                }
                end += 1;
            }
            (slice(attrs, start, end), end)
        }
        None => ("", pos),
    }
}

fn safe_link_destination(href: &str) -> Option<String> {
    let trimmed = href.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return None;
    }
    if let Some(colon) = trimmed.find(':') {
        // Compare in place: no lowercased copy of the (clipboard-derived) scheme.
        let scheme = slice(trimmed, 0, colon);
        if !(scheme.eq_ignore_ascii_case("http")
            || scheme.eq_ignore_ascii_case("https")
            || scheme.eq_ignore_ascii_case("mailto"))
        {
            return None;
        }
    }
    // Exact capacity — wrapper brackets plus one escape byte per `>`/`\` — so the
    // clipboard-derived destination never reallocates while it is built.
    let escapes = trimmed.chars().filter(|c| matches!(c, '>' | '\\')).count();
    let mut out = String::with_capacity(trimmed.len() + 2 + escapes);
    out.push('<');
    for c in trimmed.chars() {
        match c {
            '>' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('>');
    Some(out)
}

fn push_escaped_text_char(out: &mut String, c: char) {
    match c {
        '\\' | '*' | '_' | '[' | ']' | '`' | '|' | '<' | '>' => {
            push_char_wiping(out, '\\');
            push_char_wiping(out, c);
        }
        _ => push_char_wiping(out, c),
    }
}

fn backtick_delimiter(content: &str, minimum: usize) -> String {
    let needed = max_run(content, '`').saturating_add(1).max(minimum);
    "`".repeat(needed)
}

fn max_run(content: &str, needle: char) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for c in content.chars() {
        if c == needle {
            current = current.saturating_add(1);
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn needs_space_before(out: &str) -> bool {
    match out.as_bytes().last() {
        None => false,
        Some(b'\n' | b' ' | b'\t' | b'[' | b'>' | b'(') => false,
        Some(_) => true,
    }
}

fn trailing_newlines(s: &str) -> usize {
    let mut count = 0usize;
    for &b in s.as_bytes().iter().rev().take(2) {
        if b == b'\n' {
            count += 1;
        } else {
            break;
        }
    }
    count
}

fn skip_attr_noise(attrs: &str, mut pos: usize) -> usize {
    while matches!(attrs.as_bytes().get(pos), Some(b) if b.is_ascii_whitespace() || *b == b'/') {
        pos += 1;
    }
    pos
}

fn skip_ascii_whitespace(input: &str, mut pos: usize) -> usize {
    while matches!(input.as_bytes().get(pos), Some(b) if b.is_ascii_whitespace()) {
        pos += 1;
    }
    pos
}

fn advance_one_char(input: &str, pos: usize) -> usize {
    match slice(input, pos, input.len()).chars().next() {
        Some(c) => pos + c.len_utf8(),
        None => input.len(),
    }
}

fn is_tag_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b':' | b'_')
}

fn eq_ignore_ascii_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn slice(input: &str, start: usize, end: usize) -> &str {
    input.get(start..end).unwrap_or("")
}

fn find_byte(haystack: &[u8], needle: u8) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}
