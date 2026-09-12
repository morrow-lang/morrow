use fern_compiler::{Diagnostic, Span, Type, ast::BinaryOp, ir::*, lowering};
fn ex(kind: ExprKind, ty: Type) -> Expr {
    Expr {
        kind,
        ty,
        span: Span::default(),
    }
}
fn int(n: i64) -> Expr {
    ex(ExprKind::Int(n), Type::Int)
}
fn local(id: usize, ty: Type) -> Expr {
    ex(ExprKind::Local(LocalId(id)), ty)
}
fn param(id: usize, ty: Type) -> Param {
    Param {
        id: LocalId(id),
        ty,
    }
}
fn call(id: usize, args: Vec<Expr>, ty: Type) -> Expr {
    ex(
        ExprKind::Call {
            target: CallTarget::Function(FunctionId(id)),
            args,
        },
        ty,
    )
}
fn function(id: usize, params: Vec<Param>, body: Expr, ty: Type) -> Function {
    Function {
        mailbox: None,
        id: FunctionId(id),
        name: if id == 0 {
            "main".into()
        } else {
            format!("user{id}")
        },
        params,
        captures: vec![],
        return_type: ty,
        body,
        local_count: 32,
    }
}
fn emit(f: Function, others: Vec<Function>) -> Result<String, Diagnostic> {
    let mut functions = vec![
        function(0, vec![], ex(ExprKind::Unit, Type::Unit), Type::Unit),
        f,
    ];
    functions.extend(others);
    lowering::emit(&Program {
        types: vec![],
        functions,
    })
}
fn body(il: &str, id: usize) -> &str {
    let marker = format!(" $f{id}(");
    let header = il
        .lines()
        .find(|line| line.starts_with("function ") && line.contains(&marker))
        .unwrap();
    let pos = il.find(header).unwrap();
    &il[pos..pos + il[pos..].find("\n}\n").unwrap()]
}
fn branch(recursive: Expr, base: Expr) -> Expr {
    ex(
        ExprKind::If {
            condition: Box::new(ex(ExprKind::Bool(true), Type::Bool)),
            then_branch: Box::new(recursive),
            else_branch: Some(Box::new(base.clone())),
        },
        base.ty,
    )
}
#[test]
fn malformed_self_calls_are_validated_before_tail_rewriting() {
    for recursive in [
        call(1, vec![], Type::Int),
        call(1, vec![ex(ExprKind::Bool(true), Type::Bool)], Type::Int),
        call(1, vec![int(0)], Type::String),
    ] {
        let f = function(
            1,
            vec![param(0, Type::Int)],
            branch(recursive, int(0)),
            Type::Int,
        );
        assert!(emit(f, vec![]).is_err());
    }
}
#[test]
fn direct_self_tail_call_reloads_full_width_parameters_and_jumps_without_call() {
    let recursive = call(1, vec![local(1, Type::Int), local(0, Type::Int)], Type::Int);
    let f = function(
        1,
        vec![param(0, Type::Int), param(1, Type::Int)],
        branch(recursive, local(0, Type::Int)),
        Type::Int,
    );
    let il = emit(f, vec![]).unwrap();
    let f = body(&il, 1);
    assert!(f.contains("@recur\n"), "{f}");
    assert_eq!(f.matches("jmp @recur").count(), 2, "{f}");
    assert!(!f.contains("call $f1"), "{f}");
    assert!(!f.contains("jmp @start"));
    assert!(f.contains("=l loadl %"));
}
#[test]
fn explicit_return_inside_non_tail_expression_still_uses_tail_transfer() {
    let returned = ex(
        ExprKind::Return(Box::new(call(1, vec![int(2)], Type::Int))),
        Type::Never,
    );
    let f = function(
        1,
        vec![param(0, Type::Int)],
        ex(ExprKind::Block(vec![Stmt::Expr(returned)]), Type::Never),
        Type::Int,
    );
    assert!(!body(&emit(f, vec![]).unwrap(), 1).contains("call $f1"));
}
#[test]
fn self_calls_used_as_operands_and_indirect_calls_remain_ordinary() {
    let recursive = call(1, vec![int(2)], Type::Int);
    let f = function(
        1,
        vec![param(0, Type::Int)],
        ex(
            ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(recursive),
                right: Box::new(int(1)),
            },
            Type::Int,
        ),
        Type::Int,
    );
    assert!(body(&emit(f, vec![]).unwrap(), 1).contains("call $f1"));
}
#[test]
fn function_owned_defer_disables_tail_jumps_but_lifted_cleanup_does_not() {
    let cleanup = function(2, vec![], ex(ExprKind::Unit, Type::Unit), Type::Unit);
    let closure = ex(
        ExprKind::Closure {
            function: FunctionId(2),
            captures: vec![],
        },
        Type::Function(vec![], Box::new(Type::Unit)),
    );
    let deferred = ex(ExprKind::Defer(Box::new(closure.clone())), Type::Unit);
    let f = function(
        1,
        vec![],
        ex(
            ExprKind::Block(vec![
                Stmt::Expr(deferred),
                Stmt::Expr(call(1, vec![], Type::Unit)),
            ]),
            Type::Unit,
        ),
        Type::Unit,
    );
    let il = emit(f, vec![cleanup.clone()]).unwrap();
    assert!(body(&il, 1).contains("call $f1"));
    assert!(!body(&il, 1).contains("@recur"));
    let nested = function(
        3,
        vec![],
        ex(ExprKind::Defer(Box::new(closure)), Type::Unit),
        Type::Unit,
    );
    let nested_value = ex(
        ExprKind::Closure {
            function: FunctionId(3),
            captures: vec![],
        },
        Type::Function(vec![], Box::new(Type::Unit)),
    );
    let f = function(
        1,
        vec![],
        ex(
            ExprKind::Block(vec![
                Stmt::Expr(nested_value),
                Stmt::Expr(call(1, vec![], Type::Unit)),
            ]),
            Type::Unit,
        ),
        Type::Unit,
    );
    let il = emit(f, vec![cleanup, nested]).unwrap();
    assert!(!body(&il, 1).contains("call $f1"));
}
#[test]
fn loop_scratch_allocations_are_hoisted_before_the_recursion_header() {
    let loop_body = ex(
        ExprKind::For {
            pattern: Pattern::Bind(LocalId(1)),
            iterable: Box::new(ex(
                ExprKind::Range {
                    start: Box::new(int(0)),
                    end: Box::new(int(1)),
                    inclusive: false,
                },
                Type::Range,
            )),
            body: Box::new(ex(ExprKind::Unit, Type::Unit)),
        },
        Type::Unit,
    );
    let f = function(
        1,
        vec![],
        ex(
            ExprKind::Block(vec![
                Stmt::Expr(loop_body),
                Stmt::Expr(call(1, vec![], Type::Unit)),
            ]),
            Type::Unit,
        ),
        Type::Unit,
    );
    let il = emit(f, vec![]).unwrap();
    let f = body(&il, 1);
    let header = f.find("@recur\n").unwrap();
    assert!(!f[header..].contains("alloc8"), "{f}");
    assert!(f[..header].contains("alloc8"));
}

#[test]
fn every_argument_finishes_before_parameter_updates_and_fault_context_is_retained() {
    let recurse = call(
        1,
        vec![call(2, vec![], Type::Int), call(3, vec![], Type::Int)],
        Type::Int,
    );
    let f = function(
        1,
        vec![param(0, Type::Int), param(1, Type::Int)],
        branch(recurse, int(0)),
        Type::Int,
    );
    let il = emit(
        f,
        vec![
            function(2, vec![], int(1), Type::Int),
            function(3, vec![], int(2), Type::Int),
        ],
    )
    .unwrap();
    let f = body(&il, 1);
    let header = f.find("@recur\n").unwrap();
    let repeated = &f[header..];
    let first = repeated.find("call $f2(l 0, l %fault)").unwrap();
    let second = repeated.find("call $f3(l 0, l %fault)").unwrap();
    let store_first = repeated.find(", %t0\n").unwrap();
    let store_second = repeated.find(", %t1\n").unwrap();
    assert!(
        first < second && second < store_first && store_first < store_second,
        "{f}"
    );
    assert!(repeated[first..second].contains("loadl %fault"));
    assert!(!repeated.contains("storel 0, %fault"));
}

#[test]
fn tail_parameters_preserve_float_bits_bool_unit_and_pointer_width() {
    let types = [
        Type::Float,
        Type::Bool,
        Type::Unit,
        Type::List(Box::new(Type::Int)),
    ];
    let params = types
        .iter()
        .enumerate()
        .map(|(i, t)| param(i, t.clone()))
        .collect();
    let args = types
        .iter()
        .enumerate()
        .map(|(i, t)| local(i, t.clone()))
        .collect();
    let f = function(
        1,
        params,
        branch(
            call(1, args, Type::Float),
            ex(ExprKind::Float(1.5), Type::Float),
        ),
        Type::Float,
    );
    let il = emit(f, vec![]).unwrap();
    let f = body(&il, 1);
    assert!(f.contains("=l cast %v0"));
    assert!(f.contains("=l extuw %v1"));
    assert!(f.contains("=d cast %"));
    assert!(f.contains("=w copy %"));
    assert!(!f.contains("call $f1"));
}

#[test]
fn match_guards_remain_calls_while_arm_bodies_transfer_in_tail_position() {
    let value = local(0, Type::Bool);
    let recurse = call(1, vec![value.clone()], Type::Bool);
    let arms = vec![
        MatchArm {
            pattern: Pattern::Bool(true),
            guard: Some(recurse.clone()),
            body: recurse,
            span: Span::default(),
        },
        MatchArm {
            pattern: Pattern::Wildcard,
            guard: None,
            body: ex(ExprKind::Bool(false), Type::Bool),
            span: Span::default(),
        },
    ];
    let f = function(
        1,
        vec![param(0, Type::Bool)],
        ex(
            ExprKind::Match {
                value: Box::new(value),
                arms,
            },
            Type::Bool,
        ),
        Type::Bool,
    );
    let il = emit(f, vec![]).unwrap();
    let f = body(&il, 1);
    assert_eq!(f.matches("call $f1").count(), 1, "{f}");
    assert_eq!(f.matches("jmp @recur").count(), 2, "{f}");
}

#[test]
fn with_success_and_error_handler_tails_share_hoisted_scratch_slots() {
    let result = Type::Result(Box::new(Type::Int), Box::new(Type::String));
    let step = WithStep {
        pattern: Pattern::Bind(LocalId(0)),
        value: ex(
            ExprKind::Construct {
                constructor: fern_compiler::Constructor::Ok,
                value: Some(Box::new(int(1))),
            },
            result,
        ),
        error_handler: Some(0),
    };
    let handler = WithHandler {
        error: param(1, Type::String),
        body: call(1, vec![], Type::Unit),
    };
    let expr = ex(
        ExprKind::With {
            steps: vec![step],
            body: Box::new(call(1, vec![], Type::Unit)),
            handlers: vec![handler],
        },
        Type::Unit,
    );
    let il = emit(function(1, vec![], expr, Type::Unit), vec![]).unwrap();
    let f = body(&il, 1);
    assert_eq!(f.matches("jmp @recur").count(), 3, "{f}");
    assert!(!f.contains("call $f1"));
    assert!(!f[f.find("@recur\n").unwrap()..].contains("alloc8"));
}

#[test]
fn closure_capture_expressions_with_owned_defer_disable_reuse() {
    let unit = ex(ExprKind::Unit, Type::Unit);
    let cleanup = function(2, vec![], unit, Type::Unit);
    let closure = ex(
        ExprKind::Closure {
            function: FunctionId(2),
            captures: vec![],
        },
        Type::Function(vec![], Box::new(Type::Unit)),
    );
    let defer = ex(ExprKind::Defer(Box::new(closure)), Type::Unit);
    let mut captured = function(3, vec![], local(0, Type::Unit), Type::Unit);
    captured.captures = vec![param(0, Type::Unit)];
    let value = ex(
        ExprKind::Closure {
            function: FunctionId(3),
            captures: vec![defer],
        },
        Type::Function(vec![], Box::new(Type::Unit)),
    );
    let f = function(
        1,
        vec![],
        ex(
            ExprKind::Block(vec![
                Stmt::Expr(value),
                Stmt::Expr(call(1, vec![], Type::Unit)),
            ]),
            Type::Unit,
        ),
        Type::Unit,
    );
    let il = emit(f, vec![cleanup, captured]).unwrap();
    let f = body(&il, 1);
    assert!(f.contains("call $f1"));
    assert!(!f.contains("@recur"));
}

#[test]
fn indirect_self_recursion_and_mutual_calls_do_not_become_local_jumps() {
    let callee = ex(
        ExprKind::Closure {
            function: FunctionId(1),
            captures: vec![],
        },
        Type::Function(vec![], Box::new(Type::Int)),
    );
    let expr = ex(
        ExprKind::Invoke {
            callee: Box::new(callee),
            args: vec![],
        },
        Type::Int,
    );
    let il = emit(function(1, vec![], expr, Type::Int), vec![]).unwrap();
    let f = body(&il, 1);
    assert!(f.contains("call %"));
    assert!(!f.contains("@recur"));
    let one = function(1, vec![], call(2, vec![], Type::Int), Type::Int);
    let two = function(2, vec![], call(1, vec![], Type::Int), Type::Int);
    let il = emit(one, vec![two]).unwrap();
    assert!(body(&il, 1).contains("call $f2"));
    assert!(body(&il, 2).contains("call $f1"));
    assert!(!body(&il, 1).contains("@recur"));
    assert!(!body(&il, 2).contains("@recur"));
}
