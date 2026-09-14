//! The documentation Markdown renderer produces escaped, bounded HTML with safe links.
use morrow_compiler::documentation::markdown::{self, Options};

fn render(text: &str) -> String {
    markdown::render(text, &Options::default()).unwrap().html
}

#[test]
fn renders_block_structure_with_shifted_headings_and_ids() {
    let text = "# Title\n\nIntro paragraph\nspanning lines.\n\n## Usage\n\n- one\n- two `x`\n  continued\n\n1. first\n2. second\n\n> quoted\n> text\n\n---\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n";
    let options = Options {
        base_level: 3,
        ..Options::default()
    };
    let rendered = markdown::render(text, &options).unwrap();
    let html = &rendered.html;
    assert!(html.contains("<h3 id=\"title\">Title</h3>"));
    assert!(html.contains("<h4 id=\"usage\">Usage</h4>"));
    assert!(html.contains("<p>Intro paragraph\nspanning lines.</p>"));
    assert!(html.contains("<ul>\n<li>one</li>\n<li>two <code>x</code>\ncontinued</li>\n</ul>"));
    assert!(html.contains("<ol>\n<li>first</li>\n<li>second</li>\n</ol>"));
    assert!(html.contains("<blockquote>\n<p>quoted\ntext</p>\n</blockquote>"));
    assert!(html.contains("<hr>"));
    assert!(html.contains("<th>a</th><th style=\"text-align:center\">b</th>"));
    assert!(html.contains("<td>1</td><td style=\"text-align:center\">2</td>"));
    assert_eq!(rendered.headings.len(), 2);
    assert_eq!(rendered.headings[1].id, "usage");
    assert_eq!(rendered.headings[1].text, "Usage");
    // Deeper headings clamp at h6 and duplicate slugs receive numeric suffixes.
    let deep = render("###### A\n\n###### A\n\n# B c!\n");
    assert!(deep.contains("<h6 id=\"a\">A</h6>"));
    assert!(deep.contains("<h6 id=\"a-2\">A</h6>"));
    assert!(deep.contains("<h1 id=\"b-c\">B c!</h1>"));
}

#[test]
fn renders_inline_markup_and_escapes_html() {
    let html = render(
        "Use **bold**, *em*, _em_ and `code <b>` with a [link](https://example.com \"T\") and <https://x.y>.\n\nRaw <script>alert(1)</script> & stays text; `a && b`.\n",
    );
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<em>em</em>, <em>em</em>"));
    assert!(html.contains("<code>code &lt;b&gt;</code>"));
    assert!(html.contains("<a href=\"https://example.com\" title=\"T\">link</a>"));
    assert!(html.contains("<a href=\"https://x.y\">https://x.y</a>"));
    assert!(!html.contains("<script"));
    assert!(html.contains("alert(1)"));
    assert!(html.contains("&amp; stays text"));
    assert!(html.contains("<code>a &amp;&amp; b</code>"));
}

#[test]
fn unsafe_link_schemes_become_text_and_relative_links_can_be_rewritten() {
    let html = render("[x](javascript:alert(1)) [y](data:text/html,hi) [z](vbscript:x)\n");
    assert!(!html.contains("href"));
    assert!(html.contains("x y z"));
    let options = Options {
        link: &|target: &str| {
            target
                .strip_suffix(".md")
                .map(|stem| format!("{stem}.html"))
        },
        ..Options::default()
    };
    let html = markdown::render("[guide](GUIDE.md#part) and [same](other.html)\n", &options)
        .unwrap()
        .html;
    assert!(html.contains("<a href=\"GUIDE.html#part\">guide</a>"));
    assert!(html.contains("<a href=\"other.html\">same</a>"));
    let html = render("![logo](logo.png)\n");
    assert!(!html.contains("<img"));
    assert!(html.contains("<a href=\"logo.png\">logo</a>"));
}

#[test]
fn code_fences_keep_language_and_highlight_morrow() {
    let html = render(
        "```morrow\n@doc \"\"\"Hi.\"\"\"\npub fn add(a: Int, b: Int) -> Int: a + b  # => 3\n```\n\n```text\n<plain>\n```\n\n~~~\nno language\n~~~\n",
    );
    assert!(html.contains("<pre><code class=\"language-morrow\">"));
    assert!(html.contains(
        "<span class=\"kw\">pub</span> <span class=\"kw\">fn</span> <span class=\"fn\">add</span>("
    ));
    assert!(html.contains("<span class=\"ty\">Int</span>"));
    assert!(html.contains("<span class=\"result\"># =&gt; 3</span>"));
    assert!(html.contains("<span class=\"str\">&quot;&quot;&quot;Hi.&quot;&quot;&quot;</span>"));
    assert!(html.contains("<span class=\"attr\">@doc</span>"));
    assert!(html.contains("<pre><code class=\"language-text\">&lt;plain&gt;\n</code></pre>"));
    assert!(html.contains("<pre><code>no language\n</code></pre>"));
}

#[test]
fn legacy_fern_fences_use_the_morrow_highlighter() {
    let html = render("```fern\npub fn answer() -> Int: 42\n```\n");
    assert!(html.contains("<pre><code class=\"language-fern\">"));
    assert!(html.contains(
        "<span class=\"kw\">pub</span> <span class=\"kw\">fn</span> <span class=\"fn\">answer</span>("
    ));
    assert!(html.contains("<span class=\"ty\">Int</span>"));
}

#[test]
fn inline_code_cross_references_resolve_through_the_caller() {
    let options = Options {
        reference: &|name: &str| (name == "List.map").then(|| "list.html#map".to_string()),
        ..Options::default()
    };
    let html = markdown::render("Call `List.map` or `other`.\n", &options)
        .unwrap()
        .html;
    assert!(html.contains("<a href=\"list.html#map\"><code>List.map</code></a>"));
    assert!(html.contains(" or <code>other</code>."));
}

#[test]
fn raw_html_blocks_are_reduced_to_their_text() {
    let html = render(
        "<p align=\"center\"><strong>Readable.</strong><br>\nNative.</p>\n\nAfter <kbd>Ctrl</kbd> text.\n",
    );
    assert!(html.contains("<p>Readable.\nNative.</p>"));
    assert!(html.contains("<p>After Ctrl text.</p>"));
    assert!(!html.contains("align="));
}

#[test]
fn output_and_nesting_are_bounded() {
    let options = Options {
        limit: 64,
        ..Options::default()
    };
    let error = markdown::render(&"word ".repeat(100), &options).unwrap_err();
    assert!(error.message.contains("byte limit"));
    let nested = format!("{}deep\n", "> ".repeat(40));
    let error = markdown::render(&nested, &Options::default()).unwrap_err();
    assert!(error.message.contains("nesting"));
    let summary = markdown::summary("First sentence. Second sentence.\n\nMore.\n", 80);
    assert_eq!(summary, "First sentence.");
    let summary = markdown::summary("# Heading\n\nUses `code` and *emphasis*", 80);
    assert_eq!(summary, "Uses code and emphasis");
}
