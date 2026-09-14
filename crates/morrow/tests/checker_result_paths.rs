use morrow_compiler::{check, parse};
fn checked(source: &str) -> Result<(), String> {
    let ast = parse::parse(source).map_err(|e| e.message)?;
    check::check_library(&ast)
        .map(|_| ())
        .map_err(|e| e.message)
}
fn rejected(source: &str) {
    let error = checked(source).expect_err(source);
    assert!(error.contains("Result obligation"), "{error}");
}
#[test]
fn metadata_is_borrowing_and_a_later_complete_handler_remains_valid() {
    rejected(
        "fn count(xs:List(Result(Int,String)))->Int:List.len(xs)\nfn main():count([Err(\"lost\")])\n",
    );
    checked("fn count(xs:List(Result(Int,String)))->Int:List.len(xs)\nfn main():\n    let xs:List(Result(Int,String))=[Err(\"handled\")]\n    println(count(xs))\n    println(Result.unwrap_or(List.head(xs),0))\n").unwrap();
}
#[test]
fn unit_and_unused_functions_cannot_hide_locally_created_results() {
    rejected(
        "fn unused()->Unit:\n    let xs:List(Result(Int,String))=[Err(\"lost\")]\n    println(List.len(xs))\nfn main():()\n",
    );
    rejected(
        "fn unused(value:a)->a:\n    let xs:List(Result(Int,String))=[Err(\"lost\")]\n    println(List.len(xs))\n    value\nfn main():()\n",
    );
}
#[test]
fn nested_callable_and_cleanup_bodies_are_inside_the_proof_boundary() {
    rejected(
        "fn main():\n    let run:()->Unit=()->\n        let xs:List(Result(Int,String))=[Err(\"lost\")]\n        println(List.len(xs))\n    run()\n",
    );
    rejected(
        "fn main():\n    let xs:List(Result(Int,String))=[Err(\"lost\")]\n    defer println(List.len(xs))\n",
    );
}
#[test]
fn branch_and_propagation_exits_cannot_skip_prior_duties() {
    rejected(
        "fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"lost\")\n    if flag:Result.unwrap_or(r,0) else:0\n",
    );
    rejected(
        "fn bad()->Result(Int,String):\n    let a:Result(Int,String)=Err(\"lost\")\n    let b:Result(Int,String)=Err(\"stop\")\n    let value=b?\n    Ok(Result.unwrap_or(a,value))\n",
    );
}
#[test]
fn generic_instances_and_function_valued_calls_never_imply_handling() {
    rejected(
        "fn count(xs:List(a))->Int:List.len(xs)\nfn main():\n    let xs:List(Result(Int,String))=[Err(\"lost\")]\n    count(xs)\n",
    );
    rejected(
        "fn main():\n    let make:()->Result(Int,String)=()->Err(\"lost\")\n    let r=make()\n    println(List.len([r]))\n",
    );
}
#[test]
fn mutually_recursive_forwarding_does_not_prove_its_own_handling() {
    rejected(
        "fn first(flag:Bool,r:Result(Int,String))->Int:if flag:second(flag:false,r:r) else:0\nfn second(flag:Bool,r:Result(Int,String))->Int:first(flag:flag,r:r)\nfn main():first(flag:true,r:Err(\"lost\"))\n",
    );
}
#[test]
fn guaranteed_defer_and_returned_aliases_remain_valid() {
    checked("fn choose(flag:Bool)->Int:\n    let r:Result(Int,String)=Err(\"handled\")\n    defer println(Result.unwrap_or(r,0))\n    if flag:return 1\n    2\n").unwrap();
    checked("fn identity(r:Result(Int,String))->Result(Int,String):r\nfn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    Result.unwrap_or(identity(r),0)\n").unwrap();
}
#[test]
fn generic_result_templates_preserve_unknown_payload_aliases_without_a_witness_type() {
    checked("fn recover(value:Result(a,String))->Option(a):\n    match value:\n        Ok(inner)->Some(inner)\n        Err(_)->None\nfn main():\n    let value:Result(Int,String)=Ok(1)\n    println(Option.unwrap_or(recover(value),0))\n").unwrap();
    checked("fn recover(value:Result(a,String))->Option(a):\n    match value:\n        Ok(inner)->Some(inner)\n        Err(_)->None\nfn main():()\n").unwrap();
    rejected(
        "fn recover(value:Result(a,String))->Option(a):\n    match value:\n        Ok(inner)->Some(inner)\n        Err(_)->None\nfn main():\n    let value:Result(Result(Int,String),String)=Ok(Err(\"lost\"))\n    println(Option.is_some(recover(value)))\n",
    );
}

#[test]
fn generic_callable_parameters_preserve_actual_handler_and_alias_contracts() {
    let apply = "fn apply(callback:(a)->b,value:a)->b:callback(value)\n";
    checked(&format!("{apply}fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    println(apply((x)->Result.unwrap_or(x,0),r))\n")).unwrap();
    checked(&format!("{apply}fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    Result.unwrap_or(apply((x)->x,r),0)\n")).unwrap();
    rejected(&format!(
        "{apply}fn main():\n    let r:Result(Int,String)=Err(\"lost\")\n    apply((x)->List.len([x]),r)\n"
    ));
    let compose = "fn compose(first:(b)->c,second:(a)->b)->(a)->c:(value)->first(second(value))\n";
    checked(&format!("{compose}fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    compose((x)->Result.unwrap_or(x,0),(x)->x)(r)\n")).unwrap();
}

#[test]
fn conditional_callback_requirements_cannot_hide_unrelated_or_partially_passed_errors() {
    rejected(
        "fn bad(f:(Int)->Int)->Int:\n    let r:Result(Int,String)=Err(\"unrelated\")\n    println(List.len([r]))\n    f(1)\nfn main():()\n",
    );
    let compose = "fn compose(first:(b)->c,second:(a)->b)->(a)->c:(value)->first(second(value))\n";
    rejected(&format!(
        "{compose}fn main():compose((r:Result(Int,String))->List.len([r]),(n:Int)->Err(\"lost\"))(1)\n"
    ));
    let handler = "fn run(callback:(Result(Int,String))->Int)->Int:callback(Err(\"owned\"))\n";
    checked(&format!(
        "{handler}fn main():run((r)->Result.unwrap_or(r,0))\n"
    ))
    .unwrap();
    rejected(&format!("{handler}fn main():run((r)->List.len([r]))\n"));
}

#[test]
fn representative_iteration_keeps_outer_debt_and_checks_fresh_body_origins() {
    rejected(
        "fn main():\n    let r:Result(Int,String)=Err(\"outer\")\n    let empty:List(Int)=[]\n    for n in empty:\n        println(Result.unwrap_or(r,n))\n    println(List.len([r]))\n",
    );
    rejected(
        "fn main():\n    for n in [1,2]:\n        let r:Result(Int,String)=Err(\"per iteration\")\n        println(List.len([r]))\n",
    );
    checked("fn main():\n    for n in [1,2]:\n        let r:Result(Int,String)=Err(\"handled\")\n        println(Result.unwrap_or(r,n))\n").unwrap();
}

#[test]
fn empty_mapping_through_a_source_helper_creates_no_phantom_callback_results() {
    checked("fn produce(values:List(Int))->List(Result(Int,String)):List.map(values,(n)->Err(\"produced\"))\nfn main():\n    let empty:List(Int)=[]\n    println(List.len(produce(empty)))\n").unwrap();
    rejected(
        "fn produce(values:List(Int))->List(Result(Int,String)):List.map(values,(n)->Err(\"produced\"))\nfn main():println(List.len(produce([1])))\n",
    );
}

#[test]
fn early_exit_hofs_still_validate_actual_callback_owned_results() {
    let factory = "fn factory(handle:(Result(Int,String))->Int)->(Int)->Bool:\n    (n)->handle(Err(\"owned\"))>n\n";
    for operation in ["any", "all", "find"] {
        rejected(&format!(
            "{factory}fn main():\n    let callback=factory((r)->List.len([r]))\n    let value=List.{operation}([1],callback)\n    println(1)\n"
        ));
    }
}

#[test]
fn fold_accumulated_fresh_results_remain_a_family_after_many_iterations() {
    let helper = "fn add(acc:List(Result(Int,String)),n:Int)->List(Result(Int,String)):List.push(acc,Err(\"fresh\"))\n";
    rejected(&format!(
        "{helper}fn main():\n    let errors=List.fold([1,2,3],[],add)\n    Result.unwrap_or(List.head(errors),0)\n"
    ));
    checked(&format!("{helper}fn main():\n    let errors=List.fold([1,2,3],[],add)\n    for error in errors:println(Result.unwrap_or(error,0))\n")).unwrap();
    checked(&format!("{helper}fn all()->List(Result(Int,String)):List.fold([1,2,3],[],add)\nfn main():\n    for error in all():println(Result.unwrap_or(error,0))\n")).unwrap();
}

#[test]
fn closed_recursive_signatures_check_every_body_without_self_discharge() {
    checked("fn descend(n:Int)->Int:\n    let r:Result(Int,String)=Ok(n)\n    let value=Result.unwrap_or(r,0)\n    if value==0:0 else:ascend(value-1)\nfn ascend(n:Int)->Int:descend(n)\nfn main():println(descend(4))\n").unwrap();
    rejected(
        "fn descend(n:Int)->Int:\n    let r:Result(Int,String)=Err(\"lost\")\n    println(List.len([r]))\n    if n==0:0 else:ascend(n-1)\nfn ascend(n:Int)->Int:descend(n)\nfn main():()\n",
    );
    rejected(
        "fn first(n:Int)->Int:if n==0:0 else:second(n-1)\nfn second(n:Int)->Int:\n    let r:Result(Int,String)=Err(\"lost in other member\")\n    println(List.len([r]))\n    first(n)\nfn main():println(first(2))\n",
    );
}

#[test]
fn dynamic_prefix_and_rest_form_distinct_complete_partitions() {
    let prefix = "fn handle(xs:List(Result(Int,String)))->Int:\n    match xs:\n        []->0\n        [head,..tail]->\n            println(Result.unwrap_or(head,0))\n            for r in tail:println(Result.unwrap_or(r,0))\n            1\n";
    checked(&format!(
        "{prefix}fn main():handle([Ok(1),Err(\"second\"),Err(\"third\")])\n"
    ))
    .unwrap();
    let partial = "fn handle(xs:List(Result(Int,String)))->Int:\n    match xs:\n        []->0\n        [head,..tail]->\n            println(List.len(tail))\n            Result.unwrap_or(head,0)\n";
    rejected(&format!(
        "{partial}fn main():handle([Ok(1),Err(\"second\"),Err(\"third\")])\n"
    ));
    let partial = "fn handle(xs:List(Result(Int,String)))->Int:\n    match xs:\n        []->0\n        [head,..tail]->\n            println(List.len([head]))\n            for r in tail:println(Result.unwrap_or(r,0))\n            1\n";
    rejected(&format!(
        "{partial}fn main():handle([Err(\"first\"),Ok(2)])\n"
    ));
}

#[test]
fn dynamic_prefix_patterns_preserve_existing_generic_and_capture_programs() {
    checked(include_str!("sequences/generic.fn")).unwrap();
    checked(include_str!("sequences/escape.fn")).unwrap();
}

#[test]
fn traversing_a_projected_nested_collection_does_not_upgrade_partial_coverage() {
    let helper = "fn handle(groups:List(List(Result(Int,String))))->Unit:\n    let first=List.head(groups)\n    for r in first:println(Result.unwrap_or(r,0))\n";
    rejected(&format!(
        "{helper}fn main():handle([[Ok(1)],[Err(\"unvisited group\")]])\n"
    ));
    checked(&format!("{helper}fn main():\n    let groups:List(List(Result(Int,String)))=[[Ok(1)],[Err(\"later handled\")]]\n    handle(groups)\n    for group in groups:\n        for r in group:println(Result.unwrap_or(r,0))\n")).unwrap();
}

#[test]
fn nested_dynamic_partitions_and_nested_result_layers_keep_distinct_duties() {
    let helper = "fn handle(xs:List(Result(Int,String)))->Int:\n    let [first,..tail]=xs else:return 0\n    println(Result.unwrap_or(first,0))\n    let [second,..last]=tail else:return 1\n    println(Result.unwrap_or(second,0))\n    for value in last:println(Result.unwrap_or(value,0))\n    2\n";
    checked(&format!(
        "{helper}fn main():handle([Err(\"first\"),Err(\"second\"),Err(\"last\")])\n"
    ))
    .unwrap();
    let bad = helper.replace(
        "println(Result.unwrap_or(second,0))",
        "println(List.len([second]))",
    );
    rejected(&format!(
        "{bad}fn main():handle([Ok(1),Err(\"second\"),Ok(3)])\n"
    ));
    let helper = "fn handle(xs:List(Result(Result(Int,String),String)))->Int:\n    match xs:\n        []->0\n        [head,..tail]->\n            println(Result.is_ok(head))\n            for r in tail:\n                match r:\n                    Ok(inner)->println(Result.unwrap_or(inner,0))\n                    Err(_)->()\n            1\n";
    rejected(&format!(
        "{helper}fn main():handle([Ok(Err(\"nested prefix\")),Err(\"tail\")])\n"
    ));
}

#[test]
fn recursive_return_contracts_preserve_exact_aliases_without_handling_credit() {
    checked(include_str!("tail_calls/results.fn")).unwrap();
    let forward = "fn first(n:Int,r:Result(Int,String))->Result(Int,String):if n==0:r else:second(n-1,r)\nfn second(n:Int,r:Result(Int,String))->Result(Int,String):first(n,r)\n";
    checked(&format!(
        "{forward}fn main():Result.unwrap_or(first(2,Err(\"handled\")),0)\n"
    ))
    .unwrap();
    rejected(&format!(
        "{forward}fn main():println(List.len([first(2,Err(\"lost\"))]))\n"
    ));
    rejected(
        "fn replace(n:Int,r:Result(Int,String))->Result(Int,String):\n    println(List.len([r]))\n    if n==0:Ok(1) else:replace(n-1,r)\nfn main():Result.unwrap_or(replace(2,Err(\"lost original\")),0)\n",
    );
    rejected(
        "fn hidden(n:Int)->Result(Int,String):\n    let lost:Result(Int,String)=Err(\"independent\")\n    println(List.len([lost]))\n    if n==0:Ok(1) else:hidden(n-1)\nfn main():Result.unwrap_or(hidden(2),0)\n",
    );
    checked("fn produced(n:Int)->Result(Int,String):if n==0:Err(\"fresh returned\") else:produced(n-1)\nfn main():Result.unwrap_or(produced(2),0)\n").unwrap();
}

#[test]
fn loop_function_exits_preserve_outer_duties_and_function_owned_defers() {
    checked(include_str!("iteration/captures.fn")).unwrap();
    checked(include_str!("iteration/with_loop.fn")).unwrap();
    checked(include_str!("json_values/valid/members.fn")).unwrap();
    rejected(
        "fn bad()->Int:\n    let r:Result(Int,String)=Err(\"outer skipped\")\n    for n in [1,2]:return n\n    Result.unwrap_or(r,0)\n",
    );
    checked("fn good()->Int:\n    let r:Result(Int,String)=Err(\"deferred outer\")\n    defer println(Result.unwrap_or(r,0))\n    for n in [1,2]:return n\n    0\n").unwrap();
    rejected(
        "fn bad()->Result(Int,String):\n    let xs:List(Result(Int,String))=[Err(\"first\"),Err(\"unvisited\")]\n    for r in xs:return r\n    Ok(0)\n",
    );
    checked("fn good()->List(Result(Int,String)):\n    let xs:List(Result(Int,String))=[Err(\"first\"),Err(\"returned all\")]\n    for r in xs:\n        println(List.len([r]))\n        return xs\n    xs\n").unwrap();
}

#[test]
fn wrapping_a_returned_iteration_item_does_not_transfer_the_unvisited_family() {
    rejected(
        "fn bad()->Result(Result(Int,String),String):\n    let xs:List(Result(Int,String))=[Err(\"first\"),Err(\"unvisited\")]\n    for r in xs:return Ok(r)\n    Ok(Ok(0))\n",
    );
    rejected(
        "fn bad()->Result(Int,String):\n    let xs:List(Result(Int,String))=[Err(\"first\"),Err(\"unvisited\")]\n    for r in xs:println(r?)\n    Ok(0)\n",
    );
    checked("fn good()->Int:\n    let r:Result(Int,String)=Err(\"captured for exit\")\n    for n in [1,2]:\n        defer println(Result.unwrap_or(r,0))\n        return n\n    Result.unwrap_or(r,0)\n").unwrap();
    checked("fn good()->Unit:\n    let xs:List(Result(Int,String))=[Err(\"first\"),Err(\"second\")]\n    for r in xs:defer println(Result.unwrap_or(r,0))\n").unwrap();
}

#[test]
fn structurally_decreasing_list_handlers_require_every_prefix_and_payload_duty() {
    checked(include_str!("sequences/results.fn")).unwrap();
    rejected(
        "fn skip(xs:List(Result(Int,String)))->Int:\n    match xs:\n        []->0\n        [head,..tail]->\n            println(List.len([head]))\n            skip(tail)\nfn main():skip([Err(\"first\"),Ok(2)])\n",
    );
    rejected(
        "fn cycle(xs:List(Result(Int,String)))->Int:\n    match xs:\n        []->0\n        [head,..tail]->\n            println(Result.unwrap_or(head,0))\n            println(List.len(tail))\n            cycle(xs)\nfn main():cycle([Ok(1),Err(\"never reaches it\")])\n",
    );
    rejected(
        "fn outer(xs:List(Result(Result(Int,String),String)))->Int:\n    match xs:\n        []->0\n        [head,..tail]->\n            println(Result.is_ok(head))\n            outer(tail)\nfn main():outer([Ok(Err(\"nested prefix\"))])\n",
    );
}

#[test]
fn recursive_nominal_shapes_expand_only_the_source_observed_fields() {
    checked(include_str!("types/recursive_tree.fn")).unwrap();
    let prefix = "type Node:\n    value:Int\n    next:Option(Node)\nfn inspect(r:Result(Node,String))->Int:\n    match r:\n        Ok(node)->node.value\n        Err(_)->0\n";
    checked(&format!("{prefix}fn main():inspect(Ok(Node(1,None)))\n")).unwrap();
    rejected(&format!(
        "{prefix}fn main():\n    let r:Result(Node,String)=Ok(Node(1,None))\n    println(List.len([r]))\n"
    ));
}

#[test]
fn recursive_callable_collections_retain_proven_code_and_input_targets() {
    checked(include_str!("closures/stress.fn")).unwrap();
    checked(include_str!("maps/stress.fn")).unwrap();
    let helper = "fn grow(n:Int,state:Map(Int,()->Int))->Map(Int,()->Int):\n    if n==0:state else:grow(n-1,Map.put(state,n,()->n))\nfn factory(handler:(Result(Int,String))->Int)->()->Int:()->handler(Err(\"created inside callback\"))\n";
    rejected(&format!(
        "{helper}fn main():\n    let original:Map(Int,()->Int)=%{{0:factory((r)->List.len([r]))}}\n    let result=grow(2,original)\n    Option.unwrap_or(Map.get(result,0),()->0)()\n"
    ));
}
