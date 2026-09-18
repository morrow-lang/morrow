//! The documentation site renders modules and guides into a linked multi-page HTML tree.
use morrow_compiler::documentation::{
    SourceDocument,
    site::{Extra, Link, Site},
};

const MATH: &str = "module lib.math\n\n@moduledoc \"\"\"\nArithmetic helpers. See `Shape` and `lib.text.shout`.\n\n## Examples\n\n```morrow\nadd(1, 2)  # => 3\n```\n\"\"\"\n\n@doc \"\"\"Add two numbers. Second sentence.\"\"\"\npub fn add(a: Int, b: Int) -> Int: a + b\n\n@doc \"\"\"A **shape**.\"\"\"\npub type Shape:\n    Circle(Int)\n    Square(Int)\n\nfn hidden() -> Int: 1\n";
const TEXT: &str =
    "@doc \"\"\"Shout `add` loudly.\"\"\"\npub fn shout(text: String) -> String: text\n";

fn site<'a>(modules: &'a [SourceDocument<'a>], extras: &'a [Extra<'a>]) -> Site<'a> {
    Site {
        title: "Sample & Co",
        version: Some("1.2.3"),
        modules,
        schemes: None,
        extras,
        links: &[Link {
            label: "GitHub",
            url: "https://example.com/repo",
        }],
    }
}

fn page<'a>(pages: &'a [morrow_compiler::documentation::site::Page], path: &str) -> &'a str {
    let page = pages
        .iter()
        .find(|page| page.path == path)
        .unwrap_or_else(|| {
            panic!(
                "missing page {path}: {:?}",
                pages.iter().map(|p| &p.path).collect::<Vec<_>>()
            )
        });
    std::str::from_utf8(&page.contents).unwrap()
}

#[test]
fn renders_module_pages_with_summary_navigation_and_cross_references() {
    let modules = [
        SourceDocument {
            path: "lib/math.mr",
            source: MATH,
        },
        SourceDocument {
            path: "lib/text.mr",
            source: TEXT,
        },
    ];
    let pages = morrow_compiler::documentation::site::render_site(&site(&modules, &[])).unwrap();
    let math = page(&pages, "lib.math.html");
    assert!(math.starts_with("<!doctype html>"));
    assert!(math.contains("<title>lib.math — Sample &amp; Co</title>"));
    assert!(math.contains("<h1 class=\"page-title\">lib.math</h1>"));
    // Module documentation renders as Markdown with headings shifted below the page title.
    assert!(math.contains("Arithmetic helpers."));
    assert!(math.contains("<h2 id=\"examples\">Examples</h2>"));
    assert!(!math.contains("<span class=\"kw\">add</span>"));
    assert!(math.contains("<span class=\"fn\">add</span>"));
    // Cross-references resolve to local and qualified declarations.
    assert!(math.contains("<a href=\"#t:Shape\"><code>Shape</code></a>"));
    assert!(math.contains("<a href=\"lib.text.html#shout\"><code>lib.text.shout</code></a>"));
    // Summary lists public declarations grouped by kind with first-sentence summaries.
    let summary = math.find("<section class=\"summary\"").unwrap();
    let types = math[summary..].find("Types").unwrap();
    let functions = math[summary..].find("Functions").unwrap();
    assert!(types < functions);
    assert!(
        math.contains(
            "<a href=\"#add\">add</a><span class=\"summary-text\">Add two numbers.</span>"
        )
    );
    assert!(math.contains("<p>Add two numbers. Second sentence.</p>"));
    assert!(math.contains("id=\"t:Shape\""));
    assert!(math.contains("<strong>shape</strong>"));
    assert!(math.contains("<span class=\"ty\">Circle</span>(<span class=\"ty\">Int</span>)"));
    // Private declarations are documented but marked; public ones carry a badge.
    assert!(math.contains("id=\"hidden\""));
    assert!(math.contains("class=\"badge private\""));
    assert!(math.contains("class=\"badge public\""));
    // Sidebar: brand, version, search, module list with current members, links.
    assert!(math.contains("Sample &amp; Co"));
    assert!(math.contains("v1.2.3"));
    assert!(math.contains("<input id=\"search\""));
    assert!(math.contains("<li class=\"current\"><a href=\"lib.math.html\">lib.math</a>"));
    assert!(math.contains("<a href=\"lib.text.html\">lib.text</a>"));
    assert!(math.contains("<a href=\"https://example.com/repo\" rel=\"noopener\">GitHub</a>"));
    assert!(math.contains("href=\"morrow-docs.css\""));
    assert!(math.contains("src=\"morrow-search.js\""));
    // Shared assets and a JSON search index accompany the pages.
    assert!(page(&pages, "morrow-docs.css").contains("--morrow-accent"));
    assert!(page(&pages, "morrow-docs.js").contains("MORROW_SEARCH_INDEX"));
    let index = page(&pages, "morrow-search.js");
    assert!(index.starts_with("window.MORROW_SEARCH_INDEX = ["));
    assert!(index.contains("\"n\":\"add\""));
    assert!(index.contains("\"u\":\"lib.math.html#add\""));
    assert!(index.contains("\"u\":\"lib.math.html#examples\""));
    // Without a README the landing page lists modules and their summaries.
    let landing = page(&pages, "index.html");
    assert!(landing.contains("<a href=\"lib.math.html\">lib.math</a>"));
    assert!(landing.contains("Arithmetic helpers."));
}

#[test]
fn renders_guides_with_rewritten_links_and_readme_as_landing_page() {
    let extras = [
        Extra {
            path: "README.md",
            source: "<h1 align=\"center\">Sample</h1>\n\nWelcome. Read the [guide](docs/GUIDE.md#usage) and `lib.math.add`.\n",
        },
        Extra {
            path: "docs/GUIDE.md",
            source: "# Getting Started\n\n## Usage\n\nBack to the [readme](../README.md), the [source](../lib/math.mr) and [missing](OTHER.md).\n\n| Command | Purpose |\n| --- | --- |\n| `morrow doc` | docs |\n",
        },
    ];
    let modules = [SourceDocument {
        path: "lib/math.mr",
        source: MATH,
    }];
    let pages =
        morrow_compiler::documentation::site::render_site(&site(&modules, &extras)).unwrap();
    assert!(pages.iter().all(|page| page.path != "readme.html"));
    let landing = page(&pages, "index.html");
    assert!(landing.contains("<a href=\"guide.html#usage\">guide</a>"));
    assert!(landing.contains("<a href=\"lib.math.html#add\"><code>lib.math.add</code></a>"));
    assert!(!landing.contains("align="));
    let guide = page(&pages, "guide.html");
    assert!(guide.contains("<title>Getting Started — Sample &amp; Co</title>"));
    assert!(guide.contains("<a href=\"index.html\">readme</a>"));
    assert!(guide.contains("<a href=\"lib.math.html\">source</a>"));
    assert!(guide.contains("<a href=\"OTHER.md\">missing</a>"));
    assert!(guide.contains("<table>"));
    assert!(guide.contains("<li class=\"current\"><a href=\"guide.html\">Getting Started</a>"));
    assert!(guide.contains("<a href=\"#usage\">Usage</a>"));
    let index = page(&pages, "morrow-search.js");
    assert!(index.contains("\"t\":\"guide\""));
    assert!(index.contains("\"u\":\"guide.html#usage\""));
}

#[test]
fn rejects_collisions_reserved_names_and_oversized_input() {
    let modules = [SourceDocument {
        path: "guide.mr",
        source: "pub fn f() -> Int: 1\n",
    }];
    let extras = [Extra {
        path: "GUIDE.md",
        source: "# Guide\n",
    }];
    let error =
        morrow_compiler::documentation::site::render_site(&site(&modules, &extras)).unwrap_err();
    assert!(error.message.contains("guide.html"), "{}", error.message);
    let reserved = [Extra {
        path: "morrow-docs.md",
        source: "x\n",
    }];
    let error =
        morrow_compiler::documentation::site::render_site(&site(&[], &reserved)).unwrap_err();
    assert!(error.message.contains("reserved"), "{}", error.message);
    let empty = morrow_compiler::documentation::site::render_site(&site(&[], &[])).unwrap_err();
    assert!(error.message.len() < 4096 && empty.message.contains("requires"));
    let big = "x".repeat(1024 * 1024 + 1);
    let oversized = [Extra {
        path: "BIG.md",
        source: &big,
    }];
    let error =
        morrow_compiler::documentation::site::render_site(&site(&[], &oversized)).unwrap_err();
    assert!(error.message.contains("1 MiB"), "{}", error.message);
    let invalid = [SourceDocument {
        path: "bad.mr",
        source: "fn bad():\n    (\n",
    }];
    let error =
        morrow_compiler::documentation::site::render_site(&site(&invalid, &[])).unwrap_err();
    assert!(error.message.starts_with("bad.mr:"), "{}", error.message);
}

/// Every class and element the Markdown renderer emits must be styled, or a construct
/// like an alert renders indistinguishably from the paragraph beside it.
#[test]
fn the_stylesheet_styles_every_construct_the_renderer_emits() {
    let guide = "# Guide\n\n> [!WARNING]\n> Careful.\n\n- [ ] open\n- [x] done\n\n~~gone~~ and a note[^n].\n\n![Logo](logo.png)\n\n[^n]: The note.\n";
    let extras = [Extra {
        path: "GUIDE.md",
        source: guide,
    }];
    let modules = [SourceDocument {
        path: "lib/math.mr",
        source: MATH,
    }];
    let pages =
        morrow_compiler::documentation::site::render_site(&site(&modules, &extras)).unwrap();
    let css = page(&pages, "morrow-docs.css");
    let guide = page(&pages, "guide.html");
    // The page really does carry each construct, so the selectors below are not vacuous.
    for emitted in [
        "class=\"alert alert-warning\"",
        "class=\"alert-title\"",
        "class=\"task\"",
        "<del>",
        "<img",
        "class=\"footnotes\"",
        "class=\"footnote-ref\"",
        "class=\"footnote-backref\"",
    ] {
        assert!(
            guide.contains(emitted),
            "page is missing {emitted}: {guide}"
        );
    }
    for selector in [
        ".alert",
        ".alert-title",
        ".alert-note",
        ".alert-tip",
        ".alert-important",
        ".alert-warning",
        ".alert-caution",
        "li.task",
        "del",
        "img",
        ".footnotes",
        ".footnote-ref",
        ".footnote-backref",
    ] {
        assert!(
            css.contains(selector),
            "stylesheet does not style {selector}"
        );
    }
}
