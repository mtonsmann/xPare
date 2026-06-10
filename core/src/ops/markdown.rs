//! Markdown → plain text.
//!
//! **Implementation owner: strippers stream (A1).** CommonMark is too irregular to
//! reimplement safely, so this wraps the boring, well-audited `pulldown-cmark`
//! parser: walk its event stream and emit the text content, dropping formatting.
//! Our event-handling code is still fuzzed and property-tested for panic freedom.
//!
//! # Robustness contract
//!
//! `pulldown-cmark` is itself panic-free on arbitrary `&str`, and *our* event
//! handling adds no `unwrap`/`expect`/index-by-input: we only ever push the borrowed
//! text of `Text`/`Code`/`Html` events and the structural newlines we choose. Output
//! is deterministic (the parser is a pure function of input + a fixed option set).
//!
//! # Documented rules
//!
//! ## Parser options
//! `ENABLE_TABLES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS`. Tables and
//! strikethrough are common GitHub-flavored constructs whose *text* we want to
//! keep; task lists let us drop the `[ ]`/`[x]` marker cleanly. Footnotes, math,
//! smart punctuation, and metadata blocks are left **off** so the plain-text output
//! is predictable and we never emit footnote bookkeeping or YAML front-matter.
//!
//! ## Inline content
//! * `Text` and inline `Code` → emitted verbatim as plain text.
//! * Emphasis / strong / strikethrough → markers dropped, inner text kept.
//! * Links → the link **text** is kept, the URL/markup dropped.
//! * Images → the **alt text** is kept, the URL dropped. (Images carry their alt
//!   text as nested `Text` events between `Start(Image)` and `End(Image)`.)
//! * `SoftBreak` (a wrapped source line) → a single **space** (joins the line).
//! * `HardBreak` (an explicit `  `/`\` line break) → a single **`\n`**.
//! * `TaskListMarker` → dropped (the checkbox glyph is markup, not content).
//! * `FootnoteReference` / math → dropped (options that produce them are off, but
//!   we handle them defensively as no-ops).
//!
//! ## Block structure
//! Block boundaries emit structural newlines, collapsed to **at most one blank
//! line** (`\n\n`), with the whole document trimmed — so structure is preserved
//! without a pile of blank lines. To match the HTML stripper's "paragraphs are
//! separated by a blank line" feel, **loose** blocks (paragraph, heading,
//! blockquote, code block, list/table *container*, definition list) emit a newline
//! at **both** their start and end, so two of them in a row are separated by `\n\n`.
//! **Tight** boundaries — list **items** and table **rows** — emit only at their
//! end, so items/rows stay on consecutive single lines (`one\ntwo`, not
//! `one\n\ntwo`). A thematic break (`---`) emits one `\n`.
//!
//! ## Tables
//! Cell text within a row is separated by a single **tab** (`\t`); each row ends
//! with a `\n`; rows are tight. So `| a | b |` over a header + one body row yields
//! `a\tb\nc\td`.
//!
//! ## Embedded raw HTML — tags removed, but NOT a sanitizer on its own
//! `Html` (block) and `InlineHtml` events carry raw HTML fragments. We feed each
//! fragment through [`super::html::strip_html`] to extract its text (we own both
//! files, so this reuse is natural), so embedded HTML **tags** are removed.
//!
//! This is **not** a substitute for [`super::html::strip_html`] on untrusted input.
//! `pulldown-cmark` reparses the text *between* inline HTML tags as ordinary
//! Markdown, so for inline raw-text elements (`<script>`/`<style>`) the body arrives
//! as separate `Text` events and survives — e.g.
//! `before <script>evil()</script> after` → `before evil() after`. Only block-level
//! raw HTML (on its own line, blank-separated) is dropped wholesale. **To neutralize
//! scripts/styles in untrusted HTML, run `StripHtml`** (the shell does this on the
//! clipboard's HTML representation); the canonical sanitization order is
//! `StripHtml → StripMarkdown`. See the `script_body_is_strip_htmls_responsibility`
//! regression test in `core/tests/strippers.rs`.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use zeroize::Zeroizing;

use crate::ops::wipe::{push_char_wiping, push_str_wiping};

/// Strip Markdown formatting, producing plain text.
///
/// See the module documentation for the exact, frozen rules. Deterministic and
/// panic-free on any input.
pub fn strip_markdown(input: &str) -> String {
    if let Some(plain) = strip_plain_log_markdown(input) {
        return plain;
    }
    strip_markdown_parser(input)
}

fn strip_markdown_parser(input: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(input, options);

    let mut out = MarkdownOutput::with_capacity(input.len());
    // Are we currently inside a table row? Used to insert a tab between cells.
    let mut in_table_row = false;
    let mut first_cell_in_row = true;

    for event in parser {
        match event {
            Event::Text(text) | Event::Code(text) => {
                out.push_str(&text);
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // Reuse the HTML stripper to extract text from embedded markup. The
                // extracted copy is a clipboard-derived intermediate; `strip_html`
                // pre-sizes it (stripping is shrink-or-equal), so wrapping the
                // returned buffer wipes every byte it ever held.
                let extracted = Zeroizing::new(super::html::strip_html(&html));
                out.push_str(&extracted);
            }
            Event::SoftBreak => out.push_char(' '),
            Event::HardBreak => push_newline(&mut out),
            Event::Rule => push_newline(&mut out),
            Event::Start(tag) => {
                if let Tag::TableCell = tag {
                    if in_table_row && !first_cell_in_row {
                        out.push_char('\t');
                    }
                    first_cell_in_row = false;
                }
                if matches!(tag, Tag::TableRow | Tag::TableHead) {
                    in_table_row = true;
                    first_cell_in_row = true;
                }
                // "Loose" blocks emit a newline at BOTH boundaries so siblings are
                // separated by a blank line (paragraph spacing). This mirrors the
                // HTML stripper, which emits at the start and end of block tags.
                // "Tight" boundaries (list items, table rows) emit only at their
                // end (below), so items stay on consecutive lines without blanks.
                if is_loose_block_start(&tag) {
                    push_newline(&mut out);
                }
            }
            Event::End(tag) => {
                match tag {
                    TagEnd::TableRow | TagEnd::TableHead => {
                        in_table_row = false;
                        push_newline(&mut out);
                    }
                    TagEnd::TableCell => { /* separator handled at next cell start */ }
                    // Every block end emits a structural newline; for loose blocks
                    // this pairs with the start newline to yield a blank-line gap.
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::Item
                    | TagEnd::BlockQuote(_)
                    | TagEnd::CodeBlock
                    | TagEnd::HtmlBlock
                    | TagEnd::List(_)
                    | TagEnd::FootnoteDefinition
                    | TagEnd::Table
                    | TagEnd::DefinitionList
                    | TagEnd::DefinitionListTitle
                    | TagEnd::DefinitionListDefinition
                    | TagEnd::MetadataBlock(_) => push_newline(&mut out),
                    // Inline ends (Emphasis/Strong/Strikethrough/Link/Image): the
                    // text was already emitted; emit nothing extra.
                    TagEnd::Emphasis
                    | TagEnd::Strong
                    | TagEnd::Strikethrough
                    | TagEnd::Link
                    | TagEnd::Image => {}
                }
            }
            // Markers / references we deliberately drop.
            Event::TaskListMarker(_)
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {}
        }
    }

    normalize(out.into_string())
}

pub(crate) fn strip_plain_log_markdown(input: &str) -> Option<String> {
    let mut out = String::with_capacity(input.len());
    let mut in_paragraph = false;
    let mut pending_separator = false;
    let mut wrote_paragraph = false;

    for line in input.split('\n') {
        match plain_log_line_kind(line)? {
            PlainLogLineKind::Blank => {
                if in_paragraph {
                    in_paragraph = false;
                    pending_separator = true;
                }
            }
            PlainLogLineKind::Content => {
                if in_paragraph {
                    out.push(' ');
                } else if pending_separator && wrote_paragraph {
                    out.push_str("\n\n");
                    pending_separator = false;
                }
                out.push_str(line);
                in_paragraph = true;
                wrote_paragraph = true;
            }
        }
    }

    Some(out)
}

enum PlainLogLineKind {
    Blank,
    Content,
}

fn plain_log_line_kind(line: &str) -> Option<PlainLogLineKind> {
    if line.is_empty() {
        return Some(PlainLogLineKind::Blank);
    }

    let bytes = line.as_bytes();
    let mut all_blank = true;
    let mut all_dash = true;
    let mut all_equals = true;
    for (i, &byte) in bytes.iter().enumerate() {
        match byte {
            0x80..=u8::MAX => return None,
            b' ' | b'\t' => {
                all_dash = false;
                all_equals = false;
            }
            b'\r' => return None,
            b'_' => {
                if !is_intraword_ascii_underscore(bytes, i) {
                    return None;
                }
                all_blank = false;
                all_dash = false;
                all_equals = false;
            }
            b'-' => {
                all_blank = false;
                all_equals = false;
            }
            b'=' => {
                all_blank = false;
                all_dash = false;
            }
            _ if is_plain_log_rejected_byte(byte) => return None,
            _ => {
                all_blank = false;
                all_dash = false;
                all_equals = false;
            }
        }
    }

    if all_blank {
        return Some(PlainLogLineKind::Blank);
    }
    if matches!(bytes.first(), Some(b' ' | b'\t'))
        || matches!(bytes.last(), Some(b' ' | b'\t'))
        || starts_unordered_list_marker(bytes)
        || starts_ordered_list_marker(bytes)
        || all_dash
        || all_equals
    {
        return None;
    }
    Some(PlainLogLineKind::Content)
}

fn is_plain_log_rejected_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'&' | b'<'
            | b'>'
            | b'`'
            | b'*'
            | b'~'
            | b'['
            | b']'
            | b'('
            | b')'
            | b'|'
            | b'\\'
            | b'#'
            | b'!'
    )
}

fn is_intraword_ascii_underscore(bytes: &[u8], index: usize) -> bool {
    index > 0
        && index + 1 < bytes.len()
        && bytes[index - 1].is_ascii_alphanumeric()
        && bytes[index + 1].is_ascii_alphanumeric()
}

fn starts_unordered_list_marker(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(b'-' | b'+')) && matches!(bytes.get(1), Some(b' ' | b'\t'))
}

fn starts_ordered_list_marker(bytes: &[u8]) -> bool {
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    digits > 0
        && matches!(bytes.get(digits), Some(b'.' | b')'))
        && matches!(bytes.get(digits + 1), Some(b' ' | b'\t'))
}

/// "Loose" block tags emit a leading newline at their start (in addition to the
/// trailing one at their end), so that two such blocks in a row are separated by a
/// blank line. Tight constructs — list items and table rows/cells — are excluded so
/// they stay on consecutive lines. The list/table *container* is loose so it is
/// offset from surrounding paragraphs, while its rows/items remain tight.
fn is_loose_block_start(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Table(_)
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::MetadataBlock(_)
    )
}

struct MarkdownOutput {
    text: String,
    trailing_newlines: u8,
}

impl MarkdownOutput {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            text: String::with_capacity(capacity),
            trailing_newlines: 0,
        }
    }

    // All appends go through `ops::wipe`. Stripping is shrink-or-equal (growth
    // factor 1 in `Operation::max_growth_factor`), so the `input.len()` starting
    // capacity should make these wipes unreachable — routing through the helpers
    // keeps the no-unwiped-reallocation property structural rather than dependent
    // on that bound holding for every parser event sequence.
    fn push_str(&mut self, value: &str) {
        push_str_wiping(&mut self.text, value);
        self.observe_str_suffix(value);
    }

    fn push_char(&mut self, value: char) {
        push_char_wiping(&mut self.text, value);
        match value {
            '\n' => self.observe_newline(),
            ' ' | '\t' => {}
            _ => self.trailing_newlines = 0,
        }
    }

    fn push_newline(&mut self) {
        if self.text.is_empty() {
            return;
        }
        if self.trailing_newlines < 2 {
            push_char_wiping(&mut self.text, '\n');
            self.observe_newline();
        }
    }

    fn into_string(self) -> String {
        self.text
    }

    fn observe_str_suffix(&mut self, value: &str) {
        let mut suffix_newlines = 0u8;
        for &byte in value.as_bytes().iter().rev() {
            match byte {
                b' ' | b'\t' => {}
                b'\n' => {
                    suffix_newlines = (suffix_newlines + 1).min(2);
                    if suffix_newlines == 2 {
                        self.trailing_newlines = 2;
                        return;
                    }
                }
                _ => {
                    self.trailing_newlines = suffix_newlines;
                    return;
                }
            }
        }
        self.trailing_newlines = (self.trailing_newlines + suffix_newlines).min(2);
    }

    fn observe_newline(&mut self) {
        self.trailing_newlines = (self.trailing_newlines + 1).min(2);
    }
}

/// Push a `\n`, collapsing runs so at most one blank line (`"\n\n"`) ever forms.
/// Mirrors the HTML stripper's whitespace policy for consistent block separation.
fn push_newline(out: &mut MarkdownOutput) {
    out.push_newline();
}

/// Trim leading/trailing ASCII whitespace of the whole document in place.
fn normalize(mut s: String) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn plain_log_token() -> impl Strategy<Value = String> {
        proptest::string::string_regex("[A-Za-z0-9][A-Za-z0-9:/=-]{0,20}").unwrap()
    }

    fn plain_log_line() -> impl Strategy<Value = String> {
        prop::collection::vec(plain_log_token(), 1..8).prop_map(|parts| parts.join(" "))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn plain_log_fast_path_matches_parser(
            lines in prop::collection::vec(prop_oneof![Just(String::new()), plain_log_line()], 0..40),
        ) {
            let input = lines.join("\n");
            let fast = strip_plain_log_markdown(&input)
                .expect("generated input should stay in the plain-log fast path");
            prop_assert_eq!(fast, strip_markdown_parser(&input));
        }
    }
}
