use fern_compiler::{
    Type,
    ast::{BinaryOp, ExprKind, Stmt},
    parse::parse,
};

#[test]
fn parses_reference_examples() {
    for source in [
        include_str!("../../../examples/factorial.fn"),
        include_str!("../../../examples/fibonacci.fn"),
        include_str!("../../../examples/add.fn"),
        include_str!("../../../examples/conditionals.fn"),
    ] {
        assert!(parse(source).is_ok(), "{:#?}", parse(source));
    }
}

#[test]
fn preserves_types_and_precedence() {
    let p = parse("fn add(a: Int, b: Int) -> Int:\n    let x: Int = a + b * 2\n    x\n").unwrap();
    assert_eq!(p.functions[0].return_type, Some(Type::Int));
    assert_eq!(p.functions[0].params.len(), 2);
    let ExprKind::Block(stmts) = &p.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Let {
        annotation, value, ..
    } = &stmts[0]
    else {
        panic!()
    };
    assert_eq!(*annotation, Some(Type::Int));
    let ExprKind::Binary {
        op: BinaryOp::Add,
        right,
        ..
    } = &value.kind
    else {
        panic!()
    };
    assert!(matches!(
        right.kind,
        ExprKind::Binary {
            op: BinaryOp::Multiply,
            ..
        }
    ));
}

#[test]
fn strings_booleans_and_comments() {
    let p = parse("# header\nfn main():\n    # skipped\n\n    println(\"Grüße 🌿\\n\\t\\\"\\\\\") # end\n    let x = not false and true or false\n").unwrap();
    let ExprKind::Block(stmts) = &p.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Expr(call) = &stmts[0] else {
        panic!()
    };
    let ExprKind::Call { args, .. } = &call.kind else {
        panic!()
    };
    assert!(matches!(&args[0].kind, ExprKind::String(s) if s == "Grüße 🌿\n\t\"\\"));
}

#[test]
fn rejects_malformed_and_unsupported_input() {
    for (source, message) in [
        ("fn main():\n \t0", "mixed tabs and spaces"),
        ("fn main():\n    0\n  1", "indent"),
        ("fn main(): \"x\\q\"", "escape"),
        ("fn main(): \"hi {name\"", "unterminated"),
        ("fn main(): {1, 2}", "unsupported"),
        ("fn main(): 9223372036854775808", "range"),
        ("fn main(): 1.5.5", "unsupported"),
        ("fn main(): @", "unsupported"),
        ("fn main(): \"unterminated", "unterminated"),
        ("fn main():\n", "indented"),
        ("fn main(): 0 1", "end of line"),
    ] {
        let e = parse(source).unwrap_err();
        assert!(e.message.contains(message), "{source}: {}", e.message);
        assert!(e.span.start <= e.span.end && e.span.end <= source.len());
    }
}

#[test]
fn rejects_resource_exhaustion_without_panicking() {
    let huge = " ".repeat(1024 * 1024 + 1);
    assert!(parse(&huge).unwrap_err().message.contains("size"));
    let nested = format!("fn main(): {}0{}", "(".repeat(300), ")".repeat(300));
    assert!(parse(&nested).unwrap_err().message.contains("depth"));
    let chain = format!("fn main(): 0{}", "+1".repeat(300));
    assert!(parse(&chain).unwrap_err().message.contains("depth"));
}

#[test]
fn integer_minimum_and_namespaced_calls() {
    let p = parse("fn main() -> ():\n    io.println(-9223372036854775808)\n").unwrap();
    assert_eq!(p.functions[0].return_type, Some(Type::Unit));
    let ExprKind::Block(stmts) = &p.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Expr(e) = &stmts[0] else { panic!() };
    assert!(
        matches!(&e.kind, ExprKind::Call { name, args } if name == "io.println" && matches!(args[0].kind, ExprKind::Int(i64::MIN)))
    );
}

#[test]
fn functions_require_a_line_boundary() {
    let error = parse("fn first(): 0 fn main(): 1").unwrap_err();
    assert!(error.message.contains("end of line"));
}

#[test]
fn nested_blocks_preserve_following_statements() {
    let source = "fn main():\n    let x = if true:\n        if false:\n            1\n        else:\n            2\n    else:\n        3\n    println(x)\n";
    let program = parse(source).unwrap();
    let ExprKind::Block(statements) = &program.functions[0].body.kind else {
        panic!()
    };
    assert_eq!(statements.len(), 2);
}

#[test]
fn source_ranges_survive_crlf_unicode_and_missing_final_newline() {
    let source = "fn main():\r\n    println(\"🌿\")\r\n    @";
    let error = parse(source).unwrap_err();
    assert_eq!(&source[error.span.start..error.span.end], "@");
    assert!(parse("fn main():\r\n    1\r\n").is_ok());
}

#[test]
fn keyword_bindings_are_rejected() {
    for word in [
        "if", "else", "return", "match", "type", "true", "and", "fn", "do", "defer", "as",
        "module", "break", "continue", "derive", "newtype", "send", "after",
    ] {
        assert!(parse(&format!("fn main():\n    let {word} = 1")).is_err());
    }
}

#[test]
fn malformed_inputs_and_all_prefixes_never_panic() {
    let samples = [
        "fn main(): println(\"Grüße 🌿\")",
        "fn f(a: Int) -> Int:\n    if true: a\n    else: -9223372036854775808\n",
        "fn main():\n    let x = 1 + 2 * (3 - 4)\n    println(x)\n",
    ];
    for source in samples {
        for (end, _) in source
            .char_indices()
            .chain(std::iter::once((source.len(), '\0')))
        {
            let result = std::panic::catch_unwind(|| parse(&source[..end]));
            assert!(result.is_ok(), "panicked on {:?}", &source[..end]);
            if let Err(error) = result.unwrap() {
                assert!(error.span.start <= error.span.end && error.span.end <= end);
            }
        }
    }
    let mut random = 0xC0FFEEu64;
    let alphabet = b"fn let():\n \t01239+-*/%<>=!\"#_[],.\\";
    for length in 0..512 {
        let source: String = (0..length)
            .map(|_| {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                alphabet[(random as usize) % alphabet.len()] as char
            })
            .collect();
        assert!(std::panic::catch_unwind(|| parse(&source)).is_ok());
    }
}
