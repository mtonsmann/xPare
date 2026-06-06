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

proptest! {
    #[test]
    fn arbitrary_input_is_deterministic_and_panic_free(s in ".*") {
        let once = html_to_markdown(&s);
        let twice = html_to_markdown(&s);
        prop_assert_eq!(once, twice);
    }
}
