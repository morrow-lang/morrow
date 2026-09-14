//! Reachable-path regressions for stage B; source check remains unchanged until proof integration.
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
fn rejected(source: &str, name: &str) {
    let error = analyze(source, name).unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}
#[test]
fn every_normal_branch_must_handle_an_owned_result() {
    rejected(
        "fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"lost\")\n    if flag:Result.unwrap_or(r,0) else:1\n",
        "choose",
    );
    analyze("fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"handled\")\n    if flag:Result.unwrap_or(r,0) else:Result.unwrap_or(r,1)\n","choose").unwrap();
}
#[test]
fn freshly_created_branch_results_can_be_handled_after_the_join() {
    analyze("fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=if flag:Ok(1) else:Err(\"handled\")\n    Result.unwrap_or(r,0)\n","choose").unwrap();
}
#[test]
fn phi_alias_cannot_acknowledge_both_independent_originals() {
    let prefix = "fn choose(flag:Bool)->Int:\n    let a:Result(Int,String)=Err(\"a\")\n    let b:Result(Int,String)=Err(\"b\")\n    let selected=if flag:a else:b\n    println(Result.unwrap_or(selected,0))\n";
    rejected(&format!("{prefix}    0\n"), "choose");
    analyze(
        &format!("{prefix}    println(Result.unwrap_or(a,0))\n    Result.unwrap_or(b,0)\n"),
        "choose",
    )
    .unwrap();
}
#[test]
fn early_return_and_short_circuit_paths_preserve_pending_duties() {
    rejected(
        "fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"lost\")\n    if flag:return 1\n    Result.unwrap_or(r,0)\n",
        "choose",
    );
    analyze("fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"handled\")\n    if flag:return Result.unwrap_or(r,1)\n    Result.unwrap_or(r,0)\n","choose").unwrap();
    rejected(
        "fn choose(flag:Bool)->Bool:\n    let r:Result(Int,String)=Err(\"lost\")\n    flag and Result.is_ok(r)\n",
        "choose",
    );
}
#[test]
fn propagation_checks_earlier_duties_on_its_error_return() {
    rejected(
        "fn fail()->Result(Int,String):\n    let earlier:Result(Int,String)=Err(\"lost\")\n    let current:Result(Int,String)=Err(\"stop\")\n    let n=current?\n    Ok(Result.unwrap_or(earlier,n))\n",
        "fail",
    );
    analyze("fn pass()->Result(Int,String):\n    let earlier:Result(Int,String)=Err(\"handled\")\n    println(Result.unwrap_or(earlier,0))\n    let current:Result(Int,String)=Err(\"stop\")\n    Ok(current?)\n","pass").unwrap();
}
#[test]
fn explicit_patterns_handle_outer_tags_but_wildcards_do_not() {
    analyze("fn handle(r:Result(Int,String))->Int:\n    match r:\n        Ok(value)->value\n        Err(_)->0\n","handle").unwrap();
    rejected(
        "fn main():\n    let r:Result(Int,String)=Err(\"lost\")\n    match r:\n        _->()\n",
        "main",
    );
}
#[test]
fn conditional_nested_payloads_create_no_phantom_inactive_duties() {
    analyze("fn handle(r:Result(Result(Int,String),String))->Int:\n    match r:\n        Ok(inner)->Result.unwrap_or(inner,0)\n        Err(_)->0\n","handle").unwrap();
    rejected(
        "fn main():\n    let r:Result(Result(Int,String),String)=Ok(Err(\"lost\"))\n    match r:\n        whole->println(Result.is_ok(whole))\n",
        "main",
    );
}
#[test]
fn registered_defer_handles_captured_duties_on_every_actual_exit() {
    analyze("fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"handled\")\n    defer println(Result.unwrap_or(r,0))\n    if flag:return 1\n    2\n","choose").unwrap();
    rejected(
        "fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"lost\")\n    if flag:defer println(Result.unwrap_or(r,0))\n    2\n",
        "choose",
    );
}

#[test]
fn guard_return_cannot_be_resurrected_as_a_later_match_arm() {
    rejected(
        "fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"lost\")\n    match 0:\n        n if (if flag:return 1 else:false)->Result.unwrap_or(r,0)\n        _->Result.unwrap_or(r,0)\n",
        "choose",
    );
}
#[test]
fn let_else_and_with_keep_failure_paths_and_transferred_payloads() {
    analyze(
        "fn take(value:Result(Int,String))->Int:\n    let Ok(n)=value else:return 0\n    n\n",
        "take",
    )
    .unwrap();
    rejected(
        "fn main():\n    let prior:Result(Int,String)=Err(\"lost\")\n    let candidate:Result(Int,String)=Err(\"stop\")\n    let Ok(n)=candidate else:return ()\n    println(Result.unwrap_or(prior,n))\n",
        "main",
    );
    analyze("fn take(value:Result(Int,String))->Int:\n    with\n        n<-value\n    do\n        n\n    else\n        Err(_)->0\n","take").unwrap();
    rejected(
        "fn take()->Int:\n    let prior:Result(Int,String)=Err(\"lost\")\n    with\n        n<-Err(\"stop\")\n    do\n        Result.unwrap_or(prior,n)\n    else\n        Err(_)->0\n",
        "take",
    );
}
#[test]
fn complementary_boolean_paths_and_conditional_input_duties_remain_provable() {
    let result=analyze("fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"handled\")\n    if flag:println(Result.unwrap_or(r,0))\n    if not flag:println(Result.unwrap_or(r,0))\n    0\n","choose").unwrap();
    assert!(result.decision_nodes() > 0);
    let result=analyze("fn handle(r:Result(Result(Int,String),String))->Int:\n    match r:\n        Ok(inner)->Result.unwrap_or(inner,0)\n        Err(_)->0\n","handle").unwrap();
    assert_eq!(result.borrowed_inputs(), 0);
    assert_eq!(result.handled_inputs(), 2);
}
#[test]
fn sequential_independent_branches_keep_a_small_decision_graph() {
    use std::fmt::Write;
    let params = (0..80)
        .map(|i| format!("f{i}:Bool"))
        .collect::<Vec<_>>()
        .join(",");
    let mut source =
        format!("fn wide({params})->Int:\n    let r:Result(Int,String)=Err(\"handled\")\n");
    for i in 0..80 {
        writeln!(&mut source, "    if f{i}:println(0)").unwrap();
    }
    source.push_str("    Result.unwrap_or(r,0)\n");
    let result = analyze(&source, "wide").unwrap();
    assert!(result.decision_nodes() < 1000);
}

#[test]
fn one_computed_boolean_value_keeps_its_identity_across_aliases() {
    analyze("fn choose(xs:List(Int))->Int:\n    let r:Result(Int,String)=Err(\"handled\")\n    let empty=List.is_empty(xs)\n    if empty:println(Result.unwrap_or(r,0))\n    if not empty:println(Result.unwrap_or(r,0))\n    0\n","choose").unwrap();
}
