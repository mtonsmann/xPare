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
//!   searches for the next close tag without backtracking.
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
//!
//! The output is Markdown plain text. It is intentionally suitable for a one-shot
//! "convert clipboard to Markdown" command, not a persistent cleanup toggle.

use super::html;
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

struct MarkdownOut {
    text: String,
    pending_space: bool,
    pre_depth: usize,
    code_depth: usize,
    pre_buffer: Zeroizing<String>,
    code_buffer: Zeroizing<String>,
    list_stack: Vec<ListKind>,
    link_stack: Vec<Option<String>>,
    first_cell_in_row: bool,
}

impl MarkdownOut {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            text: String::with_capacity(capacity),
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
                self.text.push('#');
            }
            self.text.push(' ');
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
            self.text.push_str("---");
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
            self.text.push_str("**");
        } else if eq_ignore_ascii_case(name, "em") || eq_ignore_ascii_case(name, "i") {
            self.flush_pending_space();
            self.text.push('_');
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
            self.text.push_str("> ");
        } else if eq_ignore_ascii_case(name, "table") {
            self.ensure_blank_line();
        } else if eq_ignore_ascii_case(name, "tr") {
            self.ensure_newline();
            self.first_cell_in_row = true;
        } else if eq_ignore_ascii_case(name, "td") || eq_ignore_ascii_case(name, "th") {
            if !self.first_cell_in_row {
                self.text.push('\t');
            }
            self.first_cell_in_row = false;
        } else if eq_ignore_ascii_case(name, "img") {
            if let Some(alt) = attr_value(attrs, "alt") {
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
                self.text.push_str("](");
                self.text.push_str(&dest);
                self.text.push(')');
            }
        } else if eq_ignore_ascii_case(name, "strong") || eq_ignore_ascii_case(name, "b") {
            self.trim_trailing_inline();
            self.text.push_str("**");
        } else if eq_ignore_ascii_case(name, "em") || eq_ignore_ascii_case(name, "i") {
            self.trim_trailing_inline();
            self.text.push('_');
        } else if eq_ignore_ascii_case(name, "code") {
            if self.pre_depth == 0 && self.code_depth > 0 {
                self.code_depth -= 1;
                if self.code_depth == 0 {
                    self.flush_inline_code();
                }
            }
        } else if eq_ignore_ascii_case(name, "pre") {
            if self.pre_depth > 0 {
                self.pre_depth -= 1;
            }
            if self.pre_depth == 0 {
                self.flush_pre_block();
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
        let depth = self.list_stack.len().saturating_sub(1);
        for _ in 0..depth {
            self.text.push_str("  ");
        }
        match self.list_stack.last_mut() {
            Some(ListKind::Ordered { next }) => {
                let value = *next;
                *next = (*next).saturating_add(1);
                self.text.push_str(&value.to_string());
                self.text.push_str(". ");
            }
            _ => self.text.push_str("- "),
        }
    }

    fn start_link(&mut self, attrs: &str) {
        let dest = attr_value(attrs, "href").and_then(|href| safe_link_destination(&href));
        if dest.is_some() {
            self.flush_pending_space();
            self.text.push('[');
        }
        self.link_stack.push(dest);
    }

    fn push_text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let decoded = html::decode_entities(raw);
        if self.pre_depth > 0 {
            self.pre_buffer.push_str(&decoded);
            self.pending_space = false;
            return;
        }
        if self.code_depth > 0 {
            for c in decoded.chars() {
                if c == '\n' || c == '\r' {
                    self.code_buffer.push(' ');
                } else {
                    self.code_buffer.push(c);
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
            self.text.push(' ');
        }
        self.pending_space = false;
    }

    fn ensure_newline(&mut self) {
        self.pending_space = false;
        self.trim_trailing_inline();
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
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
            self.text.push('\n');
        }
    }

    fn trim_trailing_inline(&mut self) {
        while matches!(self.text.as_bytes().last(), Some(b' ' | b'\t')) {
            self.text.pop();
        }
    }

    fn flush_inline_code(&mut self) {
        let delimiter = backtick_delimiter(&self.code_buffer, 1);
        self.text.push_str(&delimiter);
        let needs_edge_space = self.code_buffer.starts_with('`') || self.code_buffer.ends_with('`');
        if needs_edge_space {
            self.text.push(' ');
        }
        self.text.push_str(&self.code_buffer);
        if needs_edge_space {
            self.text.push(' ');
        }
        self.text.push_str(&delimiter);
        self.code_buffer.clear();
        self.pending_space = false;
    }

    fn flush_pre_block(&mut self) {
        let delimiter = backtick_delimiter(&self.pre_buffer, 3);
        self.text.push_str(&delimiter);
        self.text.push('\n');
        self.text.push_str(&self.pre_buffer);
        if !self.pre_buffer.ends_with('\n') {
            self.text.push('\n');
        }
        self.text.push_str(&delimiter);
        self.text.push_str("\n\n");
        self.pre_buffer.clear();
        self.pending_space = false;
    }

    fn finish(mut self) -> String {
        self.pending_space = false;
        self.text
            .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
            .to_string()
    }
}

fn heading_level(name: &str) -> Option<usize> {
    match name.to_ascii_lowercase().as_str() {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
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
            let decoded = html::decode_entities(value);
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
        let scheme = slice(trimmed, 0, colon).to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https" | "mailto") {
            return None;
        }
    }
    let mut out = String::with_capacity(trimmed.len() + 2);
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
            out.push('\\');
            out.push(c);
        }
        _ => out.push(c),
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
