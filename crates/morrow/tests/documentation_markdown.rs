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
    assert!(html.contains("<img src=\"logo.png\" alt=\"logo\">"));
    let html = render("![x](javascript:alert(1)) ![y](data:text/html,hi) ![z](vbscript:x)\n");
    assert!(!html.contains("<img") && !html.contains("javascript:") && !html.contains("data:"));
    assert!(html.contains("x y z"));
}

#[test]
fn unsafe_rewritten_destinations_fall_back_to_the_safe_source() {
    let options = Options {
        link: &|_: &str| Some("javascript:alert(1)".to_string()),
        ..Options::default()
    };
    let html = markdown::render(
        "[guide](GUIDE.md) ![logo](logo.png) <a href=\"raw.html\">raw</a>\n",
        &options,
    )
    .unwrap()
    .html;
    assert!(!html.contains("javascript:"), "{html}");
    assert!(html.contains("<a href=\"GUIDE.md\">guide</a>"), "{html}");
    assert!(
        html.contains("<img src=\"logo.png\" alt=\"logo\">"),
        "{html}"
    );
    assert!(html.contains("<a href=\"raw.html\">raw</a>"), "{html}");
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
fn raw_html_passes_through_an_allowlist_and_stays_balanced() {
    let html = render(
        "<p align=\"center\"><strong>Readable.</strong><br/>\nNative.</p>\n\nAfter <kbd>Ctrl</kbd> text.\n\n<details open>\n<summary>More</summary>\n\nHidden **content**.\n\n</details>\n",
    );
    assert!(html.contains("<p align=\"center\"><strong>Readable.</strong><br>\nNative.</p>"));
    assert!(html.contains("<p>After <kbd>Ctrl</kbd> text.</p>"));
    assert!(html.contains("<details open>\n<summary>More</summary>"));
    assert!(html.contains("<p>Hidden <strong>content</strong>.</p>\n</details>"));
    // Scripts, styles, event handlers, unsafe URLs and unknown tags never survive.
    let html = render(
        "<script>alert(1)</script><div onclick=\"x()\" style=\"color:red\" class=\"note\" id=\"n1\">ok <a href=\"javascript:alert(1)\" title=\"t\">bad</a> <img src=\"https://x.y/a.png\" alt=\"A\" onerror=\"z()\"> <marquee>m</marquee></div>\n",
    );
    assert!(!html.contains("<script"));
    assert!(html.contains("alert(1)"));
    assert!(html.contains("<div class=\"note\" id=\"n1\">ok <a title=\"t\">bad</a>"));
    assert!(html.contains("<img src=\"https://x.y/a.png\" alt=\"A\">"));
    assert!(!html.contains("onclick") && !html.contains("style=") && !html.contains("onerror"));
    assert!(!html.contains("<marquee") && html.contains("m</div>"));
    let html = render(
        "<img src=\"javascript:alert(1)\" alt=\"J\"><img src=\"data:text/html,x\" alt=\"D\"><img src=\"safe.png\" alt=\"S\">\n",
    );
    assert_eq!(
        html,
        "<img alt=\"J\"><img alt=\"D\"><img src=\"safe.png\" alt=\"S\">\n"
    );
    // Unclosed and mismatched tags are repaired so one guide cannot break the page shell.
    let html = render("<div><span>text</b>\n\nafter\n");
    assert!(html.ends_with("<p>after</p>\n</span></div>\n"), "{html}");
    let html = render("text</div> more\n");
    assert_eq!(html, "<p>text more</p>\n");
    // Relative hrefs inside raw HTML follow the same rewrite as Markdown links.
    let options = Options {
        link: &|target: &str| target.strip_suffix(".md").map(|s| format!("{s}.html")),
        ..Options::default()
    };
    let html = markdown::render("<a href=\"GUIDE.md#part\">g</a>\n", &options)
        .unwrap()
        .html;
    assert!(html.contains("<a href=\"GUIDE.html#part\">g</a>"));
}

#[test]
fn footnotes_render_in_reference_order_with_backlinks() {
    let rendered = markdown::render(
        "First[^a] and second[^b] and again[^a]; missing[^zz].\n\n[^b]: Second note with `code`.\n[^a]: First note.\n    Continued line.\n\n    Second paragraph.\n\nTail.\n",
        &Options {
            id_prefix: "x-",
            ..Options::default()
        },
    )
    .unwrap();
    let html = rendered.html;
    assert!(html.contains(
        "First<sup class=\"footnote-ref\" id=\"x-fnref-a\"><a href=\"#x-fn-a\">1</a></sup>"
    ));
    assert!(html.contains(
        "second<sup class=\"footnote-ref\" id=\"x-fnref-b\"><a href=\"#x-fn-b\">2</a></sup>"
    ));
    assert!(html.contains("again<sup class=\"footnote-ref\"><a href=\"#x-fn-a\">1</a></sup>"));
    assert!(html.contains("missing[^zz]."));
    let notes = html.find("<section class=\"footnotes\">").unwrap();
    assert!(html[notes..].contains("<ol>\n<li id=\"x-fn-a\"><p>First note.\nContinued line.</p>\n<p>Second paragraph. <a href=\"#x-fnref-a\" class=\"footnote-backref\" aria-label=\"Back to reference 1\">↩</a></p>\n</li>"));
    assert!(html[notes..].contains(
        "<li id=\"x-fn-b\"><p>Second note with <code>code</code>. <a href=\"#x-fnref-b\""
    ));
    assert!(html.find("<p>Tail.</p>").unwrap() < notes);
    assert!(!html.contains("[^b]:"));
}

#[test]
fn reference_links_resolve_case_insensitively_and_unknown_labels_stay_literal() {
    let html = render(
        "See [the guide][Guide], [Guide][] and [guide]; also ![logo][img] but [nope][missing] and [alone].\n\n[guide]: https://example.com/g \"Guide title\"\n[IMG]: https://example.com/l.png\n",
    );
    assert!(html.contains("<a href=\"https://example.com/g\" title=\"Guide title\">the guide</a>"));
    assert!(html.contains("<a href=\"https://example.com/g\" title=\"Guide title\">Guide</a>"));
    assert!(html.contains("<a href=\"https://example.com/g\" title=\"Guide title\">guide</a>"));
    assert!(html.contains("<img src=\"https://example.com/l.png\" alt=\"logo\">"));
    assert!(html.contains("[nope][missing] and [alone]."));
    assert!(!html.contains("[guide]:"));
}

#[test]
fn images_alerts_task_lists_setext_headings_and_strikethrough() {
    let html = render("![Alt *text*](https://x.y/p.png \"Title\") and ![](rel/p.png)\n");
    assert!(html.contains("<img src=\"https://x.y/p.png\" alt=\"Alt text\" title=\"Title\">"));
    assert!(html.contains("<img src=\"rel/p.png\" alt=\"\">"));
    let html = render(
        "> [!WARNING]\n> Careful **here**.\n>\n> Second.\n\n> [!Tip]\n> Small.\n\n> plain\n",
    );
    assert!(html.contains(
        "<div class=\"alert alert-warning\"><p class=\"alert-title\">Warning</p>\n<p>Careful <strong>here</strong>.</p>\n<p>Second.</p>\n</div>"
    ));
    assert!(html.contains(
        "<div class=\"alert alert-tip\"><p class=\"alert-title\">Tip</p>\n<p>Small.</p>\n</div>"
    ));
    assert!(html.contains("<blockquote>\n<p>plain</p>\n</blockquote>"));
    let html = render("- [ ] open\n- [x] done `x`\n- plain\n");
    assert!(html.contains("<li class=\"task\"><input type=\"checkbox\" disabled> open</li>"));
    assert!(html.contains(
        "<li class=\"task\"><input type=\"checkbox\" disabled checked> done <code>x</code></li>"
    ));
    assert!(html.contains("<li>plain</li>"));
    let rendered = markdown::render(
        "Title line\n==========\n\nSub *title*\n---\n\ntext\n\n---\n",
        &Options::default(),
    )
    .unwrap();
    assert!(
        rendered
            .html
            .contains("<h1 id=\"title-line\">Title line</h1>")
    );
    assert!(
        rendered
            .html
            .contains("<h2 id=\"sub-title\">Sub <em>title</em></h2>")
    );
    assert!(rendered.html.contains("<p>text</p>\n<hr>"));
    assert_eq!(rendered.headings.len(), 2);
    let html = render("~~gone~~ stays ~single~ and a ~~b~~c.\n");
    assert!(html.contains("<del>gone</del> stays ~single~ and a <del>b</del>c."));
}

#[test]
fn tight_lists_omit_paragraph_wrappers_and_loose_lists_keep_them() {
    let html = render("- a\n  - nested\n- b\n\ntext\n\n1. one\n\n2. two\n");
    assert!(
        html.contains("<li>a\n<ul>\n<li>nested</li>\n</ul>\n</li>\n<li>b</li>"),
        "{html}"
    );
    assert!(
        html.contains("<ol>\n<li><p>one</p>\n</li>\n<li><p>two</p>\n</li>\n</ol>"),
        "{html}"
    );
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

/// A loose task item wraps its text in a paragraph, so a checkbox emitted before that
/// paragraph renders on a line of its own above the text it is supposed to label.
#[test]
fn loose_task_items_keep_their_checkbox_inside_the_first_paragraph() {
    let html = render("- [x] done\n\n- [ ] open\n\n  more\n");
    assert!(
        html.contains(
            "<li class=\"task\"><p><input type=\"checkbox\" disabled checked> done</p>\n</li>"
        ),
        "{html}"
    );
    assert!(
        html.contains(
            "<li class=\"task\"><p><input type=\"checkbox\" disabled> open</p>\n<p>more</p>\n</li>"
        ),
        "{html}"
    );
}
