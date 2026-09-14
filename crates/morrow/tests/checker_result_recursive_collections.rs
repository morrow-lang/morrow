use morrow_compiler::{check, parse};
const NODE: &str = "type Node:\n    value:Result(Int,String)\n    children:List(Node)\n";
const WALK: &str = "fn walk(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    for child in node.children:\n        walk(child)\n";
fn checked(source: &str) -> Result<(), String> {
    check::check_library(&parse::parse(source).unwrap())
        .map(|_| ())
        .map_err(|e| e.message)
}
fn accepts(source: &str) {
    checked(source).unwrap_or_else(|e| panic!("{source}\n{e}"));
}
fn rejects(source: &str) {
    let error = checked(source).expect_err(source);
    assert!(error.contains("Result obligation"), "{error}");
}
#[test]
fn full_child_traversal_handles_every_node_and_result_field() {
    accepts(&format!(
        "{NODE}{WALK}fn main():walk(Node(Ok(0),[Node(Err(\"first\"),[]),Node(Err(\"second\"),[])]))\n"
    ));
    accepts(&format!(
        "{NODE}{WALK}fn main():walk(Node(Err(\"root\"),[]))\n"
    ));
}
#[test]
fn early_exits_cannot_prove_a_complete_child_family() {
    for exit in ["break", "return ()"] {
        let body = WALK.replace(
            "        walk(child)",
            &format!("        walk(child)\n        {exit}"),
        );
        rejects(&format!(
            "{NODE}{body}fn main():walk(Node(Ok(0),[Node(Err(\"first\"),[]),Node(Err(\"lost\"),[])]))\n"
        ));
    }
}
#[test]
fn iterable_type_alone_never_certifies_the_original_root_as_a_child() {
    let body = WALK.replace("for child in node.children:", "for child in [node]:");
    rejects(&format!(
        "{NODE}{body}fn main():walk(Node(Err(\"lost\"),[]))\n"
    ));
}
#[test]
fn map_child_traversal_preserves_key_and_value_roles() {
    let node = NODE.replace("List(Node)", "Map(String,Node)");
    let walk = WALK.replace(
        "for child in node.children:",
        "for (key,child) in node.children:\n        println(key)",
    );
    accepts(&format!(
        "{node}{walk}fn main():walk(Node(Ok(0),%{{\"one\":Node(Err(\"handled\"),%{{}})}}))\n"
    ));
    let values = WALK.replace("in node.children:", "in Map.values(node.children):");
    accepts(&format!(
        "{node}{values}fn main():walk(Node(Ok(0),%{{\"one\":Node(Err(\"handled\"),%{{}})}}))\n"
    ));
}
#[test]
fn later_full_traversal_preserves_useful_prior_partial_handling() {
    accepts(&format!(
        "{NODE}{WALK}fn main():\n    let nodes=[Node(Err(\"first\"),[]),Node(Err(\"second\"),[])]\n    walk(List.head(nodes))\n    for node in nodes:walk(node)\n"
    ));
}
#[test]
fn exact_recursive_callback_traversal_uses_actual_descendants() {
    let map = WALK.replace(
        "    for child in node.children:\n        walk(child)",
        "    let callback=walk\n    List.map(node.children,callback)\n    ()",
    );
    accepts(&format!(
        "{NODE}{map}fn main():walk(Node(Ok(0),[Node(Err(\"child\"),[])]))\n"
    ));
    let wrong = map.replace(
        "List.map(node.children,callback)",
        "List.map([node],callback)",
    );
    rejects(&format!(
        "{NODE}{wrong}fn main():walk(Node(Err(\"lost\"),[]))\n"
    ));
}
#[test]
fn map_cardinality_is_preserved_through_borrowing_and_full_traversal() {
    let prefix = "fn handle(items:Map(String,Result(Int,String)))->Unit:\n    println(Map.is_empty(items))\n    for value in Map.values(items):println(Result.is_err(value))\n";
    accepts(&format!("{prefix}fn main():handle(%{{}})\n"));
    accepts(&format!(
        "{prefix}fn main():handle(%{{\"a\":Err(\"one\"),\"b\":Err(\"two\")}})\n"
    ));
    let skip = prefix.replace(
        "    for value in Map.values(items):println(Result.is_err(value))",
        "    ()",
    );
    rejects(&format!(
        "{skip}fn main():handle(%{{\"a\":Err(\"lost\")}})\n"
    ));
}
#[test]
fn guaranteed_child_cleanup_and_unrelated_captures_keep_separate_roles() {
    let cleanup = WALK.replace(
        "        walk(child)",
        "        defer walk(child)\n        ()",
    );
    accepts(&format!(
        "{NODE}{cleanup}fn main():walk(Node(Ok(0),[Node(Err(\"child\"),[])]))\n"
    ));
    let unrelated = WALK.replace(
        "        walk(child)",
        "        walk(node)\n        walk(child)",
    );
    rejects(&format!(
        "{NODE}{unrelated}fn main():walk(Node(Ok(0),[Node(Err(\"lost\"),[])]))\n"
    ));
}
#[test]
fn deleting_the_last_dynamic_key_cannot_reuse_the_old_nonempty_predicate() {
    let helper = "fn remove(items:Map(String,Int),r:Result(Int,String))->Unit:\n    let rest=Map.delete(items,\"x\")\n    if Map.is_empty(rest):() else:println(Result.is_err(r))\n";
    rejects(&format!(
        "{helper}fn main():remove(%{{\"x\":1}},Err(\"lost\"))\n"
    ));
    let full = helper.replace(
        "if Map.is_empty(rest):() else:println(Result.is_err(r))",
        "println(Map.is_empty(rest))\n    println(Result.is_err(r))",
    );
    accepts(&format!(
        "{full}fn main():remove(%{{\"x\":1}},Err(\"handled\"))\n"
    ));
}
#[test]
fn option_edges_and_newtype_wrappers_preserve_actual_stored_descendants() {
    let node = NODE.replace("List(Node)", "Option(Node)");
    let walk = "fn walk(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    match node.children:\n        Some(child)->walk(child)\n        None->()\n";
    accepts(&format!(
        "{node}{walk}fn main():walk(Node(Ok(0),Some(Node(Err(\"leaf\"),None))))\n"
    ));
    accepts(
        "newtype Node = Wrap((Result(Int,String),List(Node)))\nfn walk(node:Node)->Unit:\n    let Wrap((value,children))=node\n    println(Result.is_err(value))\n    for child in children:walk(child)\nfn main():walk(Wrap((Ok(0),[Wrap((Err(\"leaf\"),[]))])))\n",
    );
}
#[test]
fn separate_child_fields_cannot_collapse_and_shared_aliases_remain_valid() {
    let node = NODE.replace(
        "    children:List(Node)",
        "    left:List(Node)\n    right:List(Node)",
    );
    let walk = "fn walk(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    for child in node.left:walk(child)\n    for child in node.right:walk(child)\n";
    accepts(&format!(
        "{node}{walk}fn main():\n    let value:Result(Int,String)=Err(\"shared\")\n    let leaf=Node(value,[],[])\n    walk(Node(Ok(0),[leaf],[leaf]))\n"
    ));
    let skip = walk.replace(
        "    for child in node.right:walk(child)",
        "    println(List.len(node.right))",
    );
    rejects(&format!(
        "{node}{skip}fn main():walk(Node(Ok(0),[Node(Ok(1),[],[])],[Node(Err(\"lost\"),[],[])]))\n"
    ));
}
#[test]
fn branch_selected_descendant_family_must_still_cover_every_original_child() {
    let walk = WALK
        .replace("node:Node)", "node:Node,flag:Bool)")
        .replace("in node.children:", "in (if flag:node.children else:[]):")
        .replace("walk(child)", "walk(child,flag:flag)");
    rejects(&format!(
        "{NODE}{walk}fn main():walk(Node(Ok(0),[Node(Err(\"lost\"),[])]),flag:false)\n"
    ));
    let full = walk.replace(
        "if flag:node.children else:[]",
        "if flag:node.children else:List.reverse(node.children)",
    );
    accepts(&format!(
        "{NODE}{full}fn main():walk(Node(Ok(0),[Node(Err(\"handled\"),[])]),flag:false)\n"
    ));
}
#[test]
fn taking_one_child_or_filtering_a_subset_never_proves_complete_traversal() {
    let one = WALK.replace(
        "    for child in node.children:\n        walk(child)",
        "    if List.is_empty(node.children):() else:walk(List.head(node.children))",
    );
    rejects(&format!(
        "{NODE}{one}fn main():walk(Node(Ok(0),[Node(Err(\"first\"),[]),Node(Err(\"lost\"),[])]))\n"
    ));
    let subset = WALK.replace("in node.children:", "in List.tail(node.children):");
    rejects(&format!(
        "{NODE}{subset}fn main():walk(Node(Ok(0),[Node(Err(\"lost\"),[]),Node(Err(\"last\"),[])]))\n"
    ));
}
