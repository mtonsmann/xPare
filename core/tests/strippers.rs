//! Concrete regression + property tests for the two strippers (owner: A1).
//!
//! The `regression_*` modules pin **exact** outputs for the documented rules in
//! `ops::html` and `ops::markdown` (tags, entities, script/style dropping,
//! comments, quoted `>`, emphasis/headings/links/code/lists/tables/embedded HTML).
//! The `properties` module asserts the two universal invariants over arbitrary
//! `String`: the strippers never panic and are deterministic.

use proptest::prelude::*;
use safetystrip_core::ops::html::strip_html;
use safetystrip_core::ops::markdown::strip_markdown;

mod html_regression {
    use super::*;

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(strip_html("hello world"), "hello world");
    }

    #[test]
    fn empty_input() {
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn inline_tags_emit_no_whitespace() {
        // b, i, span, em, strong, code, a are all inline → text concatenates.
        assert_eq!(strip_html("a<b>bold</b>c"), "aboldc");
        assert_eq!(strip_html("<em>x</em><strong>y</strong>"), "xy");
        assert_eq!(strip_html("<span>1</span><code>2</code>"), "12");
    }

    #[test]
    fn paragraph_block_produces_clean_text() {
        // Leading/trailing structural newlines are trimmed.
        assert_eq!(strip_html("<p>Hello</p>"), "Hello");
    }

    #[test]
    fn two_blocks_get_one_blank_line() {
        assert_eq!(strip_html("<div>a</div><div>b</div>"), "a\n\nb");
        assert_eq!(strip_html("<p>one</p><p>two</p>"), "one\n\ntwo");
    }

    #[test]
    fn literal_newline_runs_collapse_to_one_blank_line() {
        // Source newlines (not just tag-emitted ones) collapse: at most one blank
        // line survives, matching the documented guarantee (regression).
        assert_eq!(strip_html("A\n\n\n\nB"), "A\n\nB");
        assert_eq!(strip_html("<b>a</b>\n\n\n<b>b</b>"), "a\n\nb");
        // A single source newline is preserved (a blank line == "\n\n" is allowed).
        assert_eq!(strip_html("line1\nline2"), "line1\nline2");
    }

    #[test]
    fn br_and_hr_emit_single_newline() {
        assert_eq!(strip_html("a<br>b"), "a\nb");
        assert_eq!(strip_html("a<br/>b"), "a\nb");
        assert_eq!(strip_html("x<hr>y"), "x\ny");
    }

    #[test]
    fn list_items_separate_lines() {
        assert_eq!(
            strip_html("<ul><li>one</li><li>two</li></ul>"),
            "one\n\ntwo"
        );
    }

    #[test]
    fn quoted_gt_does_not_close_tag() {
        assert_eq!(strip_html(r#"<a title="a>b">x</a>"#), "x");
        assert_eq!(strip_html(r#"<a title='a>b'>y</a>"#), "y");
        // A '>' inside the quoted attr must not leak as text.
        assert_eq!(strip_html(r#"<input value=">>>">tail"#), "tail");
    }

    #[test]
    fn script_contents_dropped() {
        assert_eq!(strip_html("<script>alert(1<2)</script>hi"), "hi");
        // Case-insensitive close tag.
        assert_eq!(strip_html("<ScRiPt>x</SCRIPT>after"), "after");
    }

    #[test]
    fn style_contents_dropped() {
        assert_eq!(strip_html("<style>body{color:red}</style>text"), "text");
    }

    #[test]
    fn unclosed_script_drops_remainder() {
        assert_eq!(strip_html("keep<script>forever and ever"), "keep");
    }

    #[test]
    fn self_closing_script_does_not_swallow_text() {
        // `<script/>` is self-closed: following text survives.
        assert_eq!(strip_html("a<script/>b"), "ab");
    }

    #[test]
    fn comments_dropped() {
        assert_eq!(strip_html("a<!-- comment -->b"), "ab");
        assert_eq!(strip_html("a<!-- has < and > inside -->b"), "ab");
    }

    #[test]
    fn unterminated_comment_drops_remainder() {
        assert_eq!(strip_html("keep<!-- never closed and on"), "keep");
    }

    #[test]
    fn doctype_and_pi_dropped() {
        assert_eq!(strip_html("<!doctype html>text"), "text");
        assert_eq!(strip_html("<?xml version=\"1.0\"?>body"), "body");
    }

    #[test]
    fn named_entities_decode() {
        assert_eq!(strip_html("&amp;&lt;&gt;&quot;&apos;"), "&<>\"'");
        assert_eq!(strip_html("&copy;&reg;&trade;"), "\u{00A9}\u{00AE}\u{2122}");
        assert_eq!(
            strip_html("&mdash;&ndash;&hellip;"),
            "\u{2014}\u{2013}\u{2026}"
        );
        assert_eq!(strip_html("&nbsp;"), "\u{00A0}");
    }

    #[test]
    fn numeric_entities_decode() {
        assert_eq!(strip_html("&#65;&#66;&#67;"), "ABC");
        assert_eq!(strip_html("&#x41;&#X42;"), "AB");
        assert_eq!(strip_html("&#128512;"), "\u{1F600}");
    }

    #[test]
    fn zero_padded_numeric_entities_decode() {
        // Leading zeros are skipped rather than counted toward the digit budget, so a
        // zero-padded but in-range reference still decodes (regression).
        assert_eq!(strip_html("&#000000065;"), "A");
        assert_eq!(strip_html("&#x000000041;"), "A");
    }

    #[test]
    fn out_of_range_numeric_becomes_replacement_char() {
        // > U+10FFFF and surrogate range → U+FFFD, never a panic, never raw digits.
        assert_eq!(strip_html("&#xffffffff;"), "\u{FFFD}");
        assert_eq!(strip_html("&#xD800;"), "\u{FFFD}");
        assert_eq!(strip_html("&#1114112;"), "\u{FFFD}"); // 0x110000
    }

    #[test]
    fn malformed_entities_are_literal() {
        assert_eq!(strip_html("bare & here"), "bare & here");
        assert_eq!(strip_html("&;"), "&;");
        assert_eq!(strip_html("&#;"), "&#;");
        assert_eq!(strip_html("&#x;"), "&#x;");
        assert_eq!(strip_html("&#xZZ;"), "&#xZZ;");
        assert_eq!(strip_html("&amp"), "&amp"); // no terminating ';'
        assert_eq!(strip_html("&unknownentity;"), "&unknownentity;");
    }

    #[test]
    fn stray_angles_are_literal() {
        assert_eq!(strip_html("3 < 4"), "3 < 4");
        assert_eq!(strip_html("5 > 2"), "5 > 2");
    }

    #[test]
    fn lone_lt_before_space_is_literal() {
        assert_eq!(strip_html("a < b"), "a < b");
        assert_eq!(strip_html("<<<"), "<<<");
    }

    #[test]
    fn unterminated_alpha_tag_at_eof_is_dropped() {
        // `<b` with no `>` is an unclosed start tag (browser-style): the markup
        // (and everything after, since there is no `>`) is consumed.
        assert_eq!(strip_html("a<b"), "a");
        assert_eq!(strip_html("text<div without close"), "text");
    }

    #[test]
    fn block_tag_pair_breaks_line_once() {
        // A single h1 contributes a start+end newline around its text; with no
        // following block there is just one break before trailing inline text.
        assert_eq!(strip_html("<h1>Title</h1>body"), "Title\nbody");
        // Two heading blocks → blank line between them.
        assert_eq!(strip_html("<h1>A</h1><h2>B</h2>"), "A\n\nB");
    }

    #[test]
    fn attributes_are_stripped() {
        assert_eq!(
            strip_html(r#"<a href="https://example.com" class="x">link</a>"#),
            "link"
        );
    }
}

mod markdown_regression {
    use super::*;

    #[test]
    fn plain_text() {
        assert_eq!(strip_markdown("hello world"), "hello world");
    }

    #[test]
    fn empty_input() {
        assert_eq!(strip_markdown(""), "");
    }

    #[test]
    fn emphasis_markers_dropped() {
        assert_eq!(strip_markdown("*italic*"), "italic");
        assert_eq!(strip_markdown("**bold**"), "bold");
        assert_eq!(strip_markdown("_em_ and __strong__"), "em and strong");
    }

    #[test]
    fn strikethrough_dropped() {
        assert_eq!(strip_markdown("~~gone~~"), "gone");
    }

    #[test]
    fn inline_code_kept() {
        assert_eq!(
            strip_markdown("use `let x = 1;` here"),
            "use let x = 1; here"
        );
    }

    #[test]
    fn headings_become_plain_lines() {
        assert_eq!(strip_markdown("# Title"), "Title");
        assert_eq!(strip_markdown("## Sub\n\nbody"), "Sub\n\nbody");
    }

    #[test]
    fn link_keeps_text_drops_url() {
        assert_eq!(
            strip_markdown("see [the docs](https://example.com) now"),
            "see the docs now"
        );
    }

    #[test]
    fn image_keeps_alt_drops_url() {
        assert_eq!(strip_markdown("![alt text](img.png)"), "alt text");
    }

    #[test]
    fn lists_separate_items() {
        // List items are "tight": one newline between items, no blank line.
        assert_eq!(strip_markdown("- one\n- two\n- three"), "one\ntwo\nthree");
    }

    #[test]
    fn task_list_marker_dropped() {
        assert_eq!(strip_markdown("- [x] done\n- [ ] todo"), "done\ntodo");
    }

    #[test]
    fn blockquote_text_kept() {
        assert_eq!(strip_markdown("> quoted line"), "quoted line");
    }

    #[test]
    fn fenced_code_block_kept_as_text() {
        let out = strip_markdown("```\nlet x = 1;\n```");
        assert_eq!(out, "let x = 1;");
    }

    #[test]
    fn soft_break_becomes_space() {
        // Two source lines in one paragraph join with a single space.
        assert_eq!(strip_markdown("line one\nline two"), "line one line two");
    }

    #[test]
    fn hard_break_becomes_newline() {
        // Trailing backslash is a hard break.
        assert_eq!(strip_markdown("line one\\\nline two"), "line one\nline two");
    }

    #[test]
    fn table_cells_tab_separated_rows_newline() {
        // Rows are tight: cells tab-separated, one newline between header and body.
        let md = "| a | b |\n| --- | --- |\n| c | d |";
        assert_eq!(strip_markdown(md), "a\tb\nc\td");
    }

    #[test]
    fn embedded_inline_html_is_stripped() {
        // The InlineHtml fragments are routed through strip_html.
        assert_eq!(strip_markdown("text <b>bold</b> end"), "text bold end");
    }

    #[test]
    fn embedded_block_html_is_stripped() {
        let md = "<div>\n<p>hello</p>\n</div>";
        let out = strip_markdown(md);
        assert!(out.contains("hello"), "got {out:?}");
        assert!(!out.contains('<'), "tags should be gone, got {out:?}");
    }

    #[test]
    fn embedded_html_entity_decoded_via_reuse() {
        // Entity decoding comes for free through the strip_html reuse path.
        assert_eq!(strip_markdown("A <span>&amp;</span> B"), "A & B");
    }

    #[test]
    fn script_body_is_strip_htmls_responsibility() {
        // pulldown-cmark reparses the text *between* inline HTML tags as Markdown text,
        // so strip_markdown ALONE removes the `<script>` tags but can leave the body
        // text behind. This is by design: `strip_html` — the parser the shell runs on
        // the raw HTML it reads from the clipboard — is the security workhorse that
        // neutralizes `<script>`/`<style>` bodies. The documented sanitization order is
        // StripHtml -> StripMarkdown (see DESIGN.md / the transform-correctness guardrail).
        let md_only = strip_markdown("before <script>evil()</script> after");
        assert!(
            !md_only.contains("<script>") && !md_only.contains("</script>"),
            "strip_markdown still removes the tags, got {md_only:?}"
        );

        // strip_html drops the script body outright (its raw-text-element rule)...
        assert!(!strip_html("before <script>evil()</script> after").contains("evil"));
        // ...so the canonical StripHtml -> StripMarkdown composition is clean.
        let sanitized = strip_markdown(&strip_html("before <script>evil()</script> after"));
        assert!(
            !sanitized.contains("evil"),
            "StripHtml -> StripMarkdown must drop the body, got {sanitized:?}"
        );
    }
}

mod properties {
    use super::*;

    proptest! {
        // Default config runs plenty of cases; keep it bounded for CI speed.
        #![proptest_config(ProptestConfig::with_cases(2048))]

        /// strip_html never panics on arbitrary text.
        #[test]
        fn strip_html_never_panics(s in any::<String>()) {
            let _ = strip_html(&s);
        }

        /// strip_markdown never panics on arbitrary text.
        #[test]
        fn strip_markdown_never_panics(s in any::<String>()) {
            let _ = strip_markdown(&s);
        }

        /// strip_html is deterministic: same input → same output.
        #[test]
        fn strip_html_deterministic(s in any::<String>()) {
            prop_assert_eq!(strip_html(&s), strip_html(&s));
        }

        /// strip_markdown is deterministic.
        #[test]
        fn strip_markdown_deterministic(s in any::<String>()) {
            prop_assert_eq!(strip_markdown(&s), strip_markdown(&s));
        }

        /// Idempotence is NOT claimed, but the output of strip_html must itself be
        /// safe to re-strip without panicking (used in chained pipelines).
        #[test]
        fn strip_html_output_restrippable(s in any::<String>()) {
            let once = strip_html(&s);
            let _ = strip_html(&once);
            let _ = strip_markdown(&once);
        }

        /// Heavy on markup characters — bias the generator toward `<`, `>`, `&`,
        /// `;`, `#`, quotes, slashes so tag/entity state machines are stressed.
        #[test]
        fn strip_html_markup_soup_never_panics(
            s in proptest::collection::vec(
                proptest::sample::select(vec![
                    '<', '>', '&', ';', '#', '/', '"', '\'', '!', '?', '-',
                    'x', 'X', 'a', 'b', 'p', '1', ' ', '\n', '\u{00A0}', '\u{1F600}',
                ]),
                0..256,
            ).prop_map(|v| v.into_iter().collect::<String>())
        ) {
            let a = strip_html(&s);
            let b = strip_html(&s);
            prop_assert_eq!(a, b);
        }

        /// Markdown markup soup — bias toward markdown metacharacters.
        #[test]
        fn strip_markdown_markup_soup_never_panics(
            s in proptest::collection::vec(
                proptest::sample::select(vec![
                    '#', '*', '_', '`', '~', '>', '-', '+', '[', ']', '(', ')',
                    '|', '!', '\\', 'x', '1', ' ', '\t', '\n', '<', '&',
                ]),
                0..256,
            ).prop_map(|v| v.into_iter().collect::<String>())
        ) {
            let a = strip_markdown(&s);
            let b = strip_markdown(&s);
            prop_assert_eq!(a, b);
        }
    }
}
