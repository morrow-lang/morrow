use fern_compiler::documentation::{Output, SourceDocument, render_project};

#[test]
fn project_output_orders_paths_and_keeps_same_named_declarations_distinct() {
    let documents = [
        SourceDocument {
            path: "z/math.fn",
            source: "@doc \"\"\"Last module.\"\"\"\nfn same(): ()\n",
        },
        SourceDocument {
            path: "a/math.fn",
            source: "@doc \"\"\"First module.\"\"\"\nfn same(): ()\n",
        },
    ];
    let text = render_project(&documents, "Fern library", Output::Html).unwrap();
    assert!(text.find("a/math.fn").unwrap() < text.find("z/math.fn").unwrap());
    assert!(text.contains("id=\"declaration-0\""));
    assert!(text.contains("<h3>same</h3>"));
    assert!(text.contains("id=\"declaration-1\""));
    assert!(text.contains("href=\"#module-0\""));
    assert!(text.contains("href=\"#module-1\""));
    assert!(text.contains("aria-label=\"Modules\""));
    assert!(text.contains("browser’s Find command"));
    assert!(text.contains("<kbd>Ctrl+F</kbd>"));
    assert!(text.contains("<kbd>Command+F</kbd>"));
    assert!(text.contains("First module."));
    assert!(text.contains("Last module."));
    assert!(!text.contains("<script"));
    assert!(!text.contains("<input"));
    assert!(!text.contains(" hidden"));
    assert_eq!(text.matches("<!doctype html>").count(), 1);
    let markdown = render_project(&documents, "Fern library", Output::Markdown).unwrap();
    assert!(markdown.contains(r"a/math\.fn"));
    assert!(markdown.contains("First module."));
    assert!(markdown.contains("### same"));
    assert!(!markdown.contains("<script>"));
}

#[test]
fn project_content_stays_literal_and_empty_modules_remain_navigable() {
    let documents = [
        SourceDocument {
            path: "</script><img>.fn",
            source: "@doc \"\"\"</script><img src=x onerror=evil()>\"\"\"\nfn same(): ()\n",
        },
        SourceDocument {
            path: "empty.fn",
            source: "# An empty module.\n",
        },
    ];
    let text = render_project(&documents, "<title>", Output::Html).unwrap();
    assert!(text.contains("&lt;/script&gt;&lt;img&gt;.fn"));
    assert!(text.contains("&lt;img src=x onerror=evil()&gt;"));
    assert!(!text.contains("<img"));
    assert!(text.contains("empty.fn"));
    assert!(!text.contains("<script"));
    assert!(!text.contains(" hidden"));
    assert!(text.contains("id=\"module-1\""));
}

#[test]
fn project_rejects_ambiguous_paths_bad_sources_and_resource_excess() {
    let one = SourceDocument {
        path: "one.fn",
        source: "fn one(): ()\n",
    };
    assert!(render_project(&[], "Empty", Output::Html).is_err());
    assert!(render_project(&[one, one], "Duplicate", Output::Html).is_err());
    assert!(render_project(&[one; 257], "Too many", Output::Html).is_err());
    let bad = SourceDocument {
        path: "bad.fn",
        source: "fn bad(:\n",
    };
    let error = render_project(&[one, bad], "Bad", Output::Html).unwrap_err();
    assert!(error.message.contains("bad.fn"));
    let large = "#".repeat(1024 * 1024);
    let documents: Vec<_> = (0..9)
        .map(|_| SourceDocument {
            path: "file.fn",
            source: &large,
        })
        .collect();
    assert!(render_project(&documents, "Large", Output::Html).is_err());
}
