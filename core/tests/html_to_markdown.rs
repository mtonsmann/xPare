use proptest::prelude::*;
use safetystrip_core::ops::html_to_markdown::html_to_markdown;
use safetystrip_core::{transform, Config, Operation};

#[test]
fn converts_common_web_structure() {
    let input = concat!(
        "<h1>Title &amp; More</h1>",
        "<p>Read <a href=\"https://example.com?a=1&amp;b=2\">the guide</a>.</p>",
        "<ul><li>One</li><li><strong>Two</strong></li></ul>"
    );
    assert_eq!(
        html_to_markdown(input),
        "# Title & More\n\nRead [the guide](<https://example.com?a=1&b=2>).\n\n- One\n- **Two**"
    );
}

#[test]
fn drops_active_content_and_unsafe_links() {
    let input = concat!(
        "<p>safe",
        "<script>alert('x')</script>",
        "<style>body{display:none}</style>",
        " <a href=\"javascript:evil()\">bad link</a>",
        " <a href=\"https://safe.example\">safe link</a>",
        "</p>"
    );
    assert_eq!(
        html_to_markdown(input),
        "safe bad link [safe link](<https://safe.example>)"
    );
}

#[test]
fn escapes_entity_decoded_raw_html_markdown() {
    let input = concat!(
        "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>",
        "<p>&lt;img src=x onerror=alert(1)&gt;</p>"
    );
    assert_eq!(
        html_to_markdown(input),
        "\\<script\\>alert(1)\\</script\\>\n\n\\<img src=x onerror=alert(1)\\>"
    );
}

#[test]
fn converts_preformatted_code() {
    let input = "<pre><code>fn main() {\n  println!(\"x\");\n}</code></pre>";
    assert_eq!(
        html_to_markdown(input),
        "```\nfn main() {\n  println!(\"x\");\n}\n```"
    );
}

#[test]
fn preformatted_code_fence_outgrows_copied_backticks() {
    let input = "<pre>```\n&lt;img src=x onerror=alert(1)&gt;</pre>";
    assert_eq!(
        html_to_markdown(input),
        "````\n```\n<img src=x onerror=alert(1)>\n````"
    );
}

#[test]
fn inline_code_delimiter_outgrows_copied_backticks() {
    let input = "<p><code>`&lt;script&gt;`</code></p>";
    assert_eq!(html_to_markdown(input), "`` `<script>` ``");
}

#[test]
fn handles_malformed_tags_without_panicking() {
    assert_eq!(html_to_markdown("keep<script>forever"), "keep");
    assert_eq!(html_to_markdown("a < b and <not-closed"), "a \\< b and");
    assert_eq!(html_to_markdown("x<!-- never closed"), "x");
}

#[test]
fn dispatches_through_transform_pipeline() {
    let cfg = Config::as_given(vec![Operation::HtmlToMarkdown]);
    assert_eq!(transform("<h2>Hello</h2>", &cfg), "## Hello");
}

// ---------------------------------------------------------------------------
// Mutation-survivor regressions. Each input pins behavior no existing test
// exercised: comment/PI/declaration skipping, raw-text close-tag scanning,
// tag/attribute parsing, table cells, heading levels, emphasis/code/list
// structure, and link-destination handling. Expected outputs are verified
// against the real parser.
// ---------------------------------------------------------------------------

#[test]
fn comments_declarations_and_pis_are_skipped_exactly() {
    // skip_comment / skip_to_gt_no_quotes index math + the b'!'|b'?' arm.
    assert_eq!(html_to_markdown("AA<!--x-->bcd"), "AAbcd");
    assert_eq!(html_to_markdown("<!DOCTYPE html>x"), "x");
    assert_eq!(html_to_markdown("<?pi?>X"), "X");
}

#[test]
fn non_alpha_tag_start_is_literal_text() {
    // parse_tag: a '<' not followed by an ASCII letter (or !/?) is literal, escaped text.
    assert_eq!(html_to_markdown("<1>x"), "\\<1\\>x");
}

#[test]
fn quote_in_attribute_does_not_end_tag_early() {
    // find_tag_end: a '>' inside a quoted attribute value does not end the tag.
    assert_eq!(html_to_markdown(r#"<p title="a>b">hi</p>"#), "hi");
}

#[test]
fn raw_text_element_closes_only_on_real_close_tag() {
    // skip_raw_text_to_close: a stray '<' in raw text must not end the element early.
    assert_eq!(html_to_markdown("<script>a<b</script>X"), "X");
}

#[test]
fn table_cells_are_tab_separated() {
    // start_tag td/th cell handling.
    assert_eq!(
        html_to_markdown("<table><tr><td>a</td><td>b</td></tr></table>"),
        "a\tb"
    );
}

#[test]
fn nested_inline_code_keeps_inner_text() {
    // start_tag code_depth tracking (== guards).
    assert_eq!(html_to_markdown("<code>a<code>b</code></code>"), "`ab`");
}

#[test]
fn heading_and_list_block_separation() {
    // end_tag blank-line / list-stack handling (|| guards).
    assert_eq!(html_to_markdown("<h1>A</h1>B"), "# A\n\nB");
    assert_eq!(
        html_to_markdown("<ul><li>a</li></ul><ul><li>b</li></ul>"),
        "- a\n\n- b"
    );
}

#[test]
fn emphasis_close_tag_emits_marker() {
    // end_tag em/i close (|| guard).
    assert_eq!(html_to_markdown("<em>x</em>"), "_x_");
}

#[test]
fn unbalanced_close_tags_do_not_underflow() {
    // end_tag depth guards must use `>` not `>=` (a `>= 0` check underflows usize -> panic).
    assert_eq!(html_to_markdown("</code>x"), "x");
    // A stray </pre> (no open <pre>) emits an empty fence rather than underflowing pre_depth.
    assert_eq!(html_to_markdown("</pre>y"), "```\n\n```\n\ny");
}

#[test]
fn all_heading_levels_render_distinctly() {
    // heading_level match arms h3/h4/h5/h6.
    assert_eq!(
        html_to_markdown("<h3>a</h3><h4>b</h4><h5>c</h5><h6>d</h6>"),
        "### a\n\n#### b\n\n##### c\n\n###### d"
    );
}

#[test]
fn link_destination_edge_cases() {
    // read_attr_value index math + safe_link_destination (empty/escaped destinations).
    assert_eq!(html_to_markdown(r#"<a href="">t</a>"#), "t");
    assert_eq!(
        html_to_markdown(r#"<a title="x" href="http://e.com">t</a>"#),
        "[t](<http://e.com>)"
    );
    assert_eq!(
        html_to_markdown("<a href=mailto:a@b.com>t</a>"),
        "[t](<mailto:a@b.com>)"
    );
    // unquoted href value terminates correctly (read_attr_value index math + || guard).
    assert_eq!(html_to_markdown("<a href=x>t</a>"), "[t](<x>)");
}

proptest! {
    #[test]
    fn arbitrary_input_is_deterministic_and_panic_free(s in ".*") {
        let once = html_to_markdown(&s);
        let twice = html_to_markdown(&s);
        prop_assert_eq!(once, twice);
    }
}

// ---------------------------------------------------------------------------
// Second batch of mutation-survivor regressions. Outputs verified against the
// real converter; each test names the source line of the mutant it pins.
// ---------------------------------------------------------------------------

#[test]
fn self_close_with_spaces_is_detected() {
    // find_tag_end L169 whitespace arm: a '/' followed by whitespace before '>' must
    // still self-close the tag (last_non_ws stays '/'), so <i/ > opens AND closes <em>.
    // Mutating the `is_whitespace()` guard to true/false breaks self-close detection
    // (a space would clobber last_non_ws, or '/' would be treated as whitespace).
    assert_eq!(html_to_markdown("<i/ >x"), "__x");
    assert_eq!(html_to_markdown("<i />x"), "__x");
}

#[test]
fn nested_pre_keeps_outer_buffer() {
    // start_tag L290 `pre_depth == 0` clears the pre buffer only on the OUTER <pre>.
    // Mutating to `!= 0` would clear on the INNER <pre>, dropping the "a" already
    // buffered, yielding "b" instead of "ab".
    assert_eq!(html_to_markdown("<pre>a<pre>b</pre></pre>"), "```\nab\n```");
}

#[test]
fn blockquote_and_table_close_blank_line() {
    // end_tag L349 `blockquote || table` close emits a blank line. Mutating `||`->`&&`
    // makes the condition unreachable, so the trailing block separation disappears.
    assert_eq!(
        html_to_markdown("<blockquote>q</blockquote>after"),
        "> q\n\nafter"
    );
    assert_eq!(
        html_to_markdown("<table><tr><td>a</td></tr></table>after"),
        "a\n\nafter"
    );
}

#[test]
fn ordered_list_items_are_numbered() {
    // start_list_item L364 `Some(ListKind::Ordered { next })` arm: deleting it falls
    // through to the unordered "- " marker, so the numbers would vanish.
    assert_eq!(
        html_to_markdown("<ol><li>a</li><li>b</li></ol>"),
        "1. a\n2. b"
    );
}

#[test]
fn newline_in_inline_code_becomes_space() {
    // push_text L395 `c == '\n' || c == '\r'` collapses code-buffer line breaks to a
    // space. Mutating `||`->`&&` makes the guard unreachable, leaking a raw newline.
    assert_eq!(html_to_markdown("<code>a\nb</code>"), "`a b`");
    assert_eq!(html_to_markdown("<code>a\rb</code>"), "`a b`");
}

#[test]
fn trailing_space_trimmed_before_inline_close() {
    // trim_trailing_inline L441 pops trailing spaces before emitting a closing marker.
    // Mutating it to a no-op leaves the space inside the emphasis/strong markers.
    assert_eq!(html_to_markdown("<em>x </em>y"), "_x_ y");
    assert_eq!(html_to_markdown("<strong>x </strong>y"), "**x** y");
}

#[test]
fn inline_code_edge_backtick_is_padded() {
    // flush_inline_code L449 `starts_with('`') || ends_with('`')` adds edge spaces when
    // the content touches a backtick. Mutating `||`->`&&` drops the space when only one
    // edge has a backtick.
    assert_eq!(html_to_markdown("<code>`x</code>tail"), "`` `x ``tail");
    assert_eq!(html_to_markdown("<code>x`</code>tail"), "`` x` ``tail");
}

#[test]
fn attr_lookup_terminates_when_attr_absent() {
    // attr_value L518 `while pos < attrs.len()` must use `<` not `<=`; with `<=` the
    // scan loops forever at pos == len when the wanted attr (here `alt`) is absent.
    assert_eq!(html_to_markdown("<img src=a>"), "");
    assert_eq!(html_to_markdown("<img src=a width=10>"), "");
}

#[test]
fn unquoted_attr_value_stops_at_whitespace() {
    // read_attr_value L552 `end += 1` index math + L562 `is_ascii_whitespace() || b == '/'`
    // terminator. Mutating `||`->`&&` would swallow following attributes into the href;
    // mutating the `+=` underflows/loops. The href must stop at the space before `bar`.
    assert_eq!(html_to_markdown("<a href=foo bar=baz>t</a>"), "[t](<foo>)");
    assert_eq!(
        html_to_markdown("<a href=mailto:a@b.com>t</a>"),
        "[t](<mailto:a@b.com>)"
    );
}

#[test]
fn link_destination_escapes_gt_and_backslash() {
    // safe_link_destination L588 `'>' | '\\'` arm escapes those characters inside the
    // emitted <...> destination. Deleting the arm leaks an unescaped '>' / '\'.
    assert_eq!(
        html_to_markdown(r#"<a href="http://e.com/a>b">t</a>"#),
        "[t](<http://e.com/a\\>b>)"
    );
    assert_eq!(
        html_to_markdown(r#"<a href="http://e.com/a\b">t</a>"#),
        "[t](<http://e.com/a\\\\b>)"
    );
}

#[test]
fn no_space_inserted_after_open_bracket() {
    // needs_space_before L629: returns false after '[' (and '\n',' ','\t','>','('), so a
    // pending space at the start of link text is dropped. Mutating the body to always
    // `true` would inject a leading space inside the `[...]`.
    assert_eq!(
        html_to_markdown("<a href=http://e.com> x</a>"),
        "[x](<http:>)"
    );
}
