//! Recursive construction must preserve error accountability at every finite return.
use morrow_compiler::{check, parse};
const TREE: &str = "type Tree:\n    Empty\n    Leaf(Result(Int,String))\n    Branch(Tree,Tree)\n";
const WALK: &str = "fn walk(tree:Tree)->Unit:\n    match tree:\n        Empty->()\n        Leaf(value)->println(Result.is_err(value))\n        Branch(left,right)->\n            walk(left)\n            walk(right)\n";
const BUILD: &str = "fn build(depth:Int)->Tree:\n    if depth<=0:Leaf(Err(\"leaf\"))\n    else:Branch(build(depth-1),build(depth-1))\n";
const APPEND: &str = "fn append(depth:Int,values:List(Result(Int,String)))->List(Result(Int,String)):\n    if depth<=0:values\n    else:append(depth-1,List.push(values,Err(\"new\")))\n";
fn checked(source: &str) -> Result<(), String> {
    check::check_library(&parse::parse(source).unwrap())
        .map(|_| ())
        .map_err(|error| error.message)
}
fn accepts(source: &str) {
    checked(source).unwrap_or_else(|error| panic!("{source}\n{error}"));
}
fn rejects(source: &str) {
    let error = checked(source).expect_err(source);
    assert!(error.contains("Result obligation"), "{error}");
}
#[test]
fn recursive_fresh_nominal_builders_transfer_all_produced_duties() {
    accepts(&format!("{TREE}{WALK}{BUILD}fn main():walk(build(4))\n"));
}
#[test]
fn recursive_builder_outputs_are_not_silently_handled() {
    rejects(&format!(
        "{TREE}{BUILD}fn main():println(List.len([build(4)]))\n"
    ));
}
#[test]
fn recursive_builder_local_duties_must_be_returned_or_handled() {
    let lost=BUILD.replace("    if depth", "    let local:Result(Int,String)=Err(\"lost\")\n    println(List.len([local]))\n    if depth");
    rejects(&format!("{TREE}{WALK}{lost}fn main():walk(build(4))\n"));
}
#[test]
fn mutual_fresh_builders_keep_independent_nested_outputs() {
    let first = "fn first(depth:Int)->Tree:if depth<=0:Leaf(Ok(1)) else:Branch(second(depth-1),second(depth-1))\n";
    let second = "fn second(depth:Int)->Tree:if depth<=0:Leaf(Err(\"leaf\")) else:first(depth-1)\n";
    for definitions in [format!("{first}{second}"), format!("{second}{first}")] {
        accepts(&format!(
            "{TREE}{WALK}{definitions}fn main():walk(first(4))\n"
        ));
    }
}
#[test]
fn recursive_list_accumulators_retain_original_and_new_result_families() {
    accepts(&format!(
        "{APPEND}fn main():\n    let original:List(Result(Int,String))=[Err(\"original\")]\n    for result in append(3,original):println(Result.is_err(result))\n"
    ));
}
#[test]
fn recursive_accumulator_summaries_never_hide_dropped_inputs() {
    let dropped = APPEND.replace("if depth<=0:values", "if depth<=0:[]");
    rejects(&format!(
        "{dropped}fn main():\n    for result in append(3,[Err(\"lost\")]):println(Result.is_err(result))\n"
    ));
    let subset = APPEND.replace("List.push(values,Err(\"new\"))", "List.tail(values)");
    rejects(&format!(
        "{subset}fn main():\n    for result in append(3,[Err(\"lost\"),Err(\"kept\")]):println(Result.is_err(result))\n"
    ));
}
#[test]
fn observing_only_an_accumulator_output_prefix_does_not_handle_the_original_family() {
    rejects(&format!(
        "{APPEND}fn main():\n    let original:List(Result(Int,String))=[Err(\"first\"),Err(\"second\")]\n    println(Result.is_err(List.head(append(2,original))))\n"
    ));
}
#[test]
fn mutual_list_accumulator_equations_are_checked_in_either_source_order() {
    let first = APPEND.replace("else:append(", "else:other(");
    let second = "fn other(depth:Int,values:List(Result(Int,String)))->List(Result(Int,String)):append(depth,values)\n";
    for definitions in [format!("{first}{second}"), format!("{second}{first}")] {
        accepts(&format!(
            "{definitions}fn main():\n    for result in append(3,[Err(\"original\")]):println(Result.is_err(result))\n"
        ));
    }
}

const GROW: &str = "fn grow(depth:Int,tree:Tree)->Tree:\n    if depth<=0:tree\n    else:grow(depth-1,Branch(tree,Leaf(Err(\"new\"))))\n";
#[test]
fn recursive_nominal_accumulators_retain_all_original_and_new_subtrees() {
    accepts(&format!(
        "{TREE}{WALK}{GROW}fn main():walk(grow(3,Leaf(Err(\"original\"))))\n"
    ));
}
#[test]
fn nominal_builder_retention_does_not_hide_dropped_or_unhandled_results() {
    let dropped = GROW.replace("if depth<=0:tree", "if depth<=0:Empty");
    rejects(&format!(
        "{TREE}{WALK}{dropped}fn main():walk(grow(3,Leaf(Err(\"lost\"))))\n"
    ));
    rejects(&format!(
        "{TREE}{WALK}{GROW}fn main():\n    let tree=Leaf(Err(\"original\"))\n    let grown=grow(3,tree)\n    walk(tree)\n    println(List.len([grown]))\n"
    ));
}

#[test]
fn nominal_builders_retain_payload_inputs_without_equal_return_types() {
    let build = "fn replicate(depth:Int,value:Result(Int,String))->Tree:\n    if depth<=0:Leaf(value)\n    else:Branch(replicate(depth-1,value),Empty)\n";
    accepts(&format!(
        "{TREE}{WALK}{build}fn main():walk(replicate(3,Err(\"kept\")))\n"
    ));
    let discarded = build.replace("if depth<=0:Leaf(value)", "if depth<=0:Empty");
    rejects(&format!(
        "{TREE}{WALK}{discarded}fn main():walk(replicate(3,Err(\"lost\")))\n"
    ));
}
#[test]
fn nominal_builders_retain_multiple_distinct_payload_inputs() {
    let build = "fn replicate(depth:Int,a:Result(Int,String),b:Result(Int,String))->Tree:\n    if depth<=0:Branch(Leaf(a),Leaf(b))\n    else:replicate(depth-1,a:a,b:b)\n";
    accepts(&format!(
        "{TREE}{WALK}{build}fn main():walk(replicate(3,a:Err(\"a\"),b:Err(\"b\")))\n"
    ));
}
#[test]
fn generic_recursive_builders_are_checked_again_for_nested_result_payloads() {
    let prefix = "type Node(a):\n    Leaf(a)\n    Branch(Node(a),Node(a))\nfn replicate(depth:Int,value:a)->Node(a):\n    if depth<=0:Leaf(value)\n    else:Branch(replicate(depth-1,value),replicate(depth-1,value))\n";
    let walk = "fn walk(node:Node(Result(Int,String)))->Unit:\n    match node:\n        Leaf(value)->println(Result.is_err(value))\n        Branch(a,b)->\n            walk(a)\n            walk(b)\n";
    accepts(&format!(
        "{prefix}{walk}fn main():walk(replicate(3,Err(\"kept\")))\n"
    ));
    rejects(&format!(
        "{prefix}fn main():\n    let value:Result(Int,String)=Err(\"lost\")\n    println(List.len([replicate(3,value)]))\n"
    ));
}

#[test]
fn mutual_nominal_builders_reject_a_dropping_member_in_either_source_order() {
    let first = GROW.replace("else:grow(", "else:other(");
    let second = "fn other(depth:Int,tree:Tree)->Tree:grow(depth,tree)\n";
    for definitions in [format!("{first}{second}"), format!("{second}{first}")] {
        accepts(&format!(
            "{TREE}{WALK}{definitions}fn main():walk(grow(3,Leaf(Err(\"kept\"))))\n"
        ));
        let dropped = definitions.replace("if depth<=0:tree", "if depth<=0:Empty");
        rejects(&format!(
            "{TREE}{WALK}{dropped}fn main():walk(grow(3,Leaf(Err(\"lost\"))))\n"
        ));
    }
}
#[test]
fn builder_output_requires_handling_nested_tags_separately() {
    let nested = TREE.replace("Result(Int,String)", "Result(Result(Int,String),String)");
    let build = BUILD.replace("Leaf(Err(\"leaf\"))", "Leaf(Ok(Err(\"inner\")))");
    rejects(&format!("{nested}{WALK}{build}fn main():walk(build(2))\n"));
    let full=WALK.replace("Leaf(value)->println(Result.is_err(value))","Leaf(value)->\n            match value:\n                Ok(inner)->println(Result.is_err(inner))\n                Err(_)->()");
    accepts(&format!("{nested}{full}{build}fn main():walk(build(2))\n"));
}
#[test]
fn builder_cannot_replace_retention_with_handling_or_early_return() {
    let consumed = GROW.replace(
        "if depth<=0:tree",
        "if depth<=0:\n        walk(tree)\n        Empty",
    );
    rejects(&format!(
        "{TREE}{WALK}{consumed}fn main():walk(grow(2,Leaf(Err(\"old\"))))\n"
    ));
    let early = GROW.replace(
        "    if depth<=0",
        "    if depth==1:return Empty\n    if depth<=0",
    );
    rejects(&format!(
        "{TREE}{WALK}{early}fn main():walk(grow(2,Leaf(Err(\"lost\"))))\n"
    ));
    let dropped = "fn replicate(depth:Int,a:Result(Int,String),b:Result(Int,String))->Tree:\n    if depth<=0:Leaf(a)\n    else:replicate(depth-1,a:a,b:b)\n";
    rejects(&format!(
        "{TREE}{WALK}{dropped}fn main():walk(replicate(2,a:Err(\"kept\"),b:Err(\"lost\")))\n"
    ));
}

#[test]
fn executable_builder_fixtures_share_the_checked_native_ir() {
    for source in [
        include_str!("result_builders/fresh.fn"),
        include_str!("result_builders/accumulators.fn"),
        include_str!("result_builders/generic.fn"),
        include_str!("result_builders/payloads.fn"),
    ] {
        let program = check::check(&parse::parse(source).unwrap()).unwrap();
        morrow_compiler::lowering::emit(&program).unwrap();
    }
}
#[test]
fn failed_builder_duties_do_not_publish_repl_bindings() {
    let mut session = morrow_compiler::repl::Session::default();
    session.evaluate(TREE).unwrap();
    session.evaluate(BUILD).unwrap();
    session.evaluate(WALK).unwrap();
    assert!(
        session
            .evaluate("let lost=build(2)\nprintln(List.len([lost]))")
            .is_err()
    );
    assert!(session.evaluate("walk(lost)").is_err());
    assert_eq!(session.evaluate("walk(build(1))").unwrap(), "true\ntrue\n");
}

#[test]
fn recursive_list_builders_embed_payload_inputs_without_accumulators() {
    let build = "fn replicate(depth:Int,value:Result(Int,String))->List(Result(Int,String)):\n    if depth<=0:[value]\n    else:List.push(replicate(depth-1,value),Err(\"new\"))\n";
    accepts(&format!(
        "{build}fn main():\n    for result in replicate(2,Err(\"kept\")):println(Result.is_err(result))\n"
    ));
    for replacement in ["[]", "[Err(\"different\")]"] {
        let dropped = build.replace("if depth<=0:[value]", &format!("if depth<=0:{replacement}"));
        rejects(&format!(
            "{dropped}fn main():\n    for result in replicate(2,Err(\"lost\")):println(Result.is_err(result))\n"
        ));
    }
}
#[test]
fn recursive_list_builders_keep_distinct_payload_and_accumulator_inputs() {
    let build = "fn build(depth:Int,a:Result(Int,String),b:Result(Int,String),values:List(Result(Int,String)))->List(Result(Int,String)):\n    if depth<=0:List.push(List.push(values,a),b)\n    else:build(depth-1,a:a,b:b,values:values)\n";
    accepts(&format!(
        "{build}fn main():\n    for result in build(2,a:Err(\"a\"),b:Err(\"b\"),values:[Err(\"old\")]):println(Result.is_err(result))\n"
    ));
    let dropped = build.replace("List.push(List.push(values,a),b)", "List.push(values,a)");
    rejects(&format!(
        "{dropped}fn main():\n    for result in build(2,a:Err(\"a\"),b:Err(\"lost\"),values:[Err(\"old\")]):println(Result.is_err(result))\n"
    ));
}
#[test]
fn optional_builder_payloads_do_not_assert_nonempty_without_present_duties() {
    let build = "fn build(depth:Int,value:Option(Result(Int,String)))->List(Option(Result(Int,String))):\n    if depth>0:build(depth-1,value)\n    else:\n        match value:\n            Some(result)->[Some(result)]\n            None->[]\n";
    let walk = "fn main():\n    for option in build(2,Some(Err(\"kept\"))):\n        match option:\n            Some(result)->println(Result.is_err(result))\n            None->()\n";
    accepts(&format!("{build}{walk}"));
    accepts(&format!(
        "{build}{}",
        walk.replace("Some(Err(\"kept\"))", "None")
    ));
    rejects(&format!(
        "{build}fn main():\n    let value:Option(Result(Int,String))=None\n    let outputs=build(2,value)\n    for option in outputs:\n        match option:\n            Some(result)->println(Result.is_err(result))\n            None->()\n    let lost:Result(Int,String)=Err(\"lost on empty\")\n    if List.is_empty(outputs):println(List.len([lost]))\n    else:println(Result.is_err(lost))\n"
    ));
}

#[test]
fn map_accumulator_overwrites_require_a_separate_retention_proof() {
    let build = "fn build(depth:Int,values:Map(Int,Result(Int,String)))->Map(Int,Result(Int,String)):\n    if depth<=0:values\n    else:build(depth-1,Map.put(values,depth,Err(\"new\")))\n";
    rejects(&format!(
        "{build}fn main():\n    for result in Map.values(build(2,%{{2:Err(\"overwritten\")}})):println(Result.is_err(result))\n"
    ));
    let handled=build.replace("else:build(depth-1,Map.put(values,depth,Err(\"new\")))", "else:\n        for result in Map.values(values):println(Result.is_err(result))\n        build(depth-1,Map.put(values,depth,Err(\"new\")))");
    rejects(&format!(
        "{handled}fn main():\n    for result in Map.values(build(2,%{{2:Err(\"handled\")}})):println(Result.is_err(result))\n"
    ));
}

#[test]
fn empty_recursive_subtrees_do_not_prove_builder_output_nonempty() {
    let source = r#"type Tree:
    Empty
    Leaf(Result(Int,String))
    Branch(Tree,Tree)
fn flatten(t:Tree)->List(Tree):
    match t:
        Empty->[]
        Leaf(r)->[Leaf(r)]
        Branch(a,b)->List.concat(flatten(a),flatten(b))
fn walk(t:Tree)->Unit:
    match t:
        Empty->()
        Leaf(r)->println(Result.is_err(r))
        Branch(a,b)->
            walk(a)
            walk(b)
fn main():
    let values=flatten(Branch(Empty,Empty))
    for t in values:walk(t)
    let lost:Result(Int,String)=Err("never handled")
    if List.is_empty(values):println(List.len([lost]))
    else:println(Result.is_err(lost))
"#;
    rejects(source);
    let prefix = source.split_once("    let lost:").unwrap().0;
    accepts(prefix);
}

#[test]
fn duty_free_list_elements_do_not_prove_retained_accumulator_nonempty() {
    let build = "fn compact(depth:Int,values:List(Option(Result(Int,String))))->List(Option(Result(Int,String))):\n    if depth>0:compact(depth-1,values)\n    else:List.filter(values,fn(value)->Option.is_some(value))\n";
    rejects(&format!(
        "{build}fn main():\n    let values:List(Option(Result(Int,String)))=[None]\n    let compacted=compact(2,values)\n    for option in compacted:\n        match option:\n            Some(result)->println(Result.is_err(result))\n            None->()\n    let lost:Result(Int,String)=Err(\"lost on empty\")\n    if List.is_empty(compacted):println(List.len([lost]))\n    else:println(Result.is_err(lost))\n"
    ));
}
