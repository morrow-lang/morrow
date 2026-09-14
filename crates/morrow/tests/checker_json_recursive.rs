//! Regular recursive JSON schema acceptance and rejection boundaries.
use morrow_compiler::{check, parse};

fn accepts(source: &str) {
    let ast = parse::parse(source).unwrap_or_else(|e| panic!("source syntax: {e:?}"));
    check::check(&ast).unwrap_or_else(|e| panic!("source checking: {e:?}"));
}
fn rejects(source: &str) {
    let ast = parse::parse(source).expect("negative is valid source syntax");
    assert!(check::check(&ast).is_err());
}

#[test]
fn regular_recursive_record_accepts_empty_children_base_case() {
    accepts(
        "type Node derive(Json):\n    value:Int\n    children:List(Node)\nfn main() -> Result(Unit,json.Error):\n    let value = json.decode(\"\\{\\\"value\\\":1,\\\"children\\\":[]\\}\",Node)?\n    println(json.encode(value)?)\n    Ok(())\n",
    );
}
#[test]
fn mutually_recursive_records_have_a_finite_optional_base() {
    accepts(
        "type A derive(Json):\n    next:Option(B)\ntype B derive(Json):\n    next:Option(A)\nfn main() -> Result(Unit,json.Error):\n    let value = json.decode(\"\\{\\\"next\\\":null\\}\",A)?\n    println(json.encode(value)?)\n    Ok(())\n",
    );
}
#[test]
fn concrete_generic_recursive_records_do_not_share_instantiations() {
    accepts(
        "type Tree(a) derive(Json):\n    value:a\n    children:List(Tree(a))\nfn main() -> Result(Unit,json.Error):\n    let integers:Tree(Int) = Tree(1,[])\n    let strings:Tree(String) = Tree(\"Morrow\",[])\n    println(json.encode(integers)?)\n    println(json.encode(strings)?)\n    Ok(())\n",
    );
}

#[test]
fn cached_cycles_do_not_skip_unsupported_fields_or_expanding_arguments() {
    for source in [
        "type Node derive(Json):\n    children:List(Node)\n    value:Result(Int,String)\nfn main(): ()\n",
        "type Node derive(Json):\n    children:List(Node)\n    value:()->Int\nfn main(): ()\n",
        "type Grow(a) derive(Json):\n    children:List(Grow(List(a)))\nfn main(): ()\n",
        "type Loop derive(Json):\n    next:Loop\nfn main(): ()\n",
    ] {
        rejects(source);
    }
}
#[test]
fn finite_generic_permutation_closes_with_distinct_type_keys() {
    accepts(
        "type Flip(a,b) derive(Json):\n    first:a\n    second:b\n    children:List(Flip(b,a))\nfn main() -> Result(Unit,json.Error):\n    let value:Flip(Int,String) = Flip(1,\"Morrow\",[])\n    println(json.encode(value)?)\n    Ok(())\n",
    );
}

#[test]
fn maps_supply_a_finite_recursive_base_and_unused_cycles_still_validate() {
    accepts("type Node derive(Json):\n    children:Map(String,Node)\nfn main(): ()\n");
    rejects("type A derive(Json):\n    next:B\ntype B derive(Json):\n    next:A\nfn main(): ()\n");
}
