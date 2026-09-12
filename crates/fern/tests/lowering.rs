use fern_compiler::{
    Span, Type,
    ast::{BinaryOp, UnaryOp},
    ir::*,
    lowering,
};

fn expr(kind: ExprKind, ty: Type) -> Expr {
    Expr {
        kind,
        ty,
        span: Span::default(),
    }
}
fn int(value: i64) -> Expr {
    expr(ExprKind::Int(value), Type::Int)
}
fn boolean(value: bool) -> Expr {
    expr(ExprKind::Bool(value), Type::Bool)
}
fn string(value: &str) -> Expr {
    expr(ExprKind::String(value.into()), Type::String)
}
fn function(id: usize, name: &str, body: Expr) -> Function {
    Function {
        mailbox: None,
        captures: vec![],
        id: FunctionId(id),
        name: name.into(),
        return_type: body.ty.clone(),
        body,
        params: vec![],
        local_count: 0,
    }
}
fn program(body: Expr) -> Program {
    Program {
        types: vec![],
        functions: vec![function(0, "main", body)],
    }
}
fn call(target: CallTarget, args: Vec<Expr>, ty: Type) -> Expr {
    expr(ExprKind::Call { target, args }, ty)
}
fn block(stmts: Vec<Stmt>, ty: Type) -> Expr {
    expr(ExprKind::Block(stmts), ty)
}
fn binary(op: BinaryOp, left: Expr, right: Expr, ty: Type) -> Expr {
    expr(
        ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        ty,
    )
}

#[test]
fn literal_and_main_symbol() {
    let il = lowering::emit(&program(int(42))).unwrap();
    assert!(il.contains("export function w $fern_main()"), "{il}");
    assert!(il.contains("storel 42, %return_slot"), "{il}");
}

#[test]
fn typed_function_calls_use_pointer_returns_and_mangled_ids() {
    let target = function(7, "unsafe$name", string("hello"));
    let value = call(CallTarget::Function(FunctionId(7)), vec![], Type::String);
    let main = function(
        0,
        "main",
        call(
            CallTarget::Builtin(Builtin::Println),
            vec![value],
            Type::Unit,
        ),
    );
    let il = lowering::emit(&Program {
        types: vec![],
        functions: vec![main, target],
    })
    .unwrap();
    assert!(il.contains("function l $f7(l %env, l %fault)"), "{il}");
    assert!(il.contains("=l call $f7(l 0, l %fault)"), "{il}");
    assert!(il.contains("call $fern_println_str(l %"), "{il}");
    assert!(!il.contains("unsafe$name"));
}

#[test]
fn signed_integer_print_uses_runtime_int64_abi() {
    let il = lowering::emit(&program(call(
        CallTarget::Builtin(Builtin::Print),
        vec![int(-7)],
        Type::Unit,
    )))
    .unwrap();
    assert!(!il.contains("extsw"), "{il}");
    assert!(il.contains("call $fern_print_int(l -7"), "{il}");
}

#[test]
fn bool_print_and_string_builtins_have_distinct_abis() {
    let equal = call(
        CallTarget::Builtin(Builtin::StringEq),
        vec![string("x"), string("x")],
        Type::Bool,
    );
    let concat = call(
        CallTarget::Builtin(Builtin::StringConcat),
        vec![string("x"), string("y")],
        Type::String,
    );
    let length = call(
        CallTarget::Builtin(Builtin::StringLen),
        vec![concat],
        Type::Int,
    );
    let main = block(
        vec![
            Stmt::Expr(call(
                CallTarget::Builtin(Builtin::Println),
                vec![equal],
                Type::Unit,
            )),
            Stmt::Expr(length),
        ],
        Type::Int,
    );
    let il = lowering::emit(&program(main)).unwrap();
    assert!(il.contains("=w call $fern_str_eq(l $str"), "{il}");
    assert!(il.contains("call $fern_println_bool("), "{il}");
    assert!(il.contains("=l call $fern_str_concat(l $str"), "{il}");
    assert!(il.contains("=l call $fern_str_len(l %"), "{il}");
}

#[test]
fn utf8_quotes_backslashes_and_long_strings_are_byte_safe() {
    let value = format!("a\"\\\n🌿{}", "z".repeat(4096));
    let mut p = program(call(
        CallTarget::Builtin(Builtin::Println),
        vec![string(&value)],
        Type::Unit,
    ));
    let il = lowering::emit(&p).unwrap();
    assert!(il.contains("b 34"), "{il}");
    assert!(il.contains("b 92"), "{il}");
    assert!(il.contains("b 240"), "{il}");
    assert!(!il.contains(&"z".repeat(1024)), "QBE token must be bounded");
    p.functions[0].body = call(
        CallTarget::Builtin(Builtin::Println),
        vec![string("bad\0string")],
        Type::Unit,
    );
    assert!(
        lowering::emit(&p).is_err(),
        "C runtime cannot preserve embedded NUL"
    );
}

#[test]
fn nested_if_phi_uses_actual_predecessors() {
    let inner = expr(
        ExprKind::If {
            condition: Box::new(boolean(true)),
            then_branch: Box::new(int(11)),
            else_branch: Some(Box::new(int(22))),
        },
        Type::Int,
    );
    let outer = expr(
        ExprKind::If {
            condition: Box::new(boolean(false)),
            then_branch: Box::new(inner),
            else_branch: Some(Box::new(int(33))),
        },
        Type::Int,
    );
    let il = lowering::emit(&program(outer)).unwrap();
    assert_eq!(il.matches("=l phi").count(), 2, "{il}");
    assert!(
        il.contains("@b5 %t0, @b1 33"),
        "outer phi needs inner merge predecessor: {il}"
    );
}

#[test]
fn short_circuit_rhs_is_branched_and_merged() {
    for op in [BinaryOp::And, BinaryOp::Or] {
        let rhs = call(CallTarget::Function(FunctionId(1)), vec![], Type::Bool);
        let body = block(
            vec![
                Stmt::Expr(binary(op, boolean(false), rhs, Type::Bool)),
                Stmt::Expr(int(0)),
            ],
            Type::Int,
        );
        let p = Program {
            types: vec![],
            functions: vec![
                function(0, "main", body),
                function(1, "effect", boolean(true)),
            ],
        };
        let il = lowering::emit(&p).unwrap();
        assert!(
            il.find("jnz").unwrap() < il.find("call $f1").unwrap(),
            "{il}"
        );
        assert!(il.contains("=w phi"), "{il}");
        assert!(!il.contains("=w and") && !il.contains("=w or"), "{il}");
    }
}

#[test]
fn locals_parameters_and_final_block_value_are_preserved() {
    let body = block(
        vec![
            Stmt::Let {
                id: LocalId(1),
                value: int(3),
            },
            Stmt::Expr(binary(
                BinaryOp::Subtract,
                expr(ExprKind::Local(LocalId(0)), Type::Int),
                expr(ExprKind::Local(LocalId(1)), Type::Int),
                Type::Int,
            )),
        ],
        Type::Int,
    );
    let mut helper = function(1, "helper", body);
    helper.params.push(Param {
        id: LocalId(0),
        ty: Type::Int,
    });
    helper.local_count = 2;
    let p = Program {
        types: vec![],
        functions: vec![
            function(
                0,
                "main",
                call(
                    CallTarget::Function(FunctionId(1)),
                    vec![int(10)],
                    Type::Int,
                ),
            ),
            helper,
        ],
    };
    let il = lowering::emit(&p).unwrap();
    assert!(
        il.contains("function l $f1(l %env, l %fault, l %v0)"),
        "{il}"
    );
    assert!(il.contains("sub %v0, 3"), "{il}");
}

#[test]
fn malformed_ir_rejects_unknown_locals_functions_and_type_annotations() {
    assert!(lowering::emit(&program(expr(ExprKind::Local(LocalId(7)), Type::Int))).is_err());
    assert!(
        lowering::emit(&program(call(
            CallTarget::Function(FunctionId(99)),
            vec![],
            Type::Int
        )))
        .is_err()
    );
    assert!(lowering::emit(&program(expr(ExprKind::Int(2), Type::String))).is_err());
    assert!(
        lowering::emit(&program(binary(
            BinaryOp::Add,
            string("a"),
            int(2),
            Type::Int
        )))
        .is_err()
    );
    assert!(
        lowering::emit(&program(call(
            CallTarget::Builtin(Builtin::Print),
            vec![],
            Type::Unit
        )))
        .is_err()
    );
    assert!(
        lowering::emit(&program(expr(
            ExprKind::Unary {
                op: UnaryOp::Not,
                value: Box::new(int(1))
            },
            Type::Int
        )))
        .is_err()
    );
}

#[test]
fn local_scope_cannot_escape_branch_or_be_defined_twice() {
    let branch = block(
        vec![Stmt::Let {
            id: LocalId(0),
            value: int(3),
        }],
        Type::Unit,
    );
    let conditional = expr(
        ExprKind::If {
            condition: Box::new(boolean(true)),
            then_branch: Box::new(branch),
            else_branch: None,
        },
        Type::Unit,
    );
    let mut p = program(block(
        vec![
            Stmt::Expr(conditional),
            Stmt::Expr(expr(ExprKind::Local(LocalId(0)), Type::Int)),
        ],
        Type::Int,
    ));
    p.functions[0].local_count = 1;
    assert!(lowering::emit(&p).is_err());
    p.functions[0].body = block(
        vec![
            Stmt::Let {
                id: LocalId(0),
                value: int(1),
            },
            Stmt::Let {
                id: LocalId(0),
                value: int(2),
            },
        ],
        Type::Unit,
    );
    p.functions[0].return_type = Type::Unit;
    assert!(lowering::emit(&p).is_err());
}

#[test]
fn duplicate_function_id_invalid_main_and_wrong_returns_are_rejected() {
    let mut p = Program {
        types: vec![],
        functions: vec![function(0, "main", int(0)), function(0, "other", int(0))],
    };
    assert!(lowering::emit(&p).is_err());
    p.functions.pop();
    p.functions[0].return_type = Type::String;
    assert!(lowering::emit(&p).is_err());
    assert!(lowering::emit(&program(string("no"))).is_err());
    assert!(
        lowering::emit(&Program {
            types: vec![],
            functions: vec![]
        })
        .is_err()
    );
}

#[test]
fn full_width_int_math_comparison_and_main_exit_wrapper() {
    let body = binary(BinaryOp::Subtract, int(i64::MAX), int(i64::MIN), Type::Int);
    let il = lowering::emit(&program(body)).unwrap();
    assert!(
        il.contains("=l sub 9223372036854775807, -9223372036854775808"),
        "{il}"
    );
    assert!(il.contains("function l $f0(l %env, l %fault)"), "{il}");
    assert!(il.contains("export function w $fern_main()"), "{il}");
    assert!(il.contains("=w copy %exit"), "{il}");
    let cmp = binary(BinaryOp::Lt, int(-1), int(1), Type::Bool);
    let il = lowering::emit(&program(call(
        CallTarget::Builtin(Builtin::Println),
        vec![cmp],
        Type::Unit,
    )))
    .unwrap();
    assert!(il.contains("=w csltl -1, 1"), "{il}");
    assert!(il.contains("=l extuw"), "{il}");
}

#[test]
fn only_unit_main_discards_a_nonunit_body() {
    let mut main = function(0, "main", int(99));
    main.return_type = Type::Unit;
    let il = lowering::emit(&Program {
        types: vec![],
        functions: vec![main.clone()],
    })
    .unwrap();
    assert!(
        il.contains("function w $f0(l %env, l %fault)\n")
            || il.contains("function w $f0(l %env, l %fault) {"),
        "{il}"
    );
    assert!(!il.contains("ret 99"), "{il}");
    let mut helper = function(1, "helper", int(99));
    helper.return_type = Type::Unit;
    assert!(
        lowering::emit(&Program {
            types: vec![],
            functions: vec![main, helper]
        })
        .is_err()
    );
}

#[test]
fn unit_parameters_and_if_without_else_evaluate_effects() {
    let mut helper = function(1, "helper", expr(ExprKind::Local(LocalId(0)), Type::Unit));
    helper.params.push(Param {
        id: LocalId(0),
        ty: Type::Unit,
    });
    helper.local_count = 1;
    let effect = call(
        CallTarget::Builtin(Builtin::Println),
        vec![string("effect")],
        Type::Unit,
    );
    let body = expr(
        ExprKind::If {
            condition: Box::new(boolean(true)),
            then_branch: Box::new(call(
                CallTarget::Function(FunctionId(1)),
                vec![effect],
                Type::Unit,
            )),
            else_branch: None,
        },
        Type::Unit,
    );
    let il = lowering::emit(&Program {
        types: vec![],
        functions: vec![function(0, "main", body), helper],
    })
    .unwrap();
    assert!(
        il.contains("function w $f1(l %env, l %fault, w %v0)"),
        "{il}"
    );
    assert!(il.contains("call $f1(l 0, l %fault, w 0)"), "{il}");
    assert!(
        il.find("call $fern_println_str").unwrap() < il.find("call $f1").unwrap(),
        "{il}"
    );
}

#[test]
fn string_inequality_compares_contents_not_pointers() {
    let body = binary(BinaryOp::Ne, string("a"), string("b"), Type::Bool);
    let il = lowering::emit(&program(call(
        CallTarget::Builtin(Builtin::Println),
        vec![body],
        Type::Unit,
    )))
    .unwrap();
    assert!(il.contains("call $fern_str_eq(l $str0, l $str1)"), "{il}");
    assert!(il.contains("ceqw %t0, 0"), "{il}");
    assert!(
        !il.split("function $fern_rs_run_defers")
            .next()
            .unwrap()
            .contains("cnel"),
        "{il}"
    );
}

#[test]
fn invalid_signatures_local_bounds_and_nesting_are_diagnostics() {
    let mut helper = function(1, "helper", int(0));
    helper.params.push(Param {
        id: LocalId(0),
        ty: Type::Int,
    });
    helper.local_count = 1;
    let mut p = Program {
        types: vec![],
        functions: vec![
            function(
                0,
                "main",
                call(
                    CallTarget::Function(FunctionId(1)),
                    vec![string("bad")],
                    Type::Int,
                ),
            ),
            helper,
        ],
    };
    assert!(lowering::emit(&p).is_err());
    p.functions[0].body = int(0);
    p.functions[1].local_count = 0;
    assert!(lowering::emit(&p).is_err());
    let mut deep = int(0);
    for _ in 0..300 {
        deep = expr(
            ExprKind::Unary {
                op: UnaryOp::Negate,
                value: Box::new(deep),
            },
            Type::Int,
        );
    }
    let error = lowering::emit(&program(deep)).unwrap_err();
    assert!(error.message.contains("complexity limit"));
}

#[test]
fn string_addition_uses_concat_and_rejects_other_string_arithmetic() {
    let addition = binary(BinaryOp::Add, string("a"), string("b"), Type::String);
    let il = lowering::emit(&program(call(
        CallTarget::Builtin(Builtin::Println),
        vec![addition],
        Type::Unit,
    )))
    .unwrap();
    assert!(
        il.contains("=l call $fern_str_concat(l $str0, l $str1)"),
        "{il}"
    );
    assert!(!il.contains("fern_str_eq"), "{il}");
    for op in [
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Remainder,
    ] {
        let invalid = binary(op, string("a"), string("b"), Type::String);
        assert!(
            lowering::emit(&program(call(
                CallTarget::Builtin(Builtin::Println),
                vec![invalid],
                Type::Unit
            )))
            .is_err()
        );
    }
}
