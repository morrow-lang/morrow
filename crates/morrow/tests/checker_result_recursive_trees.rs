use morrow_compiler::{check, parse};
const TREE: &str = "type Tree:\n    Empty\n    Leaf(Result(Int,String))\n    Branch(Tree,Tree)\n";
const WALK: &str = "fn walk(tree:Tree)->Unit:\n    match tree:\n        Empty->()\n        Leaf(value)->println(Result.is_err(value))\n        Branch(left,right)->\n            walk(left)\n            walk(right)\n";
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
fn complete_sum_handlers_visit_each_recursive_sibling() {
    accepts(&format!(
        "{TREE}{WALK}fn main():\n    let a:Result(Int,String)=Err(\"first\")\n    let b:Result(Int,String)=Err(\"second\")\n    walk(Branch(Leaf(a),Branch(Empty,Leaf(b))))\n"
    ));
    accepts(&format!("{TREE}{WALK}fn main():walk(Empty)\n"));
}
#[test]
fn metadata_borrowing_and_whole_tree_aliases_keep_the_original_duties() {
    let helpers = "fn count(tree:Tree)->Int:List.len([tree])\nfn identity(tree:Tree)->Tree:tree\n";
    accepts(&format!(
        "{TREE}{WALK}{helpers}fn main():\n    let value:Result(Int,String)=Err(\"shared\")\n    let tree=Branch(Leaf(value),Leaf(value))\n    println(count(tree))\n    walk(identity(tree))\n"
    ));
    rejects(&format!(
        "{TREE}{helpers}fn main():println(count(Leaf(Err(\"lost\"))))\n"
    ));
}
#[test]
fn recursive_credit_never_substitutes_for_unhandled_siblings() {
    let skipped = WALK.replace(
        "            walk(right)",
        "            println(List.len([right]))",
    );
    rejects(&format!(
        "{TREE}{skipped}fn main():walk(Branch(Leaf(Ok(1)),Leaf(Err(\"lost\"))))\n"
    ));
}
#[test]
fn self_calls_and_reconstructed_roots_are_not_strict_descendants() {
    rejects(&format!(
        "{TREE}fn walk(tree:Tree)->Unit:walk(tree)\nfn main():walk(Leaf(Err(\"lost\")))\n"
    ));
    let rebuilt = WALK.replace(
        "            walk(left)\n            walk(right)",
        "            walk(Branch(left,right))",
    );
    rejects(&format!(
        "{TREE}{rebuilt}fn main():walk(Branch(Empty,Leaf(Err(\"lost\"))))\n"
    ));
}
#[test]
fn nested_result_layers_and_handler_local_results_remain_accountable() {
    let nested = TREE.replace("Result(Int,String)", "Result(Result(Int,String),String)");
    rejects(&format!(
        "{nested}{WALK}fn main():walk(Leaf(Ok(Err(\"inner\"))))\n"
    ));
    let full=WALK.replace("Leaf(value)->println(Result.is_err(value))","Leaf(value)->\n            match value:\n                Ok(inner)->println(Result.is_err(inner))\n                Err(_)->()");
    accepts(&format!(
        "{nested}{full}fn main():walk(Branch(Empty,Leaf(Ok(Err(\"handled\")))))\n"
    ));
    let local=WALK.replace("        Empty->()","        Empty->\n            let lost:Result(Int,String)=Err(\"local\")\n            println(List.len([lost]))");
    rejects(&format!("{TREE}{local}fn main():walk(Empty)\n"));
}
#[test]
fn sibling_aliases_use_actual_identity_without_collapsing_distinct_cuts() {
    let touch = "fn touch(tree:Tree)->Unit:\n    match tree:\n        Empty->()\n        Leaf(value)->println(Result.is_err(value))\n        Branch(left,right)->\n            walk(left)\n            println(List.len([right]))\n";
    accepts(&format!(
        "{TREE}{WALK}{touch}fn main():\n    let r:Result(Int,String)=Err(\"shared\")\n    touch(Branch(Leaf(r),Leaf(r)))\n"
    ));
    rejects(&format!(
        "{TREE}{WALK}{touch}fn main():\n    touch(Branch(Leaf(Err(\"left\")),Leaf(Err(\"right\"))))\n"
    ));
}
#[test]
fn inactive_variants_neither_create_nor_acknowledge_tree_duties() {
    let make = "fn make(flag:Bool)->Tree:if flag:Leaf(Err(\"conditional\")) else:Empty\n";
    accepts(&format!(
        "{TREE}{make}fn main():println(List.len([make(flag:false)]))\n"
    ));
    rejects(&format!(
        "{TREE}{make}fn main():println(List.len([make(flag:true)]))\n"
    ));
    let guarded = WALK.replace(
        "walk(left)",
        "if false:walk(left) else:println(List.len([left]))",
    );
    rejects(&format!(
        "{TREE}{guarded}fn main():walk(Branch(Leaf(Err(\"lost\")),Empty))\n"
    ));
}
#[test]
fn a_returned_subtree_cannot_transfer_its_unreturned_siblings() {
    let select = "fn select(tree:Tree)->Tree:\n    match tree:\n        Empty->Empty\n        Leaf(value)->Leaf(value)\n        Branch(left,right)->\n            println(List.len([right]))\n            left\n";
    rejects(&format!(
        "{TREE}{WALK}{select}fn main():walk(select(Branch(Leaf(Err(\"left\")),Leaf(Err(\"right\")))))\n"
    ));
    accepts(&format!(
        "{TREE}{WALK}{select}fn main():\n    let tree=Branch(Leaf(Err(\"left\")),Leaf(Err(\"right\")))\n    walk(select(tree))\n    walk(tree)\n"
    ));
}
