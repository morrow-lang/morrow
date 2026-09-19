use morrow_compiler::{check, lowering, parse};

#[test]
fn receiving_source_emits_distinct_steps_and_explicit_execution_context() {
    let source = "fn worker() -> ():\n    let value = receive:\n        value -> value\n    println(value)\nfn main() -> Result((), Int):\n    let pid: Pid(Int) = spawn(worker)\n    send(pid, 42)?\n    Ok(())\n";
    let ast = parse::parse(source).unwrap();
    let program = check::check(&ast).unwrap();
    let il = lowering::emit(&program).unwrap();
    assert!(il.contains("call $morrow_managed_receive(l %exec"));
    assert!(il.contains("call $morrow_managed_spawn(l %exec"));
    assert!(il.contains("call $morrow_managed_run"));
}

fn checked(source: &str) -> morrow_compiler::ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}

#[test]
fn inactive_root_receive_cannot_forge_a_managed_context() {
    use morrow_compiler::{Span, Type, ir::*};
    let mut program = checked("fn main(): ()");
    let span = Span::default();
    program.functions[0].body = Expr {
        ty: Type::Unit,
        span,
        kind: ExprKind::Block(vec![
            Stmt::Expr(Expr {
                ty: Type::Never,
                span,
                kind: ExprKind::Return(Box::new(Expr {
                    ty: Type::Unit,
                    span,
                    kind: ExprKind::Unit,
                })),
            }),
            Stmt::Expr(Expr {
                ty: Type::Unit,
                span,
                kind: ExprKind::Actor(ActorExpr::Receive {
                    view: morrow_compiler::processes::ReceiveView::Messages,
                    mailbox: Type::Int,
                    arms: vec![MatchArm {
                        pattern: Pattern::Wildcard,
                        guard: None,
                        body: Expr {
                            kind: ExprKind::Unit,
                            ty: Type::Unit,
                            span,
                        },
                        span,
                    }],
                    timeout: None,
                }),
            }),
        ]),
    };
    assert!(
        lowering::emit(&program)
            .unwrap_err()
            .message
            .contains("actor context")
    );
}

#[test]
fn inactive_send_still_checks_its_mailbox_signature() {
    use morrow_compiler::{Span, Type, ir::*};
    let mut program = checked("fn main(): ()");
    let span = Span::default();
    let pid = Expr {
        kind: ExprKind::Int(0),
        ty: Type::Pid(Box::new(Type::Int)),
        span,
    };
    let message = Expr {
        kind: ExprKind::String("bad".into()),
        ty: Type::String,
        span,
    };
    program.functions[0].body = Expr {
        ty: Type::Unit,
        span,
        kind: ExprKind::Block(vec![
            Stmt::Expr(Expr {
                ty: Type::Never,
                span,
                kind: ExprKind::Return(Box::new(Expr {
                    ty: Type::Unit,
                    span,
                    kind: ExprKind::Unit,
                })),
            }),
            Stmt::Expr(Expr {
                ty: Type::Result(Box::new(Type::Unit), Box::new(Type::Int)),
                span,
                kind: ExprKind::Actor(ActorExpr::Send {
                    pid: Box::new(pid),
                    message: Box::new(message),
                }),
            }),
        ]),
    };
    assert!(lowering::emit(&program).is_err());
}

#[test]
fn mailbox_metadata_is_bounded_even_without_receives() {
    use morrow_compiler::Type;
    let mut program = checked("fn main(): ()\nfn unused(): ()");
    let mut ty = Type::Int;
    for _ in 0..130 {
        ty = Type::List(Box::new(ty));
    }
    program
        .functions
        .iter_mut()
        .find(|f| f.name == "unused")
        .unwrap()
        .mailbox = Some(ty);
    assert!(
        lowering::emit(&program)
            .unwrap_err()
            .message
            .contains("nesting")
    );
}

fn inactive(expr: morrow_compiler::ir::Expr) -> morrow_compiler::ir::Program {
    use morrow_compiler::{Span, Type, ir::*};
    let mut program = checked("fn main(): ()");
    let unit = Expr {
        kind: ExprKind::Unit,
        ty: Type::Unit,
        span: Span::default(),
    };
    program.functions[0].body.kind = ExprKind::Block(vec![
        Stmt::Expr(Expr {
            kind: ExprKind::Return(Box::new(unit)),
            ty: Type::Never,
            span: Span::default(),
        }),
        Stmt::Expr(expr),
    ]);
    program
}

#[test]
fn inactive_spawn_rejects_unknown_closure_identity() {
    use morrow_compiler::{Span, Type, ir::*};
    let entry = Expr {
        kind: ExprKind::Closure {
            function: FunctionId(9999),
            captures: vec![],
        },
        ty: Type::Function(vec![], Box::new(Type::Unit)),
        span: Span::default(),
    };
    let program = inactive(Expr {
        kind: ExprKind::Actor(ActorExpr::Spawn {
            max_restarts: None,
            entry: Box::new(entry),
            mailbox: Type::Int,
        }),
        ty: Type::Pid(Box::new(Type::Int)),
        span: Span::default(),
    });
    assert!(
        lowering::emit(&program).is_err(),
        "inactive spawn cannot hide an unknown executable identity"
    );
}

#[test]
fn inactive_closure_cannot_forge_its_capture_layout() {
    use morrow_compiler::{Span, Type, ir::*};
    let entry = Expr {
        kind: ExprKind::Closure {
            function: FunctionId(0),
            captures: vec![Expr {
                kind: ExprKind::Int(1),
                ty: Type::Int,
                span: Span::default(),
            }],
        },
        ty: Type::Function(vec![], Box::new(Type::Unit)),
        span: Span::default(),
    };
    assert!(
        lowering::emit(&inactive(entry)).is_err(),
        "inactive closure must retain exact capture arity"
    );
}

#[test]
fn inactive_actor_function_type_requires_a_real_function_signature() {
    use morrow_compiler::{Span, Type, ir::*};
    let entry = Expr {
        kind: ExprKind::Int(0),
        ty: Type::ActorFunction(Box::new(Type::Int), Box::new(Type::Bool)),
        span: Span::default(),
    };
    assert!(
        lowering::emit(&inactive(entry)).is_err(),
        "actor metadata cannot replace a function signature with a scalar"
    );
}

#[test]
fn receiving_control_cannot_hide_forged_result_types_during_cps() {
    use morrow_compiler::{Span, Type, ir::*};
    let span = Span::default();
    let unit = Expr {
        kind: ExprKind::Unit,
        ty: Type::Unit,
        span,
    };
    let int = Expr {
        kind: ExprKind::Int(42),
        ty: Type::Int,
        span,
    };
    let arm = |body| MatchArm {
        pattern: Pattern::Wildcard,
        guard: None,
        body,
        span,
    };
    let receive = |body, ty| Expr {
        kind: ExprKind::Actor(ActorExpr::Receive {
            view: morrow_compiler::processes::ReceiveView::Messages,
            mailbox: Type::Bool,
            arms: vec![arm(body)],
            timeout: None,
        }),
        ty,
        span,
    };
    let conditional = Expr {
        kind: ExprKind::If {
            condition: Box::new(Expr {
                kind: ExprKind::Bool(true),
                ty: Type::Bool,
                span,
            }),
            then_branch: Box::new(int.clone()),
            else_branch: Some(Box::new(unit.clone())),
        },
        ty: Type::Unit,
        span,
    };
    let matching = Expr {
        kind: ExprKind::Match {
            value: Box::new(unit.clone()),
            arms: vec![arm(int.clone())],
        },
        ty: Type::Unit,
        span,
    };
    let block = Expr {
        kind: ExprKind::Block(vec![Stmt::Expr(int.clone())]),
        ty: Type::Unit,
        span,
    };
    let cases = vec![
        receive(int.clone(), Type::Unit),
        receive(unit.clone(), Type::String),
        Expr {
            kind: ExprKind::Return(Box::new(int)),
            ty: Type::Never,
            span,
        },
        receive(conditional, Type::Unit),
        receive(matching, Type::Unit),
        receive(block, Type::Unit),
    ];
    for (index, expr) in cases.into_iter().enumerate() {
        for dead in [false, true] {
            let mut program = checked(
                "fn worker():\n    receive:\n        true -> ()\nfn main():\n    let pid: Pid(Bool) = spawn(worker)\n    ()\n",
            );
            let function = program
                .functions
                .iter_mut()
                .find(|f| f.mailbox.is_some())
                .unwrap();
            function.body = if dead {
                Expr {
                    kind: ExprKind::Block(vec![
                        Stmt::Expr(Expr {
                            kind: ExprKind::Return(Box::new(unit.clone())),
                            ty: Type::Never,
                            span,
                        }),
                        Stmt::Expr(expr.clone()),
                    ]),
                    ty: Type::Unit,
                    span,
                }
            } else {
                expr.clone()
            };
            assert!(
                lowering::emit(&program).is_err(),
                "forged receiving result case {index}, inactive={dead}"
            );
        }
    }
}

#[test]
fn original_and_generated_actor_local_spaces_are_bounded_before_cps() {
    for count in [usize::MAX, 200_000] {
        let mut program = checked(
            "fn worker():\n    receive:\n        true -> ()\nfn main():\n    let pid: Pid(Bool) = spawn(worker)\n    ()\n",
        );
        program
            .functions
            .iter_mut()
            .find(|f| f.mailbox.is_some())
            .unwrap()
            .local_count = count;
        let outcome = std::panic::catch_unwind(|| lowering::emit(&program));
        assert!(outcome.is_ok(), "untrusted local count must never panic");
        assert!(
            outcome.unwrap().is_err(),
            "generated locals retain the existing node limit"
        );
    }
}

fn forged_no_else_never(inactive: bool) -> morrow_compiler::ir::Program {
    use morrow_compiler::{Span, Type, ir::*};
    let span = Span::default();
    let unit = Expr {
        kind: ExprKind::Unit,
        ty: Type::Unit,
        span,
    };
    let condition = Expr {
        kind: ExprKind::Bool(true),
        ty: Type::Bool,
        span,
    };
    let forged = Expr {
        kind: ExprKind::If {
            condition: Box::new(condition),
            then_branch: Box::new(unit.clone()),
            else_branch: None,
        },
        ty: Type::Never,
        span,
    };
    let mut program = checked(
        "fn worker():\n    receive:\n        true -> ()\nfn main():\n    let pid: Pid(Bool) = spawn(worker)\n    ()\n",
    );
    let function = program
        .functions
        .iter_mut()
        .find(|f| f.mailbox.is_some())
        .unwrap();
    function.body = if inactive {
        Expr {
            kind: ExprKind::Block(vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Return(Box::new(unit)),
                    ty: Type::Never,
                    span,
                }),
                Stmt::Expr(forged),
            ]),
            ty: Type::Never,
            span,
        }
    } else {
        forged
    };
    program
}

#[test]
fn ordinary_recursive_cli_does_not_acquire_actor_descriptors_or_a_session() {
    let program = checked(
        "fn helper(n: Int):\n    if n == 0: ()\n    else: helper(n - 1)\nfn main(): helper(3)\n",
    );
    let native = lowering::emit(&program).unwrap();
    assert!(!native.contains("morrow_managed_"));
    assert!(!native.contains("actor_descriptor"));
    assert!(!native.contains("actor_callback"));
}

#[test]
fn resumable_unit_helpers_validate_control_types_before_replacing_results() {
    use morrow_compiler::{Span, Type, ir::*};
    for inactive in [false, true] {
        let mut program = checked(
            "fn helper(n: Int):\n    if n == 0: ()\n    else: helper(n - 1)\nfn main():\n    let pid: Pid(()) = spawn(() -> helper(3))\n    ()\n",
        );
        let function = program
            .functions
            .iter_mut()
            .find(|f| f.name == "helper")
            .unwrap();
        let mut conditional = match &function.body.kind {
            ExprKind::Block(stmts) => match stmts.last().unwrap() {
                Stmt::Expr(expr) => expr.clone(),
                _ => panic!("conditional fixture"),
            },
            _ => function.body.clone(),
        };
        let ExprKind::If { then_branch, .. } = &mut conditional.kind else {
            panic!("conditional fixture");
        };
        **then_branch = Expr {
            kind: ExprKind::Int(42),
            ty: Type::Int,
            span: Span::default(),
        };
        function.body = if inactive {
            Expr {
                kind: ExprKind::Block(vec![
                    Stmt::Expr(Expr {
                        kind: ExprKind::Return(Box::new(Expr {
                            kind: ExprKind::Unit,
                            ty: Type::Unit,
                            span: Span::default(),
                        })),
                        ty: Type::Never,
                        span: Span::default(),
                    }),
                    Stmt::Expr(conditional),
                ]),
                ty: Type::Unit,
                span: Span::default(),
            }
        } else {
            conditional
        };
        assert!(
            lowering::lower(&program).is_err(),
            "Unit tail cloning cannot erase forged branch evidence, inactive={inactive}"
        );
    }
}

#[test]
fn active_no_else_if_cannot_claim_divergence_when_its_condition_completes() {
    assert!(lowering::emit(&forged_no_else_never(false)).is_err());
}

#[test]
fn inactive_no_else_if_cannot_hide_false_divergence_before_cps() {
    assert!(lowering::emit(&forged_no_else_never(true)).is_err());
}
