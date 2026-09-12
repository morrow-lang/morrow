//! Function summaries must preserve every returned value before source-call composition.
use super::*;
fn analyze(source: &str, name: &str) -> Checked<Summary> {
    let source = crate::parse::parse(source).unwrap();
    let program = super::super::pipeline_mode(&source, |_, _, _| Ok(()), false)
        .unwrap()
        .0;
    analyze_function(
        &program,
        program.functions.iter().find(|f| f.name == name).unwrap(),
    )
}
/// Inspect exported output provenance using the summary's own bounded predicate identity space.
fn output_origins(summary: Summary, outer: bool) -> BTreeMap<usize, Predicate> {
    let program = ir::Program::default();
    let mut engine = Engine::new(&program);
    engine.predicates = summary.predicates;
    engine
        .origins_of(&summary.output, outer, Span::default())
        .unwrap()
}
#[test]
fn explicit_returns_are_retained_in_summary_output() {
    let summary=analyze("fn take(flag:Bool, r:Result(Int,String))->Result(Int,String):\n    if flag:return r\n    r\n", "take").unwrap();
    let output = output_origins(summary, false);
    assert_eq!(output.len(), 1);
    assert_eq!(output.get(&0), Some(&Predicate::TRUE));
    let summary = analyze(
        "fn choose(flag:Bool)->Result(Int,String):\n    if flag:return Err(\"first\")\n    Ok(2)\n",
        "choose",
    )
    .unwrap();
    assert_eq!(output_origins(summary, false).len(), 2);
}
#[test]
fn propagated_returns_keep_a_new_outer_error_layer_and_original_nested_payload() {
    let summary = analyze(
        "fn take(r:Result(Int,Result(Int,String)))->Result(Int,Result(Int,String)):\n    Ok(r?)\n",
        "take",
    )
    .unwrap();
    let output = output_origins(summary, false);
    assert_eq!(
        output.len(),
        3,
        "both new outer constructors plus the nested Err payload"
    );
    assert!(
        !output.contains_key(&0),
        "observed input outer duty is not the produced output outer duty"
    );
}
#[test]
fn borrowing_helpers_keep_caller_duties_while_handler_helpers_discharge_them() {
    let helper = "fn count(xs:List(Result(Int,String)))->Int:List.len(xs)\n";
    let error = analyze(
        &format!("{helper}fn main():count([Err(\"lost\")])\n"),
        "main",
    )
    .unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
    analyze(&format!("{helper}fn main():\n    let xs:List(Result(Int,String))=[Err(\"handled\")]\n    println(count(xs))\n    Result.unwrap_or(List.head(xs),0)\n"),"main").unwrap();
    analyze("fn handle(r:Result(Int,String))->Int:Result.unwrap_or(r,0)\nfn main():handle(Err(\"handled\"))\n","main").unwrap();
}
#[test]
fn source_identity_and_explicit_return_summaries_preserve_original_input_origins() {
    analyze("fn identity(r:Result(Int,String))->Result(Int,String):r\nfn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    println(Result.unwrap_or(identity(r),0))\n","main").unwrap();
    analyze("fn pick(flag:Bool,r:Result(Int,String))->Result(Int,String):\n    if flag:return r\n    r\nfn main():Result.unwrap_or(pick(flag:true,r:Err(\"handled\")),0)\n","main").unwrap();
}
#[test]
fn helper_path_predicates_are_substituted_from_actual_boolean_arguments() {
    let helper = "fn maybe(flag:Bool,r:Result(Int,String))->Int:\n    if flag:Result.unwrap_or(r,0) else:0\n";
    analyze(
        &format!("{helper}fn main():maybe(flag:true,r:Err(\"handled\"))\n"),
        "main",
    )
    .unwrap();
    let error = analyze(
        &format!("{helper}fn main():maybe(flag:false,r:Err(\"lost\"))\n"),
        "main",
    )
    .unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}
#[test]
fn distinct_calls_produce_distinct_obligations_and_selected_inputs_do_not_merge() {
    let make = "fn make()->Result(Int,String):Err(\"fresh\")\n";
    let error=analyze(&format!("{make}fn main():\n    let a=make()\n    let b=make()\n    println(Result.is_ok(a))\n    println(List.len([b]))\n"),"main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
    let source = "fn pick(flag:Bool,a:Result(Int,String),b:Result(Int,String))->Result(Int,String):if flag:a else:b\nfn bad(flag:Bool):\n    let a:Result(Int,String)=Err(\"a\")\n    let b:Result(Int,String)=Err(\"b\")\n    Result.unwrap_or(pick(flag:flag,a:a,b:b),0)\n";
    let error = analyze(source, "bad").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}
#[test]
fn nested_and_multi_variant_input_shapes_keep_conditional_origin_identity() {
    analyze("fn handle(r:Result(Result(Int,String),Result(Int,String)))->Int:\n    match r:\n        Ok(inner)->Result.unwrap_or(inner,0)\n        Err(inner)->Result.unwrap_or(inner,0)\nfn main():handle(Err(Err(\"handled\")))\n","main").unwrap();
    let source = "type Three:\n    A(Result(Int,String))\n    B(Result(Int,String))\n    C(Result(Int,String))\nfn handle(v:Three)->Int:\n    match v:\n        A(r)->Result.unwrap_or(r,0)\n        B(r)->Result.unwrap_or(r,0)\n        C(r)->Result.unwrap_or(r,0)\nfn main():handle(C(Err(\"handled\")))\n";
    analyze(source, "main").unwrap();
}
#[test]
fn generic_instances_keep_borrowing_and_input_aliases_without_cross_contamination() {
    let identity = "fn identity(value:a)->a:value\n";
    analyze(&format!("{identity}fn main():\n    println(identity(1))\n    let r:Result(Int,String)=Err(\"handled\")\n    Result.unwrap_or(identity(r),0)\n"),"main").unwrap();
    let source = "fn count(values:List(a))->Int:List.len(values)\nfn main():\n    println(count([1]))\n    let xs:List(Result(Int,String))=[Err(\"lost\")]\n    count(xs)\n";
    let error = analyze(source, "main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}
#[test]
fn summaries_borrow_dependent_on_actual_variant_and_do_not_merge_distinct_parameters() {
    let source = "fn handle(v:Result(Result(Int,String),String))->Int:\n    match v:\n        Ok(inner)->Result.unwrap_or(inner,0)\n        Err(_)->0\nfn main():handle(Err(\"no phantom Ok payload\"))\n";
    analyze(source, "main").unwrap();
    let source = "fn first(a:Result(Int,String),b:Result(Int,String))->Result(Int,String):\n    println(Result.is_ok(b))\n    a\nfn main():\n    let r:Result(Int,String)=Err(\"shared\")\n    Result.unwrap_or(first(a:r,b:r),0)\n";
    analyze(source, "main").unwrap();
}
#[test]
fn whole_conditional_collection_inputs_remain_transferable_through_helpers() {
    let source = "fn identity(xs:List(Result(Int,String)))->List(Result(Int,String)):xs\nfn pass(flag:Bool)->List(Result(Int,String)):\n    let xs:List(Result(Int,String))=if flag:[Ok(1)] else:[Err(\"returned\")]\n    identity(xs)\n";
    analyze(source, "pass").unwrap();
}
#[test]
fn direct_callable_aliases_keep_intrinsic_borrowing_and_actual_handler_effects() {
    let error=analyze("fn main():\n    let count:(List(Result(Int,String)))->Int=List.len\n    count([Err(\"lost\")])\n","main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
    analyze("fn main():\n    let handle:(Result(Int,String))->Int=(r)->Result.unwrap_or(r,0)\n    let alias=handle\n    alias(Err(\"handled\"))\n","main").unwrap();
}
#[test]
fn captured_boolean_and_returned_callable_provenance_remain_correlated() {
    let source = "fn make(flag:Bool)->(Result(Int,String))->Int:\n    (r)->if flag:Result.unwrap_or(r,0) else:0\nfn main():\n    let handle=make(flag:true)\n    handle(Err(\"handled\"))\n";
    analyze(source, "main").unwrap();
    let error = analyze(&source.replace("flag:true", "flag:false"), "main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}
#[test]
fn first_class_runtime_results_create_the_same_pending_duties_as_direct_calls() {
    analyze("fn main()->Result((),Int):\n    let write:(String)->Result((),Int)=System.write_stderr\n    write(\"handled\")?\n    Ok(())\n","main").unwrap();
}
#[test]
fn sum_callbacks_execute_only_the_selected_path_and_preserve_nested_duties() {
    analyze("fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    Result.unwrap_or_else(r,(error)->String.len(error))\n","main").unwrap();
    analyze("fn main():\n    let r:Result(Int,String)=Ok(2)\n    Result.unwrap_or(Result.map(r,(n)->n+1),0)\n","main").unwrap();
    analyze("fn main():\n    let r:Result(Int,String)=Ok(2)\n    Result.unwrap_or(Result.and_then(r,(n)->Ok(n+1)),0)\n","main").unwrap();
    let source = "fn main():\n    let nested:Result(Result(Int,String),String)=Ok(Err(\"lost\"))\n    Result.unwrap_or(Result.map(nested,(r)->List.len([r])),0)\n";
    let error = analyze(source, "main").unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}
#[test]
fn map_metadata_in_a_result_handling_body_does_not_require_a_fabricated_effect() {
    analyze("fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    let values:Map(Int,Float)=%{1:2.5}\n    println(Option.unwrap_or(Map.get(values,1),0.0))\n    Result.unwrap_or(r,0)\n","main").unwrap();
}

#[test]
fn callable_parameter_summaries_use_actual_targets_without_borrowing_as_handling() {
    let helper = "fn apply(f:(Result(Int,String))->Int,r:Result(Int,String))->Int:f(r)\n";
    analyze(
        &format!("{helper}fn main():apply((r)->Result.unwrap_or(r,0),Err(\"handled\"))\n"),
        "main",
    )
    .unwrap();
    let bad = analyze(
        &format!("{helper}fn main():apply((r)->List.len([r]),Err(\"lost\"))\n"),
        "main",
    )
    .unwrap_err();
    assert!(bad.message.contains("remains unhandled"), "{bad:?}");
}
#[test]
fn callable_parameter_results_keep_input_identity_and_captured_conditions() {
    let helper = "fn apply(f:(Result(Int,String))->Result(Int,String),r:Result(Int,String))->Result(Int,String):f(r)\n";
    analyze(&format!("{helper}fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    Result.unwrap_or(apply((x)->x,r),0)\n"),"main").unwrap();
    let helper = "fn apply(f:(Result(Int,String))->Int,r:Result(Int,String))->Int:f(r)\nfn make(flag:Bool)->(Result(Int,String))->Int:(r)->if flag:Result.unwrap_or(r,0) else:0\n";
    analyze(
        &format!("{helper}fn main():apply(make(flag:true),Err(\"handled\"))\n"),
        "main",
    )
    .unwrap();
    let bad = analyze(
        &format!("{helper}fn main():apply(make(flag:false),Err(\"lost\"))\n"),
        "main",
    )
    .unwrap_err();
    assert!(bad.message.contains("remains unhandled"), "{bad:?}");
}
