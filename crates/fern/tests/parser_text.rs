use fern_compiler::{format, parse};

#[test]
fn multiline_strings_unicode_names_and_nested_comments_roundtrip() {
    for source in [
        "fn café(π: Int) -> Int: π + 1\nfn main(): println(café(41))\n",
        "fn 🌿(value: Int) -> Int: value\nfn main(): println(🌿(42))\n",
        "/* outer\n /* inner */\n*/\nfn main():\n    /* block\nno indentation here */\n    println(1 /* between */ + 2)\n",
        "fn main():\n    let text = \"\"\"\n  🌿\n    indented\n\"\"\"\n    println(text)\n",
        "fn main(): println(\"\"\"hello\n{1 + /* expression */ 2}\n\\{literal\\}\"\"\")\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn documentation_is_associated_with_the_following_declaration() {
    let source = "@doc \"\"\"\nA literal {example}.\n# Examples\nanswer() # => 42\n\"\"\"\npub fn answer() -> Int: 42\n@doc \"\"\"A box.\"\"\"\ntype Box:\n    value: Int\nfn main(): println(answer())\n";
    let canonical = format::format(source).unwrap();
    assert_eq!(format::format(&canonical).unwrap(), canonical);
}

#[test]
fn unterminated_text_comments_docs_and_nesting_fail_cleanly() {
    for source in [
        "/* never closed",
        "fn main(): \"\"\"never closed",
        "@doc \"\"\"orphan\"\"\"",
        "@doc \"ordinary\"\nfn main(): ()",
        "fn main(): \"ordinary\nstring\"\n",
    ] {
        assert!(parse::parse(source).is_err(), "{source}");
    }
    let source = format!("{}{}fn main(): ()\n", "/*".repeat(140), "*/".repeat(140));
    assert!(parse::parse(&source).unwrap_err().message.contains("limit"));
}

#[test]
fn literal_metadata_and_multiline_bytes_keep_exact_source_locations() {
    use fern_compiler::ast::{ExprKind, StringPart};
    let source = "@doc \"\"\"doc {literal}\r\n  next\"\"\"\r\nfn text() -> String: \"\"\"\r\n  🌿\n\"\"\"\nfn main(): println(text())\n";
    let program = parse::parse(source).unwrap();
    assert_eq!(program.docs[0].target, "text");
    assert_eq!(program.docs[0].text, "doc {literal}\r\n  next");
    assert!(source[program.docs[0].span.start..program.docs[0].span.end].starts_with("@doc"));
    let value = &program.functions[0].body;
    let ExprKind::MultilineString(parts) = &value.kind else {
        panic!("{value:?}")
    };
    assert!(matches!(&parts[..], [StringPart::Text(text)] if text == "\r\n  🌿\n"));
    assert_eq!(
        &source[value.span.start..value.span.end],
        "\"\"\"\r\n  🌿\n\"\"\""
    );
}

#[test]
fn comments_and_triple_strings_in_interpolation_preserve_semantics() {
    use fern_compiler::{check, lowering};
    for source in [
        "/* before */\n@doc \"\"\"# Literal docs\n{example}\"\"\" # attached\nfn value() -> String: \"\"\"quotes \" and \\\"\\\"\\\"\n{\"\"\"nested\ntext\"\"\"}\"\"\"\n/* after */\nfn main(): println(value())\n",
        "fn main(): println(\"\"\"{1 + # inside hole\n2}\"\"\") # end\n",
        "fn main():\n    let value = 1 /* multiline\ncomment */ + 2\n    println(value)\n",
    ] {
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
        let emit = |text: &str| {
            lowering::emit(&check::check(&parse::parse(text).unwrap()).unwrap()).unwrap()
        };
        assert_eq!(emit(source), emit(&formatted));
    }
}

#[test]
fn text_prefixes_and_deep_interpolation_return_located_errors_without_panics() {
    let source = "/* outer /* nested */ */\n@doc \"\"\"🌿 {literal}\n# example\"\"\"\npub fn café() -> String: \"\"\"🌿\n{\"value {42}\"}\n\"\"\"\nfn main(): println(café())\n";
    for (end, _) in source
        .char_indices()
        .chain(std::iter::once((source.len(), ' ')))
    {
        if let Err(error) = parse::parse(&source[..end]) {
            assert!(
                error.span.start <= error.span.end && error.span.end <= end,
                "{error:?}"
            );
        }
    }
    let mut value = "42".to_owned();
    for _ in 0..140 {
        value = format!("\"\"\"{{{value}}}\"\"\"");
    }
    let error = parse::parse(&format!("fn main(): {value}\n")).unwrap_err();
    assert!(error.message.contains("limit"), "{error:?}");
    let excessive = format!("fn main(): \"\"\"{}\"\"\"", "x".repeat(1024 * 1024));
    assert!(
        parse::parse(&excessive)
            .unwrap_err()
            .message
            .contains("size")
    );
}
