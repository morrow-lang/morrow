use fern_compiler::{format, parse};

#[test]
fn returns_postfix_guards_and_defer_parse_and_format() {
    for source in [
        "fn choose(value: Int) -> Int:\n    return value + 1 if value > 0\n    return 0\nfn main(): println(choose(41))\n",
        "fn main():\n    defer println(\"🌿 cleanup\")\n    println(\"body\") if not false\n    return ()\n",
        "fn main(): defer println(\"cleanup\")\n",
        "fn choose(value: Int) -> Int: if value > 0: return value else: return 0\nfn main(): ()\n",
        "fn main():\n    let callback = (value: Int) ->\n        defer println(\"cleanup\")\n        return value\n    println(callback(42))\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn condition_matches_and_let_else_keep_nested_layout() {
    for source in [
        "fn choose(value: Int) -> String:\n    match:\n        value < 0 -> \"negative\"\n        (value > 0) -> return \"positive\"\n        _ -> \"zero\"\nfn main(): println(choose(1))\n",
        "fn unwrap(value: Option(Int)) -> Int:\n    let Some(number): Option(Int) = value else:\n        println(\"missing\")\n        return 0\n    number\nfn main(): println(unwrap(Some(42)))\n",
        "fn unwrap(value: Result((Int, Int), String)) -> Int:\n    let Ok((left, right)) = value else: return 0\n    left + right\nfn main(): ()\n",
        "fn main():\n    match:\n        test((x: Int) -> x) ->\n            match Some(1):\n                Some(n) -> println(n)\n                None -> return ()\n        _ -> println(0)\n    println(42)\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn malformed_control_syntax_is_bounded_and_located() {
    for source in [
        "fn main(): return\n",
        "fn main(): defer\n",
        "fn main(): println(defer println(1))\n",
        "fn main(): println(1) if\n",
        "fn main():\n    let Some(value) = None else\n        return ()\n",
        "fn main():\n    match:\n        _ -> ()\n        true -> ()\n",
        "fn main():\n    match:\n        true ()\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.span.start <= source.len(), "{source}: {error:?}");
    }
    let source = format!("fn main(): {}42\n", "return ".repeat(140));
    assert!(parse::parse(&source).unwrap_err().message.contains("limit"));
}

#[test]
fn postfix_guard_wraps_return_and_does_not_consume_a_following_if() {
    use fern_compiler::ast::{ExprKind, Stmt};
    let source = "fn choose(value: Int) -> Int:\n    return value + 1 if value > 0\n    if value == 0:\n        println(0)\n    if value < 0:\n        return 0\n    return 1\n";
    let program = parse::parse(source).unwrap();
    let ExprKind::Block(statements) = &program.functions[0].body.kind else {
        panic!()
    };
    assert_eq!(statements.len(), 4);
    let Stmt::Expr(first) = &statements[0] else {
        panic!()
    };
    let ExprKind::PostfixIf { value, condition } = &first.kind else {
        panic!()
    };
    assert!(matches!(value.kind, ExprKind::Return(_)));
    assert!(matches!(condition.kind, ExprKind::Binary { .. }));
    assert!(matches!(&statements[1],Stmt::Expr(value) if matches!(value.kind,ExprKind::If{..})));
    assert!(matches!(&statements[2],Stmt::Expr(value) if matches!(value.kind,ExprKind::If{..})));
    let formatted = format::format(source).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
}

#[test]
fn control_comments_and_unicode_prefixes_are_preserved_without_panics() {
    let source = "fn choose(value: Option(Int)) -> Int:\n    defer println(\"🌿 cleanup\") # defer\n    let Some(number) = value else: # failure\n        return 0 # return\n    match: # conditions\n        number > 0 -> return number # positive\n        _ -> 0 # default\n";
    let canonical = format::format(source).unwrap();
    assert_eq!(format::format(&canonical).unwrap(), canonical);
    for comment in [
        "# defer",
        "# failure",
        "# return",
        "# conditions",
        "# positive",
        "# default",
    ] {
        assert_eq!(
            canonical
                .lines()
                .filter(|line| line.ends_with(comment))
                .count(),
            1
        );
    }
    for end in (0..source.len()).filter(|end| source.is_char_boundary(*end)) {
        let _ = parse::parse(&source[..end]);
    }
}
