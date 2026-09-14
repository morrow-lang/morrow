use fern_compiler::{Span, Type, ast::BinaryOp, ir::*, lowering};
fn ex(kind: ExprKind, ty: Type) -> Expr {
    Expr {
        kind,
        ty,
        span: Span::default(),
    }
}
fn int(value: i64) -> Expr {
    ex(ExprKind::Int(value), Type::Int)
}
fn binary(op: BinaryOp, a: Expr, b: Expr) -> Expr {
    let ty = a.ty.clone();
    ex(
        ExprKind::Binary {
            op,
            left: Box::new(a),
            right: Box::new(b),
        },
        ty,
    )
}
fn function(id: usize, name: &str, body: Expr) -> Function {
    Function {
        mailbox: None,
        id: FunctionId(id),
        name: name.into(),
        params: vec![],
        captures: vec![],
        return_type: body.ty.clone(),
        body,
        local_count: 8,
    }
}
fn emit(body: Expr, helpers: Vec<Function>) -> String {
    let mut main = function(0, "main", body);
    main.return_type = Type::Unit;
    let mut functions = vec![main];
    functions.extend(helpers);
    lowering::emit(&Program {
        functions,
        types: vec![],
    })
    .unwrap()
}
#[test]
fn integer_division_guards_domain_and_overflow_before_machine_division() {
    let il = emit(binary(BinaryOp::Divide, int(i64::MIN), int(-1)), vec![]);
    assert!(il.contains("l %env, l %fault"), "{il}");
    assert!(il.contains("fern_rs_int_div"), "{il}");
    assert!(il.contains("integer division by zero"), "{il}");
    assert!(il.contains("storel 1, %fault"), "{il}");
}

#[test]
fn safe_literal_integer_divisors_do_not_require_a_runtime_helper_call() {
    for divisor in [1, 2, 7, 31, 2_147_483_647, i64::MAX, -2, i64::MIN] {
        for op in [BinaryOp::Divide, BinaryOp::Remainder] {
            let il = emit(binary(op, int(i64::MIN), int(divisor)), vec![]);
            assert!(
                !il.contains("call $fern_rs_int_div"),
                "safe divisor {divisor} must remain visible to native optimization: {il}"
            );
            let instruction = if op == BinaryOp::Divide { "div" } else { "rem" };
            assert!(
                il.contains(&format!("{instruction} -9223372036854775808, {divisor}")),
                "{il}"
            );
        }
    }
    // Both possible hardware trap domains retain the established guarded helper.
    for op in [BinaryOp::Divide, BinaryOp::Remainder] {
        for divisor in [0, -1] {
            let il = emit(binary(op, int(i64::MIN), int(divisor)), vec![]);
            assert!(il.contains("call $fern_rs_int_div"), "{il}");
        }
        let runtime_divisor = ex(
            ExprKind::Call {
                target: CallTarget::Function(FunctionId(1)),
                args: vec![],
            },
            Type::Int,
        );
        let il = emit(
            binary(op, int(i64::MIN), runtime_divisor),
            vec![function(1, "divisor", int(7))],
        );
        assert!(il.contains("call $fern_rs_int_div"), "{il}");
    }
}
#[test]
fn generated_and_indirect_calls_receive_current_fault_context_and_check_it() {
    let helper = function(1, "bad", binary(BinaryOp::Divide, int(1), int(0)));
    let closure = ex(
        ExprKind::Closure {
            function: FunctionId(1),
            captures: vec![],
        },
        Type::Function(vec![], Box::new(Type::Int)),
    );
    let body = ex(
        ExprKind::Block(vec![
            Stmt::Expr(ex(
                ExprKind::Call {
                    target: CallTarget::Function(FunctionId(1)),
                    args: vec![],
                },
                Type::Int,
            )),
            Stmt::Expr(ex(
                ExprKind::Invoke {
                    callee: Box::new(closure),
                    args: vec![],
                },
                Type::Int,
            )),
        ]),
        Type::Int,
    );
    let il = emit(body, vec![helper]);
    assert!(il.contains("call $f1(l 0, l %fault)"), "{il}");
    assert!(il.matches("loadl %fault").count() >= 4, "{il}");
    assert!(il.contains("storel 0, %return_slot"), "{il}");
}
#[test]
fn cleanup_runner_clears_fault_for_each_callback_then_restores_first_failure() {
    let il = emit(int(0), vec![]);
    let helper = il.split("function $fern_rs_run_defers").nth(1).unwrap();
    assert!(helper.contains("l %fault"), "{il}");
    assert!(helper.contains("storel 0, %fault"), "{il}");
    assert!(helper.contains("call %code(l %closure, l %fault)"), "{il}");
    assert!(helper.contains("storel %primary, %fault"), "{il}");
    assert!(il.contains("call $fern_rs_report_fault"), "{il}");
}

#[test]
fn power_and_bitwise_operators_use_bounded_exact_integer_lowering() {
    let il = emit(binary(BinaryOp::Power, int(2), int(63)), vec![]);
    assert!(
        il.contains("call $fern_rs_int_pow(l %fault, l 2, l 63)"),
        "{il}"
    );
    assert!(il.contains("storel 2, %fault"), "{il}");
    for op in [BinaryOp::ShiftLeft, BinaryOp::ShiftRight] {
        let il = emit(binary(op, int(-8), int(65)), vec![]);
        assert!(il.contains("and 65, 63"), "{il}");
    }
    let il = emit(
        ex(
            ExprKind::Unary {
                op: fern_compiler::ast::UnaryOp::BitNot,
                value: Box::new(int(0)),
            },
            Type::Int,
        ),
        vec![],
    );
    assert!(il.contains("xor 0, -1"), "{il}");
}

#[test]
fn float_power_and_contains_use_double_values_not_payload_bit_equality() {
    let float = |n| ex(ExprKind::Float(n), Type::Float);
    let il = emit(binary(BinaryOp::Power, float(2.0), float(-3.0)), vec![]);
    assert!(il.contains("=d call $pow(d %"), "{il}");
    for target in [
        CallTarget::Builtin(Builtin::ListContains),
        CallTarget::Runtime(fern_compiler::runtime::resolve("List.contains").unwrap()),
    ] {
        let list = ex(
            ExprKind::List(vec![float(-0.0)]),
            Type::List(Box::new(Type::Float)),
        );
        let il = emit(
            ex(
                ExprKind::Call {
                    target,
                    args: vec![list, float(0.0)],
                },
                Type::Bool,
            ),
            vec![],
        );
        assert!(il.contains("call $fern_rs_list_contains_float(l %"), "{il}");
        assert!(il.contains("ceqd %value, %needle"), "{il}");
        assert!(!il.contains("call $fern_list_contains(l"), "{il}");
    }
}

#[test]
fn numeric_public_ir_rejects_wrong_types_and_contains_payloads() {
    let float = ex(ExprKind::Float(2.0), Type::Float);
    let invalid = [
        binary(BinaryOp::BitAnd, float.clone(), float.clone()),
        binary(BinaryOp::ShiftLeft, float.clone(), float),
        binary(
            BinaryOp::Power,
            ex(ExprKind::Bool(true), Type::Bool),
            ex(ExprKind::Bool(false), Type::Bool),
        ),
        ex(
            ExprKind::Call {
                target: CallTarget::Builtin(Builtin::ListContains),
                args: vec![
                    ex(ExprKind::List(vec![]), Type::List(Box::new(Type::Float))),
                    int(1),
                ],
            },
            Type::Bool,
        ),
    ];
    for body in invalid {
        let mut main = function(0, "main", body);
        main.return_type = Type::Unit;
        assert!(
            lowering::emit(&Program {
                types: vec![],
                functions: vec![main]
            })
            .is_err()
        );
    }
}
