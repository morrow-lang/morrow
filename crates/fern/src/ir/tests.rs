use super::*;

#[test]
fn publication_and_qbe_reject_probes_even_after_unconditional_return() {
    let mut program =
        crate::check::check(&crate::parse::parse("fn main() -> Int: 0\n").unwrap()).unwrap();
    let value = program.functions[0].body.clone();
    let probe = Expr {
        kind: ExprKind::Probe {
            token: ProbeToken::new(0),
            children: vec![value.clone()],
            bindings: vec![],
        },
        ty: Type::Int,
        span: Span::default(),
    };
    let returning = Expr {
        kind: ExprKind::Return(Box::new(value)),
        ty: Type::Never,
        span: Span::default(),
    };
    program.functions[0].body.kind =
        ExprKind::Block(vec![Stmt::Expr(returning), Stmt::Expr(probe)]);
    assert!(
        reject_probes(&program)
            .unwrap_err()
            .message
            .contains("inference probe")
    );
    assert!(
        crate::lowering::emit(&program)
            .unwrap_err()
            .message
            .contains("inference probe")
    );
}

#[test]
fn shared_child_traversal_retains_probe_operands_in_source_order() {
    let values = [1, 2]
        .into_iter()
        .map(|n| Expr {
            kind: ExprKind::Int(n),
            ty: Type::Int,
            span: Span::default(),
        })
        .collect();
    let probe = Expr {
        kind: ExprKind::Probe {
            token: ProbeToken::new(0),
            children: values,
            bindings: vec![],
        },
        ty: Type::Int,
        span: Span::default(),
    };
    let values = children(&probe);
    assert!(matches!(values[0].kind, ExprKind::Int(1)));
    assert!(matches!(values[1].kind, ExprKind::Int(2)));
}

#[test]
fn executable_boundaries_reject_editor_holes_even_when_unreachable() {
    let mut program =
        crate::check::check(&crate::parse::parse("fn main() -> Int: 0\n").unwrap()).unwrap();
    let value = program.functions[0].body.clone();
    let hole = Expr {
        kind: ExprKind::EditorHole {
            token: EditorHoleToken::new(),
            receiver: Box::new(value.clone()),
        },
        ty: Type::Int,
        span: Span::default(),
    };
    let returning = Expr {
        kind: ExprKind::Return(Box::new(value)),
        ty: Type::Never,
        span: Span::default(),
    };
    program.functions[0].body.kind = ExprKind::Block(vec![Stmt::Expr(returning), Stmt::Expr(hole)]);
    assert!(
        reject_probes(&program)
            .unwrap_err()
            .message
            .contains("editor hole")
    );
    assert!(
        crate::lowering::emit(&program)
            .unwrap_err()
            .message
            .contains("editor hole")
    );
}

#[test]
fn unboxed_layers_cannot_hide_private_editor_or_inference_nodes() {
    for editor in [true, false] {
        let mut program = crate::check::check(
            &crate::parse::parse("newtype Id=Id(Int)\nfn main()->Int:Id(0).0\n").unwrap(),
        )
        .unwrap();
        let leaf = Expr {
            kind: ExprKind::Int(0),
            ty: Type::Int,
            span: Span::default(),
        };
        let kind = if editor {
            ExprKind::EditorHole {
                token: EditorHoleToken::new(),
                receiver: Box::new(leaf),
            }
        } else {
            ExprKind::Probe {
                token: ProbeToken::new(0),
                children: vec![leaf],
                bindings: vec![],
            }
        };
        let hidden = Expr {
            kind,
            ty: Type::Int,
            span: Span::default(),
        };
        let wrapped = Expr {
            kind: ExprKind::Wrap(Box::new(hidden)),
            ty: Type::Named("Id".into(), vec![]),
            span: Span::default(),
        };
        program.functions[0].body = Expr {
            kind: ExprKind::Unwrap(Box::new(wrapped)),
            ty: Type::Int,
            span: Span::default(),
        };
        let message = if editor {
            "editor hole"
        } else {
            "inference probe"
        };
        assert!(
            reject_probes(&program)
                .unwrap_err()
                .message
                .contains(message)
        );
        assert!(
            crate::lowering::emit(&program)
                .unwrap_err()
                .message
                .contains(message)
        );
    }
}

#[test]
fn inactive_union_conversions_cannot_hide_private_nodes() {
    for editor in [false, true] {
        for widening in [false, true] {
            let source = "fn unused()->Int | String: 1\nfn main(): ()\n";
            let mut program = crate::check::check(&crate::parse::parse(source).unwrap()).unwrap();
            let function = program
                .functions
                .iter_mut()
                .find(|f| f.name == "unused")
                .unwrap();
            let leaf = function.body.clone();
            let kind = if editor {
                ExprKind::EditorHole {
                    token: EditorHoleToken::new(),
                    receiver: Box::new(leaf.clone()),
                }
            } else {
                ExprKind::Probe {
                    token: ProbeToken::new(0),
                    children: vec![leaf.clone()],
                    bindings: vec![],
                }
            };
            let hidden = Box::new(Expr {
                kind,
                ty: leaf.ty.clone(),
                span: leaf.span,
            });
            let kind = if widening {
                ExprKind::UnionWiden { value: hidden }
            } else {
                ExprKind::UnionInject { value: hidden }
            };
            let conversion = Expr {
                kind,
                ty: leaf.ty.clone(),
                span: leaf.span,
            };
            let returning = Expr {
                kind: ExprKind::Return(Box::new(leaf)),
                ty: Type::Never,
                span: Span::default(),
            };
            function.body.kind =
                ExprKind::Block(vec![Stmt::Expr(returning), Stmt::Expr(conversion)]);
            let message = if editor {
                "editor hole"
            } else {
                "inference probe"
            };
            assert!(
                reject_probes(&program)
                    .unwrap_err()
                    .message
                    .contains(message)
            );
            assert!(
                crate::lowering::emit(&program)
                    .unwrap_err()
                    .message
                    .contains(message)
            );
        }
    }
}

#[test]
fn unused_codec_inputs_cannot_hide_private_nodes() {
    for editor in [false, true] {
        let source = "fn unused()->Result(String,json.Error):json.encode(1)\nfn main():()\n";
        let mut program = crate::check::check(&crate::parse::parse(source).unwrap()).unwrap();
        let ExprKind::JsonCodec { input, .. } = &mut program.functions[0].body.kind else {
            panic!("codec")
        };
        input.kind = if editor {
            ExprKind::EditorHole {
                token: EditorHoleToken::new(),
                receiver: Box::new((**input).clone()),
            }
        } else {
            ExprKind::Probe {
                token: ProbeToken::new(0),
                children: vec![(**input).clone()],
                bindings: vec![],
            }
        };
        let message = if editor {
            "editor hole"
        } else {
            "inference probe"
        };
        assert!(
            reject_probes(&program)
                .unwrap_err()
                .message
                .contains(message)
        );
        assert!(
            crate::lowering::emit(&program)
                .unwrap_err()
                .message
                .contains(message)
        );
    }
}

fn codec_template_program() -> Program {
    let mut p = crate::check::check(&crate::parse::parse("fn main()->Int:1\n").unwrap()).unwrap();
    let input = p.functions[0].body.clone();
    p.functions[0].body = Expr {
        span: Span::default(),
        ty: Type::Result(
            Box::new(Type::String),
            Box::new(Type::Native(crate::runtime::NativeType::JsonError)),
        ),
        kind: ExprKind::JsonCodecTemplate {
            direction: crate::json_codec::Direction::Encode,
            target: Type::Int,
            input: Box::new(input),
            token: CodecTemplateToken::new(),
        },
    };
    p
}
#[test]
fn codec_templates_are_private_to_validated_generic_proofs() {
    let p = codec_template_program();
    validate_codec_templates(&p, &mut 0).unwrap();
    assert!(
        reject_probes(&p)
            .unwrap_err()
            .message
            .contains("codec template")
    );
    assert!(
        crate::lowering::emit(&p)
            .unwrap_err()
            .message
            .contains("codec template")
    );
}
#[test]
fn codec_template_proof_rejects_mismatched_targets_and_hidden_holes() {
    let mut p = codec_template_program();
    if let ExprKind::JsonCodecTemplate { target, .. } = &mut p.functions[0].body.kind {
        *target = Type::Bool;
    }
    assert!(validate_codec_templates(&p, &mut 0).is_err());
    for editor in [false, true] {
        let mut p = codec_template_program();
        if let ExprKind::JsonCodecTemplate { input, .. } = &mut p.functions[0].body.kind {
            let leaf = input.clone();
            input.kind = if editor {
                ExprKind::EditorHole {
                    token: EditorHoleToken::new(),
                    receiver: leaf,
                }
            } else {
                ExprKind::Probe {
                    token: ProbeToken::new(0),
                    children: vec![*leaf],
                    bindings: vec![],
                }
            };
        }
        assert!(validate_codec_templates(&p, &mut 0).is_err());
    }
}

#[test]
fn template_validation_budget_is_shared_across_separate_generic_bodies() {
    let p = codec_template_program();
    let mut work = 399_995;
    assert!(
        validate_codec_templates(&p, &mut work)
            .unwrap_err()
            .message
            .contains("work limit")
    );
    let before = work;
    assert!(validate_codec_templates(&p, &mut work).is_err());
    assert!(work > before);
}
#[test]
fn unused_templates_cannot_hide_in_inactive_functions_or_test_publication() {
    let mut p = codec_template_program();
    let mut inactive = p.functions[0].clone();
    inactive.name = "inactive".into();
    inactive.id = FunctionId(1);
    p.functions[0].body = Expr {
        kind: ExprKind::Int(0),
        ty: Type::Int,
        span: Span::default(),
    };
    p.functions.push(inactive);
    assert!(reject_probes(&p).is_err());
    assert!(crate::lowering::emit(&p).is_err());
    let ast=crate::parse::parse("fn write(value):json.encode(value)\nfn test_codec()->Result(Unit,json.Error):\n    let text=write(42)?\n    println(text)\n    Ok(())\n").unwrap();
    let checked = crate::check::check_test(&ast, "test_codec").unwrap();
    reject_probes(&checked).unwrap();
}
