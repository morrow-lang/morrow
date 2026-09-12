use fern_compiler::{check, format, lowering, parse};
fn compile(source: &str) -> String {
    lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap()
}

#[test]
fn interpolates_scalars_and_arithmetic_with_original_spans() {
    let source = "fn main():\n    let name = \"🌿\"\n    println(\"Hello {name}: {40 + 2} {true} {1.25}\")\n";
    let il = compile(source);
    assert!(il.contains("fern_int_to_str"));
    assert!(il.contains("fern_bool_to_str"));
    // Both backends now use the fixed Float ABI; variadic calls stay in the runtime.
    assert!(il.contains("call $fern_float_to_str(d "));
    assert!(!il.contains("snprintf"));
    let source = "fn main(): println(\"🌿 {missing}\")\n";
    let error = check::check(&parse::parse(source).unwrap()).unwrap_err();
    assert_eq!(&source[error.span.start..error.span.end], "missing");
}

#[test]
fn nested_strings_brace_escapes_and_comment_characters_roundtrip() {
    let source = r#"fn main():
    let number = 42
    println("literal \{brace\}; {"inner {number} # 🌿"}") # actual comment
"#;
    let formatted = format::format(source).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    assert_eq!(compile(source), compile(&formatted));
    assert_eq!(formatted.matches("# actual comment").count(), 1);
}

#[test]
fn malformed_and_non_scalar_interpolation_is_rejected() {
    for value in [
        "\"{\"",
        "\"{}\"",
        "\"bad }\"",
        "\"{1 2}\"",
        "\"{[1, 2]}\"",
        "\"{(1, 2)}\"",
        "\"{Some(1)}\"",
    ] {
        let source = format!("fn main(): println({value})\n");
        assert!(
            parse::parse(&source)
                .and_then(|p| check::check(&p))
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn interpolation_arguments_run_once_in_source_order() {
    let source = "fn item(n: Int) -> Int:\n    println(n)\n    n\nfn main():\n    println(\"{item(1)}:{item(2)}\")\n";
    let il = compile(source);
    assert_eq!(il.matches("call $f0(").count(), 2);
}

#[test]
fn interpolation_depth_and_token_counts_are_bounded() {
    let mut value = "1".to_owned();
    for _ in 0..140 {
        value = format!("\"{{{value}}}\"");
    }
    assert!(
        parse::parse(&format!("fn main(): {value}\n"))
            .unwrap_err()
            .message
            .contains("limit")
    );
    let source = format!("fn main(): \"{}\"\n", "{1 + 1}".repeat(20_000));
    assert!(parse::parse(&source).unwrap_err().message.contains("limit"));
}

#[test]
fn formatter_preserves_literal_braces_without_creating_interpolation() {
    let source = r#"fn main(): println("\{literal\} {"{42}"}")
"#;
    let formatted = format::format(source).unwrap();
    assert_eq!(compile(source), compile(&formatted));
    assert!(formatted.contains("\\{literal\\}"));
}

#[test]
fn interpolation_visitors_preserve_generics_imports_and_error_handling() {
    compile(
        "fn describe(value: a) -> String: \"value={value}\"\nfn main(): println(describe(42))\n",
    );
    let source = "fn value() -> Result(String, String):\n    let result: Result(Int, String) = Ok(42)\n    Ok(\"number={result?}\")\nfn main():\n    println(Result.unwrap_or(value(), \"error\"))\n";
    compile(source);
    let source =
        "fn main():\n    let result: Result(Int, String) = Ok(42)\n    println(\"{result}\")\n";
    assert!(
        check::check(&parse::parse(source).unwrap())
            .unwrap_err()
            .message
            .contains("interpolation")
    );
}

#[test]
fn every_unicode_source_prefix_returns_without_panicking() {
    let source = r#"fn main(): println("🌿 {"nested {1 + 2} #"} \{literal\}")"#;
    for end in (0..=source.len()).filter(|at| source.is_char_boundary(*at)) {
        let _ = parse::parse(&source[..end]);
    }
}
