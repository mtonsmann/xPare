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
