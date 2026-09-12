use super::*;

fn analyze(source: &str, name: &str) -> Checked<Summary> {
    let ast = crate::parse::parse(source).unwrap();
    let program = super::super::pipeline_mode(&ast, |_, _, _| Ok(()), false)
        .unwrap()
        .0;
    let function = program.functions.iter().find(|f| f.name == name).unwrap();
    analyze_function(&program, function)
}
fn rejected(source: &str) {
    let error = analyze(source, "main").unwrap_err();
    assert!(error.message.contains("Result obligation"), "{error:?}");
}

#[test]
fn metadata_borrows_instead_of_handling_contained_results() {
    for value in [
        "let xs:List(Result(Int,String))=[Err(\"lost\")]\n    List.len(xs)",
        "let x:Option(Result(Int,String))=Some(Err(\"lost\"))\n    Option.is_some(x)",
        "let xs:Map(String,Result(Int,String))=%{\"key\":Err(\"lost\")}\n    Map.len(xs)",
    ] {
        rejected(&format!("fn main():\n    {value}\n"));
    }
}

#[test]
fn ordinary_aliases_and_each_tuple_projection_handle_original_origins() {
    analyze("fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    let alias=r\n    println(Result.unwrap_or(alias,0))\n", "main").unwrap();
    analyze("fn main():\n    let p:(Result(Int,String),Result(Int,String))=(Ok(1),Err(\"handled\"))\n    println(Result.unwrap_or(p.0,0))\n    println(Result.unwrap_or(p.1,0))\n", "main").unwrap();
    rejected(
        "fn main():\n    let p:(Result(Int,String),Result(Int,String))=(Ok(1),Err(\"lost\"))\n    println(Result.unwrap_or(p.0,0))\n",
    );
}

#[test]
fn result_predicates_acknowledge_only_the_outer_result_layer() {
    rejected(
        "fn main():\n    let r:Result(Result(Int,String),String)=Ok(Err(\"lost\"))\n    Result.is_ok(r)\n",
    );
    analyze("fn main():\n    let inner:Result(Int,String)=Err(\"handled\")\n    let outer:Result(Result(Int,String),String)=Ok(inner)\n    println(Result.is_ok(outer))\n    println(Result.unwrap_or(inner,0))\n", "main").unwrap();
}

#[test]
fn eager_unused_fallback_is_an_independent_result_origin() {
    rejected(
        "fn main():\n    let fallback:Result(Int,String)=Err(\"lost\")\n    let selected:Option(Result(Int,String))=Some(Ok(1))\n    Result.unwrap_or(Option.unwrap_or(selected,fallback),0)\n",
    );
}

#[test]
fn literal_elements_are_distinct_but_dynamic_projection_is_not_full_coverage() {
    rejected(
        "fn main():\n    let xs:List(Result(Int,String))=[Ok(1),Err(\"lost\")]\n    Result.unwrap_or(List.head(xs),0)\n",
    );
    analyze("fn main():\n    let xs:List(Result(Int,String))=[Ok(1),Err(\"handled\")]\n    println(List.len(xs))\n    println(Result.unwrap_or(List.get(xs,0),0))\n    println(Result.unwrap_or(List.get(xs,1),0))\n", "main").unwrap();
    let summary = analyze(
        "fn first(xs:List(Result(Int,String)))->Int:Result.unwrap_or(List.head(xs),0)\n",
        "first",
    )
    .unwrap();
    assert_eq!(summary.handled_inputs(), 0);
    assert!(summary.borrowed_inputs() > 0);
}

#[test]
fn returned_whole_regions_transfer_every_contained_duty() {
    analyze("fn make()->List(Result(Int,String)):\n    let xs:List(Result(Int,String))=[Ok(1),Err(\"transferred\")]\n    println(List.len(xs))\n    xs\n", "make").unwrap();
    let error=analyze("fn first()->Result(Int,String):\n    let xs:List(Result(Int,String))=[Ok(1),Err(\"lost\")]\n    List.head(xs)\n", "first").unwrap_err();
    assert!(error.message.contains("Result obligation"));
}

#[test]
fn map_overwrite_does_not_transfer_the_replaced_entry() {
    let error=analyze("fn update()->Map(String,Result(Int,String)):\n    let xs:Map(String,Result(Int,String))=%{\"key\":Err(\"lost\")}\n    Map.put(xs,\"key\",Ok(1))\n", "update").unwrap_err();
    assert!(error.message.contains("Result obligation"));
    analyze("fn retain()->Map(String,Result(Int,String)):\n    let xs:Map(String,Result(Int,String))=%{\"key\":Err(\"retained\")}\n    println(Map.len(xs))\n    xs\n", "retain").unwrap();
}

#[test]
fn borrowing_and_handling_are_explicit_distinct_input_summaries() {
    let borrowed = analyze(
        "fn count(xs:List(Result(Int,String)))->Int:List.len(xs)\n",
        "count",
    )
    .unwrap();
    assert!(borrowed.borrowed_inputs() > 0);
    assert_eq!(borrowed.handled_inputs(), 0);
    let handled = analyze(
        "fn observe(r:Result(Int,String))->Bool:Result.is_ok(r)\n",
        "observe",
    )
    .unwrap();
    assert!(handled.handled_inputs() > 0);
    assert_eq!(handled.borrowed_inputs(), 0);
}

#[test]
fn nominal_wrappers_preserve_origins_without_adding_an_error_layer() {
    analyze("newtype Box=Box(Result(Int,String))\nfn main():\n    let value=Box(Err(\"handled\"))\n    Result.unwrap_or(value.0,0)\n", "main").unwrap();
    rejected(
        "type Pair:\n    left:Result(Int,String)\n    right:Result(Int,String)\nfn main():\n    let pair=Pair(Ok(1),Err(\"lost\"))\n    Result.unwrap_or(pair.left,0)\n",
    );
}

#[test]
fn unsupported_control_flow_and_callable_proofs_fail_conservatively() {
    analyze("fn main():for n in [1]:println(n)\n", "main").unwrap();
    analyze("fn main():for n in [1]:break\n", "main").unwrap();
    let error = analyze(
        "fn main():\n    let f:(List(Result(Int,String)))->Int=List.len\n    f([Err(\"lost\")])\n",
        "main",
    )
    .unwrap_err();
    assert!(error.message.contains("remains unhandled"), "{error:?}");
}

#[test]
fn uncertain_map_key_equality_never_transfers_a_possibly_overwritten_origin() {
    let source = "fn replace(key:String)->Map(String,Result(Int,String)):%{key:Err(\"lost\"),\"same\":Ok(1)}\n";
    let error = analyze(source, "replace").unwrap_err();
    assert!(error.message.contains("does not yet prove"), "{error:?}");
}

#[test]
fn shared_alias_graphs_do_not_expand_exponentially() {
    use std::fmt::Write;
    let mut source="fn nested():\n    let seed:Result(Int,String)=Err(\"transferred\")\n    let level0=[seed]\n".to_owned();
    for i in 1..30 {
        writeln!(
            &mut source,
            "    let level{i}=[level{},level{}]",
            i - 1,
            i - 1
        )
        .unwrap();
    }
    source.push_str("    level29\n");
    analyze(&source, "nested").unwrap();
}

#[test]
fn work_limit_is_charged_before_origin_or_region_allocation() {
    let program = ir::Program::default();
    let mut engine = Engine::new(&program);
    engine.work = WORK_LIMIT;
    assert!(engine.origin(None, Span::default()).is_err());
    assert!(engine.origins.is_empty());
    assert!(engine.node(Region::Empty, Span::default()).is_err());
    assert_eq!(engine.nodes, 0);
}

#[test]
fn unused_result_input_rule_remains_independent_of_borrowing() {
    let ty = Type::Result(Box::new(Type::Int), Box::new(Type::String));
    let function = ir::Function {
        mailbox: None,
        id: ir::FunctionId(0),
        name: "ignored".into(),
        params: vec![ir::Param {
            id: ir::LocalId(0),
            ty,
        }],
        captures: vec![],
        return_type: Type::Int,
        body: ir::Expr {
            kind: ir::ExprKind::Int(0),
            ty: Type::Int,
            span: Span::default(),
        },
        local_count: 1,
    };
    let error = analyze_function(&ir::Program::default(), &function).unwrap_err();
    assert!(error.message.contains("never used"), "{error:?}");
}

#[test]
fn source_summary_retains_return_provenance_separately_from_handling() {
    let result = analyze(
        "fn identity(xs:List(Result(Int,String)))->List(Result(Int,String)):xs\n",
        "identity",
    )
    .unwrap();
    assert_eq!(result.handled_inputs(), 0);
    assert_eq!(result.borrowed_inputs(), 0);
    assert!(
        result
            .origins
            .iter()
            .any(|o| o.input == Some(0) && o.returned != Predicate::FALSE)
    );
    assert!(matches!(
        result.output.node.kind,
        Region::List { exact: false, .. }
    ));
}
