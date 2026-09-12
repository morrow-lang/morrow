//! Recovery must validate unrelated effects without treating the opaque hole as a handler.
use super::*;
fn recover(source: &str) -> Checked<super::super::editor::Facts> {
    let cursor = source.find("box.)").unwrap() + 4;
    let (ast, site) = crate::parse::recover_member(source, cursor).unwrap();
    super::super::recovery::analyze(&ast, &site)
}
#[test]
fn member_recovery_cannot_hide_unhandled_results_in_current_or_unaffected_functions() {
    let prefix = "type Box:\n    field:Int\n";
    let body = "fn inspect(box:Box)->Unit:\n    let xs:List(Result(Int,String))=[Err(\"lost\")]\n    println(List.len(xs))\n    println(box.)\n";
    let error = recover(&format!("{prefix}{body}"))
        .expect_err("forgotten Result must invalidate member facts");
    assert!(error.message.contains("Result obligation"), "{error:?}");
    let body = "fn bad()->Unit:\n    let xs:List(Result(Int,String))=[Err(\"lost\")]\n    println(List.len(xs))\nfn inspect(box:Box)->Unit:println(box.)\n";
    let error = recover(&format!("{prefix}{body}")).expect_err("unaffected bodies remain checked");
    assert!(error.message.contains("Result obligation"), "{error:?}");
}
#[test]
fn independently_handled_results_keep_opaque_member_recovery_usable() {
    let source = "type Box:\n    field:Int\nfn inspect(box:Box)->Unit:\n    let r:Result(Int,String)=Err(\"handled\")\n    println(Result.unwrap_or(r,0))\n    println(box.)\n";
    assert!(recover(source).is_ok());
}
#[test]
fn codec_effects_remain_checked_during_member_recovery() {
    let declaration = "type Box:\n    field:Int\nfn encode(value):json.encode(value)\n";
    let good =
        "fn inspect(box:Box)->Unit:\n    println(Result.is_ok(encode(1)))\n    println(box.)\n";
    assert!(recover(&format!("{declaration}{good}")).is_ok());
    let bad = "fn inspect(box:Box)->Unit:\n    let r=encode(1)\n    println(List.len([r]))\n    println(box.)\n";
    let error = recover(&format!("{declaration}{bad}")).unwrap_err();
    assert!(error.message.contains("Result obligation"), "{error:?}");
}
#[test]
fn inactive_private_codec_templates_cannot_enter_concrete_proof() {
    let ast = crate::parse::parse("fn main():()\n").unwrap();
    let mut program = super::super::pipeline_mode(&ast, |_, _, _| Ok(()), false)
        .unwrap()
        .0;
    let span = Span::default();
    let output = Type::Result(
        Box::new(Type::String),
        Box::new(Type::Native(crate::runtime::NativeType::JsonError)),
    );
    let template = ir::Expr {
        kind: ir::ExprKind::JsonCodecTemplate {
            direction: crate::json_codec::Direction::Encode,
            input: Box::new(ir::Expr {
                kind: ir::ExprKind::Int(1),
                ty: Type::Int,
                span,
            }),
            target: Type::Int,
            token: ir::CodecTemplateToken::new(),
        },
        ty: output,
        span,
    };
    program.functions[0].body = ir::Expr {
        kind: ir::ExprKind::If {
            condition: Box::new(ir::Expr {
                kind: ir::ExprKind::Bool(false),
                ty: Type::Bool,
                span,
            }),
            then_branch: Box::new(template),
            else_branch: Some(Box::new(program.functions[0].body.clone())),
        },
        ty: Type::Unit,
        span,
    };
    let error = gate::check(&program, 0).unwrap_err();
    assert!(error.message.contains("template"), "{error:?}");
}
#[test]
fn codec_validation_and_template_obligations_share_one_budget() {
    let ast = crate::parse::parse("fn main():()\n").unwrap();
    let program = super::super::pipeline_mode(&ast, |_, _, _| Ok(()), false)
        .unwrap()
        .0;
    let registry = super::super::nominal::Registry::new(&ast).unwrap();
    registry.codec_template_work.set(WORK_LIMIT);
    let roots = HashSet::from([program.functions[0].id.0]);
    let error = gate::templates(program.functions, &registry, &roots).unwrap_err();
    assert!(error.message.contains("limit"), "{error:?}");
}
