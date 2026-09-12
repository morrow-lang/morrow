use fern_compiler::{check, lowering, parse};

#[test]
fn typed_string_function_reaches_qbe() {
    let source =
        "fn greeting() -> String:\n    \"Hello, Fern!\"\nfn main():\n    println(greeting())\n";
    let syntax = parse::parse(source).expect("independent parser");
    let typed = check::check(&syntax).expect("typed IR");
    let il = lowering::emit(&typed).expect("QBE lowering");
    assert!(il.contains("fern_println_str"));
    assert!(!il.contains("fern_println_int"));
}

#[test]
fn string_addition_preserves_checked_type_through_lowering() {
    let source = "fn suffix() -> String:\n    \"Fern\"\nfn main():\n    println(\"Hello, \" + suffix() + \"!\")\n";
    let typed = check::check(&parse::parse(source).unwrap()).unwrap();
    let il = lowering::emit(&typed).expect("String Add accepted by checker must lower");
    assert_eq!(il.matches("call $fern_str_concat").count(), 2);
    assert!(il.contains("call $fern_println_str(l %"), "{il}");
}

#[test]
fn every_supported_binary_operand_combination_reaches_lowering() {
    let cases = [
        ("3", "2", "+"),
        ("3", "2", "-"),
        ("3", "2", "*"),
        ("3", "2", "/"),
        ("3", "2", "%"),
        ("3", "2", "=="),
        ("3", "2", "!="),
        ("3", "2", "<"),
        ("3", "2", "<="),
        ("3", "2", ">"),
        ("3", "2", ">="),
        ("true", "false", "=="),
        ("true", "false", "!="),
        ("true", "false", "and"),
        ("true", "false", "or"),
        ("\"a\"", "\"b\"", "+"),
        ("\"a\"", "\"b\"", "=="),
        ("\"a\"", "\"b\"", "!="),
    ];
    for (left, right, op) in cases {
        let source = format!("fn main():\n    println({left} {op} {right})\n");
        let typed = check::check(&parse::parse(&source).unwrap()).unwrap();
        lowering::emit(&typed).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
}
