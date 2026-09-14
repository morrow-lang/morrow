use morrow_compiler::{Type, check, ir, parse};
fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
#[test]
fn power_and_bitwise_expressions_preserve_semantic_operand_types() {
    let p = checked(
        "fn integer(x: Int, y: Int) -> Int: (x ** y) + ((x &&& y) ||| (x ^^^ y)) + (x <<< y) + (x >>> y) + ~~~x\nfn real(x: Float, y: Float) -> Float: x ** y\nfn main():\n    println(integer(x: 2, y: 3))\n    println(real(x: 2.0, y: -1.0))\n",
    );
    assert_eq!(
        p.functions
            .iter()
            .find(|f| f.name == "integer")
            .unwrap()
            .body
            .ty,
        Type::Int
    );
    assert_eq!(
        p.functions
            .iter()
            .find(|f| f.name == "real")
            .unwrap()
            .body
            .ty,
        Type::Float
    );
}
#[test]
fn unsupported_numeric_domains_do_not_coerce_or_inherit_transport_widths() {
    for expr in [
        "true ** false",
        "\"a\" ** \"b\"",
        "2 ** 3.0",
        "2.0 ** 3",
        "1.0 &&& 2.0",
        "true ||| false",
        "\"a\" ^^^ \"b\"",
        "1 <<< 2.0",
        "1.0 >>> 2.0",
        "~~~true",
        "~~~1.0",
    ] {
        assert!(
            !rejected(&format!("fn main(): {expr}\n")).is_empty(),
            "{expr}"
        );
    }
}
#[test]
fn numeric_operators_constrain_lambdas_and_generic_function_instances() {
    checked(
        "fn square(x: a) -> a: x ** x\nfn main():\n    let shift = (x) -> x <<< 1\n    let invert = (x) -> ~~~x\n    println(shift(invert(0)))\n    println(square(2))\n    println(square(2.0))\n",
    );
}
#[test]
fn float_list_membership_accepts_direct_and_first_class_calls() {
    checked(
        "fn main():\n    let has: (List(Float), Float) -> Bool = List.contains\n    println(List.contains([0.0, -0.0], 0.0))\n    println(has([1.0], 1.0))\n",
    );
    assert!(rejected("fn main(): List.contains([[1]], [1])\n").contains("requires"));
    assert!(rejected("fn main(): Map.len(%{1.0: 1})\n").contains("map key"));
}
#[test]
fn runtime_domain_failures_remain_typed_and_never_operands_still_exit() {
    checked(
        "fn bad(exponent: Int) -> Int: 2 ** exponent\nfn value() -> Int: (return 7) ** 2\nfn main():\n    println(bad(-1))\n    println(1 / 0)\n    println(1 % 0)\n    println(value())\n",
    );
}

#[test]
fn numeric_operator_type_matrix_checks_public_ast_independently_of_parser() {
    use morrow_compiler::ast::{BinaryOp as Op, ExprKind};
    for (op, accepts_float) in [
        (Op::Power, true),
        (Op::BitAnd, false),
        (Op::BitOr, false),
        (Op::BitXor, false),
        (Op::ShiftLeft, false),
        (Op::ShiftRight, false),
    ] {
        for (source, expected, valid) in [
            ("fn main(): 2 + 3\n", Type::Int, true),
            ("fn main(): 2.0 + 3.0\n", Type::Float, accepts_float),
            ("fn main(): true == false\n", Type::Bool, false),
        ] {
            let mut ast = parse::parse(source).unwrap();
            let ExprKind::Binary { op: actual, .. } = &mut ast.functions[0].body.kind else {
                panic!()
            };
            *actual = op;
            let result = check::check(&ast);
            if valid {
                assert_eq!(result.unwrap().functions[0].body.ty, expected);
            } else {
                assert!(result.is_err(), "{op:?} / {expected:?}");
            }
        }
    }
}
