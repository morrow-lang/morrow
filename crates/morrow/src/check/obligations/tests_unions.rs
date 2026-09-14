//! Union tags route provenance without acknowledging contained Result tags.
use super::*;

fn analyze(source: &str, name: &str) -> Checked<Summary> {
    let ast = crate::parse::parse(source).unwrap();
    let program = super::super::pipeline_mode(&ast, |_, _, _| Ok(()), false)
        .unwrap()
        .0;
    let function = program.functions.iter().find(|f| f.name == name).unwrap();
    analyze_function(&program, function)
}

const HANDLE: &str = "fn handle(x:Int|Result(Int,String))->Int:\n    match x:\n        n:Int->n\n        r:Result(Int,String)->Result.unwrap_or(r,0)\n";

#[test]
fn union_projection_never_upgrades_partial_family_coverage() {
    let program = ir::Program::default();
    let mut engine = Engine::new(&program);
    let span = Span::default();
    let result = Type::Result(Box::new(Type::Int), Box::new(Type::String));
    let ty = Type::Union(vec![Type::Int, result.clone()]);
    let original = engine.fresh(&ty, Some(0), span, 0).unwrap();
    let choices = engine
        .node(
            Region::Choice(vec![(Predicate::TRUE, original.clone())]),
            span,
        )
        .unwrap();
    for view in [original.partial(), choices.partial()] {
        let (_, selected) = engine.union_narrow(&view, &result, span, 0).unwrap();
        assert!(
            engine
                .origins_of(&selected, false, span)
                .unwrap()
                .is_empty()
        );
        let (_, subset) = engine.union_narrow(&view, &ty, span, 0).unwrap();
        assert!(engine.origins_of(&subset, false, span).unwrap().is_empty());
        let carrier = engine
            .union_carrier(&view, crate::unions::members(&ty), span, 0)
            .unwrap();
        assert!(!carrier.complete);
    }
    assert_eq!(engine.origins_of(&original, false, span).unwrap().len(), 1);
    assert!(engine.origins_of(&original, true, span).unwrap().is_empty());
}

#[test]
fn union_member_storage_and_walks_respect_the_shared_work_and_depth_budget() {
    let program = ir::Program::default();
    let mut engine = Engine::new(&program);
    let span = Span::default();
    let value = engine.node(Region::Empty, span).unwrap();
    engine.work = WORK_LIMIT;
    assert!(
        engine
            .union_value(&[Type::Int, Type::String], value.clone(), span)
            .is_err()
    );
    assert_eq!(
        engine.nodes, 1,
        "member tables must be charged before node allocation"
    );
    engine.work = 0;
    assert!(
        engine
            .union_narrow(&value, &Type::Int, span, DEPTH_LIMIT)
            .is_err()
    );
    assert!(
        engine
            .union_carrier(&value, &[Type::Int], span, DEPTH_LIMIT)
            .is_err()
    );
}

#[test]
fn injected_members_keep_original_result_origins_through_narrowing() {
    analyze(
        &format!("{HANDLE}fn main():handle(Err(\"handled\"))\n"),
        "main",
    )
    .unwrap();
    analyze(&format!("{HANDLE}fn main():handle(7)\n"), "main").unwrap();
    let summary = analyze(HANDLE, "handle").unwrap();
    assert_eq!(summary.handled_inputs(), 1);
    assert_eq!(summary.borrowed_inputs(), 0);
}

#[test]
fn union_identity_and_widening_remap_members_without_creating_new_debt() {
    let source = "fn widen(x:Result(Int,String)|String)->Bool|String|Result(Int,String):x\nfn handle(x:Bool|String|Result(Int,String))->Int:\n    match x:\n        r:Result(Int,String)->Result.unwrap_or(r,0)\n        _:Bool|String->0\nfn main():handle(widen(Err(\"handled\")))\n";
    analyze(source, "main").unwrap();
    analyze(&source.replace("Err(\"handled\")", "\"plain\""), "main").unwrap();
    let summary = analyze(source, "widen").unwrap();
    assert_eq!(summary.origins.len(), 1);
    assert_eq!(summary.handled_inputs(), 0);
    assert_eq!(summary.borrowed_inputs(), 0);
}

#[test]
fn subset_narrowing_retains_payloads_for_later_member_selection() {
    let source = format!(
        "{HANDLE}fn subset(x:Bool|Int|Result(Int,String))->Int:\n    match x:\n        part:Int|Result(Int,String)->handle(part)\n        _:Bool->0\nfn main():subset(Err(\"handled\"))\n"
    );
    analyze(&source, "main").unwrap();
    analyze(
        &source.replace("subset(Err(\"handled\"))", "subset(false)"),
        "main",
    )
    .unwrap();
}

#[test]
fn selecting_a_union_tag_does_not_handle_its_result_payload() {
    let source = "fn borrow(x:Int|Result(Int,String))->Int:\n    match x:\n        n:Int->n\n        r:Result(Int,String)->List.len([r])\nfn main():borrow(Err(\"lost\"))\n";
    let error = analyze(source, "main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
    let summary = analyze(source, "borrow").unwrap();
    assert_eq!(summary.handled_inputs(), 0);
    assert_eq!(summary.borrowed_inputs(), 1);
}

#[test]
fn nested_result_payloads_require_handling_after_union_and_outer_result_tests() {
    let source = "fn handle(x:Int|Result(Result(Int,String),String))->Int:\n    match x:\n        n:Int->n\n        r:Result(Result(Int,String),String)->match r:\n            Ok(inner)->List.len([inner])\n            Err(_)->0\nfn main():handle(Ok(Err(\"lost\")))\n";
    let error = analyze(source, "main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
    analyze(
        &source.replace("List.len([inner])", "Result.unwrap_or(inner,0)"),
        "main",
    )
    .unwrap();
}

#[test]
fn guarded_union_arms_preserve_caller_boolean_correlations() {
    let source = "fn handle(flag:Bool,x:Int|Result(Int,String))->Int:\n    match x:\n        r:Result(Int,String) if flag->Result.unwrap_or(r,0)\n        n:Int->n\n        r:Result(Int,String)->List.len([r])\nfn main():handle(flag:true,x:Err(\"handled\"))\n";
    analyze(source, "main").unwrap();
    let error = analyze(&source.replace("flag:true", "flag:false"), "main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}

#[test]
fn union_phi_choices_never_merge_independent_result_origins() {
    let source = format!(
        "{HANDLE}fn pick(flag:Bool):\n    let a:Result(Int,String)=Err(\"a\")\n    let b:Result(Int,String)=Err(\"b\")\n    let x:Int|Result(Int,String)=if flag:a else:b\n    handle(x)\n"
    );
    let error = analyze(&source, "pick").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
    analyze(
        &source.replace(
            "    handle(x)\n",
            "    handle(x)\n    Result.unwrap_or(a,0)\n    Result.unwrap_or(b,0)\n",
        ),
        "pick",
    )
    .unwrap();
}
