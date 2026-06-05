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
//! ## Embedded raw HTML — no chaining needed
//! `Html` (block) and `InlineHtml` events carry raw HTML fragments. We feed each
//! fragment through [`super::html::strip_html`] to extract its text (we own both
//! files, so this reuse is natural). **Because of this, callers do not need to
//! chain StripMarkdown → StripHtml to clean Markdown that embeds HTML:** a single
//! StripMarkdown already removes embedded tags.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Strip Markdown formatting, producing plain text.
///
/// See the module documentation for the exact, frozen rules. Deterministic and
/// panic-free on any input.
pub fn strip_markdown(input: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(input, options);

    let mut out = String::with_capacity(input.len());
    // Are we currently inside a table row? Used to insert a tab between cells.
    let mut in_table_row = false;
    let mut first_cell_in_row = true;

    for event in parser {
        match event {
            Event::Text(text) | Event::Code(text) => {
                out.push_str(&text);
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // Reuse the HTML stripper to extract text from embedded markup.
                let extracted = super::html::strip_html(&html);
                out.push_str(&extracted);
            }
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => push_newline(&mut out),
            Event::Rule => push_newline(&mut out),
            Event::Start(tag) => {
                if let Tag::TableCell = tag {
                    if in_table_row && !first_cell_in_row {
                        out.push('\t');
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

    normalize(out)
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

/// Push a `\n`, collapsing runs so at most one blank line (`"\n\n"`) ever forms.
/// Mirrors the HTML stripper's whitespace policy for consistent block separation.
fn push_newline(out: &mut String) {
    let trailing_newlines = out
        .as_bytes()
        .iter()
        .rev()
        .take_while(|&&b| b == b'\n' || b == b' ' || b == b'\t')
        .filter(|&&b| b == b'\n')
        .count();
    if trailing_newlines < 2 {
        out.push('\n');
    }
}

/// Trim leading/trailing whitespace of the whole document. A single allocation only
/// when trimming actually changes the string.
fn normalize(s: String) -> String {
    let trimmed = s.trim_matches(|c: char| c == '\n' || c == ' ' || c == '\t' || c == '\r');
    if trimmed.len() == s.len() {
        s
    } else {
        trimmed.to_string()
    }
}
