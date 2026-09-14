use morrow_compiler::{format, parse};

#[test]
fn with_multiline_inline_nested_and_missing_else_roundtrip() {
    for source in [
        "fn work() -> Int:\n    with\n        first <- load(),\n        second <- convert(first),\n    do\n        second\n    else\n        Err(AuthError(message)) -> 1\n        Err(FileError(code)) if code > 0 -> 2\n        _ -> 3\n",
        "fn work() -> Result(Int, String):\n    with\n        value <- load()\n    do\n        Ok(value)\n",
        "fn work() -> Int: with value <- load() do value else Err(_) -> 0\n",
        "fn work() -> Int:\n    with\n        value <- load()\n    do\n        with\n            result <- convert(value)\n        do\n            result\n        else\n            Err(_) -> 1\n    else\n        Err(_) -> 0\n",
    ] {
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn for_loops_ranges_and_enumeration_roundtrip() {
    for source in [
        "fn main():\n    for number in 0..=3:\n        continue if number == 1\n        defer println(number)\n        break if number == 2\n        println(number)\n    println(42)\n",
        "fn main():\n    for (key, value) in %{\"a\": 1, \"b\": 2}:\n        println(\"{key}: {value}\")\n    for (index, item) in List.enumerate([1, 2]): println(index + item)\n",
        "fn main():\n    let items = [41, 42]\n    for (index, item) in items.enumerate():\n        println(index + item)\n",
        "fn main():\n    let range: Range = -2..=2\n    for item in range:\n        for inner in item..item + 2:\n            break if inner > 0\n        continue\n",
        "fn main():\n    let bounds = (1, 3)\n    for item in bounds.0..bounds.1: println(item)\n    println(1.5 + 2.0)\n",
    ] {
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn malformed_with_ranges_and_for_syntax_is_bounded() {
    for source in [
        "fn main(): x <- load()\n",
        "fn main(): with x = load() do x\n",
        "fn main():\n    with\n        x <- load()\n    else\n        _ -> 0\n",
        "fn main(): with x <- load(), do x\n",
        "fn main(): for item [1, 2]: println(item)\n",
        "fn main(): for item in [1, 2] println(item)\n",
        "fn main(): 1..2..3\n",
        "fn main(): 1..\n",
        "fn main(): ..10\n",
        "fn main(): let range: Range(Int) = 1..2\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.span.start <= source.len(), "{source}: {error:?}");
    }
    let source = format!("fn main(): {}()\n", "for item in []: ".repeat(140));
    assert!(parse::parse(&source).unwrap_err().message.contains("depth"));
}

#[test]
fn loop_fallback_lambda_starts_a_new_statement_after_dedent() {
    use morrow_compiler::ast::{ExprKind, Stmt};
    let source = "fn choose() -> () -> Int:\n    for value in 0..3:\n        return () -> value\n    () -> 0\n";
    let program = parse::parse(source).unwrap();
    let ExprKind::Block(statements) = &program.functions[0].body.kind else {
        panic!()
    };
    assert_eq!(statements.len(), 2);
    assert!(matches!(&statements[0],Stmt::Expr(value) if matches!(value.kind,ExprKind::For{..})));
    assert!(
        matches!(&statements[1],Stmt::Expr(value) if matches!(value.kind,ExprKind::Lambda{..}))
    );
    let canonical = format::format(source).unwrap();
    assert_eq!(format::format(&canonical).unwrap(), canonical);
}

#[test]
fn block_callback_iterables_and_with_initializers_roundtrip() {
    for source in [
        "fn main():\n    for value in List.map([1, 2],\n        (item) ->\n            item + 1\n    ):\n        println(value)\n",
        "fn main():\n    with\n        value <- apply(\n            () ->\n                Ok(1)\n        ),\n        next <- load(value)\n    do\n        println(next)\n    else\n        Err(_) -> ()\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}
