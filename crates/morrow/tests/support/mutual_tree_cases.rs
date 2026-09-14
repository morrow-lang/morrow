// One source corpus is checked with and without the Result proof; rejection must be semantic.
pub const TREE: &str =
    "type Tree:\n    Empty\n    Leaf(Result(Int,String))\n    Branch(Tree,Tree)\n";
pub fn walk(name: &str, other: &str) -> String {
    format!(
        "fn {name}(tree:Tree)->Unit:\n    match tree:\n        Empty->()\n        Leaf(value)->println(Result.is_err(value))\n        Branch(left,right)->\n            {other}(left)\n            {other}(right)\n"
    )
}
pub fn tree_cases() -> Vec<(&'static str, bool, String)> {
    let a = walk("even", "odd");
    let b = walk("odd", "even");
    let main = "fn main():even(Branch(Leaf(Err(\"left\")),Leaf(Err(\"right\"))))\n";
    let mut cases = vec![
        ("alternating", true, format!("{TREE}{a}{b}{main}")),
        ("reversed", true, format!("{TREE}{b}{a}{main}")),
        (
            "shared siblings",
            true,
            format!(
                "{TREE}{a}{b}fn main():\n    let child=Leaf(Err(\"shared\"))\n    even(Branch(child,child))\n"
            ),
        ),
        (
            "bouncing root",
            false,
            format!(
                "{TREE}fn even(tree:Tree)->Unit:odd(tree)\nfn odd(tree:Tree)->Unit:even(tree)\n{main}"
            ),
        ),
    ];
    for (name,bad) in [
        ("skipped sibling",b.replace("even(right)","println(List.len([right]))")),
        ("rebuilt root",b.replace("even(left)\n            even(right)","even(Branch(left,right))")),
        ("inactive handling",b.replace("even(left)","if false:even(left) else:println(List.len([left]))")),
        ("local debt",b.replace("Empty->()","Empty->\n            let lost:Result(Int,String)=Err(\"local\")\n            println(List.len([lost]))")),
    ] { cases.push((name,false,format!("{TREE}{a}{bad}{main}"))); }
    let nested = TREE.replace("Result(Int,String)", "Result(Result(Int,String),String)");
    cases.push((
        "nested tag only",
        false,
        format!("{nested}{a}{b}fn main():even(Leaf(Ok(Err(\"nested\"))))\n"),
    ));
    let helper = "fn touch(tree:Tree)->Unit:\n    match tree:\n        Empty->()\n        Leaf(value)->println(Result.is_err(value))\n        Branch(left,right)->\n            even(left)\n            println(List.len([right]))\n";
    cases.push(("actual shared alias",true,format!("{TREE}{a}{b}{helper}fn main():\n    let child=Leaf(Err(\"shared\"))\n    touch(Branch(child,child))\n")));
    cases.push((
        "independent siblings",
        false,
        format!("{TREE}{a}{b}{helper}fn main():touch(Branch(Leaf(Err(\"a\")),Leaf(Err(\"b\"))))\n"),
    ));
    cases
}
pub fn collection_cases() -> Vec<(&'static str, bool, String)> {
    let node = "type Node:\n    value:Result(Int,String)\n    children:List(Node)\n";
    let even = "fn even(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    for child in node.children:\n        odd(child)\n";
    let odd = even
        .replace("fn even", "fn odd")
        .replace("odd(child)", "even(child)");
    let main = "fn main():even(Node(Ok(0),[Node(Err(\"left\"),[]),Node(Err(\"right\"),[])]))\n";
    let mut cases = vec![("full list", true, format!("{node}{even}{odd}{main}"))];
    for (name, bad) in [
        (
            "break",
            odd.replace("        even(child)", "        even(child)\n        break"),
        ),
        (
            "return",
            odd.replace(
                "        even(child)",
                "        even(child)\n        return ()",
            ),
        ),
        ("root iterable", odd.replace("node.children", "[node]")),
        (
            "single projected child",
            odd.replace(
                "for child in node.children:\n        even(child)",
                "if List.len(node.children)>0:even(List.head(node.children))",
            ),
        ),
    ] {
        cases.push((name, false, format!("{node}{even}{bad}{main}")));
    }
    let mapped = odd.replace(
        "    for child in node.children:\n        even(child)",
        "    let callback=even\n    List.map(node.children,callback)\n    ()",
    );
    cases.push((
        "exact callback",
        true,
        format!("{node}{even}{mapped}{main}"),
    ));
    let deferred = odd.replace(
        "        even(child)",
        "        defer even(child)\n        ()",
    );
    cases.push((
        "per child defer",
        true,
        format!("{node}{even}{deferred}{main}"),
    ));
    let node = node.replace("List(Node)", "Map(String,Node)");
    let even = even.replace("in node.children:", "in Map.values(node.children):");
    let odd = odd.replace("in node.children:", "in Map.values(node.children):");
    cases.push(("full map",true,format!("{node}{even}{odd}fn main():even(Node(Ok(0),%{{\"a\":Node(Err(\"left\"),%{{}}),\"b\":Node(Err(\"right\"),%{{}})}}))\n")));
    cases
}
pub fn nominal_cases() -> Vec<(&'static str, bool, String)> {
    let types = "type A:\n    EndA\n    LeafA(Result(Int,String))\n    ToB(B,B)\ntype B:\n    EndB\n    LeafB(Result(Int,String))\n    ToA(A,A)\n";
    let a = "fn walk_a(tree:A)->Unit:\n    match tree:\n        EndA->()\n        LeafA(r)->println(Result.is_err(r))\n        ToB(left,right)->\n            walk_b(left)\n            walk_b(right)\n";
    let b = a
        .replace("walk_a", "walk_b")
        .replace("tree:A", "tree:B")
        .replace("EndA", "EndB")
        .replace("LeafA", "LeafB")
        .replace("ToB", "ToA")
        .replace("walk_b(left)", "walk_a(left)")
        .replace("walk_b(right)", "walk_a(right)");
    let main = "fn main():walk_a(ToB(LeafB(Err(\"left\")),ToA(EndA,LeafA(Err(\"right\")))))\n";
    vec![
        (
            "distinct nominal types",
            true,
            format!("{types}{a}{b}{main}"),
        ),
        (
            "distinct nominal reversed",
            true,
            format!("{types}{b}{a}{main}"),
        ),
        (
            "distinct nominal lost sibling",
            false,
            format!(
                "{types}{a}{}{main}",
                b.replace("walk_a(right)", "println(List.len([right]))")
            ),
        ),
    ]
}
pub fn boundary_cases() -> Vec<(&'static str, bool, String)> {
    let a = walk("visit_a", "visit_b").replace("visit_b(right)", "visit_c(right)");
    let b = walk("visit_b", "visit_d");
    let c = walk("visit_c", "visit_b");
    let d = walk("visit_d", "visit_a");
    let main = "fn main():visit_a(Branch(Leaf(Err(\"left\")),Leaf(Err(\"right\"))))\n";
    let mut cases = vec![
        ("SCC cross edge", true, format!("{TREE}{a}{b}{c}{d}{main}")),
        (
            "SCC cross edge unsafe member",
            false,
            format!(
                "{TREE}{a}{b}{}{d}{main}",
                c.replace("visit_b(left)", "println(List.len([left]))")
            ),
        ),
    ];
    let first = walk("first", "second")
        .replace("first(tree:Tree)", "first(n:Int,tree:Tree)")
        .replace("second(left)", "second(left,0)")
        .replace("second(right)", "second(right,0)");
    let second = walk("second", "first")
        .replace("second(tree:Tree)", "second(tree:Tree,n:Int)")
        .replace("first(left)", "first(0,left)")
        .replace("first(right)", "first(0,right)");
    cases.push((
        "different parameter positions",
        true,
        format!("{TREE}{first}{second}fn main():first(0,Leaf(Err(\"leaf\")))\n"),
    ));
    let a = walk("even", "odd");
    let b = walk("odd", "even");
    let dead = b.replace(
        "    match tree:",
        "    if false:even(tree)\n    match tree:",
    );
    cases.push((
        "inactive nondecreasing edge",
        true,
        format!("{TREE}{a}{dead}fn main():even(Empty)\n"),
    ));
    let aliases = "newtype Wrapped = Wrapped(Inner)\ntype Inner:\n    End\n    Leaf(Result(Int,String))\n    Child(Wrapped)\n";
    let left = "fn left(value:Wrapped)->Unit:right(value.0)\n";
    let right = "fn right(value:Inner)->Unit:left(Wrapped(value))\n";
    cases.push((
        "unboxed root bounce",
        false,
        format!("{aliases}{left}{right}fn main():left(Wrapped(Leaf(Err(\"lost\"))))\n"),
    ));
    cases
}
pub fn guarded_cases() -> Vec<(&'static str, bool, String)> {
    let even = walk("even", "odd").replace("tree:Tree", "tree:Tree,flag:Bool").replace("            odd(left)", "            let chosen=if flag:left else:tree\n            if flag:odd(chosen) else:odd(left)");
    let odd = walk("odd", "even")
        .replace("even(left)", "even(left,flag:true)")
        .replace("even(right)", "even(right,flag:false)");
    vec![
        (
            "guarded descendant alias",
            true,
            format!("{TREE}{even}{odd}fn main():even(Leaf(Err(\"leaf\")),flag:true)\n"),
        ),
        (
            "unguarded root alternative",
            false,
            format!(
                "{TREE}{}{odd}fn main():even(Empty,flag:false)\n",
                even.replace("if flag:odd(chosen) else:odd(left)", "odd(chosen)")
            ),
        ),
    ]
}
pub fn alternating_collections() -> Vec<(&'static str, bool, String)> {
    let types = "type A:\n    value:Result(Int,String)\n    children:List(B)\ntype B:\n    value:Result(Int,String)\n    children:List(A)\n";
    let a = "fn even(node:A)->Unit:\n    println(Result.is_err(node.value))\n    for child in node.children:\n        odd(child)\n";
    let b = a
        .replace("fn even(node:A)", "fn odd(node:B)")
        .replace("odd(child)", "even(child)");
    let main = "fn main():even(A(Ok(0),[B(Err(\"child\"),[])]))\n";
    let mut cases = vec![
        (
            "alternating list layouts",
            true,
            format!("{types}{a}{b}{main}"),
        ),
        (
            "alternating list skipped child",
            false,
            format!(
                "{types}{a}{}{main}",
                b.replace("        even(child)", "        even(child)\n        break")
            ),
        ),
    ];
    let types = types
        .replace("List(B)", "Map(String,B)")
        .replace("List(A)", "Map(String,A)");
    let a = a.replace("in node.children:", "in Map.values(node.children):");
    let b = b.replace("in node.children:", "in Map.values(node.children):");
    let main = "fn main():even(A(Ok(0),%{\"child\":B(Err(\"child\"),%{})}))\n";
    cases.push((
        "alternating map layouts",
        true,
        format!("{types}{a}{b}{main}"),
    ));
    cases.push((
        "map projected child",
        false,
        format!(
            "{types}{a}{}{main}",
            b.replace(
                "for child in Map.values(node.children):\n        even(child)",
                "if Map.len(node.children)>0:even(List.head(Map.values(node.children)))"
            )
        ),
    ));
    cases
}
pub fn wrapped_cases() -> Vec<(&'static str, bool, String)> {
    let list = "newtype Wrapped = Wrapped(Node)\ntype Node:\n    value:Result(Int,String)\n    children:List(Wrapped)\nfn even(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    for child in node.children:\n        odd(child.0)\nfn odd(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    for child in node.children:\n        even(child.0)\nfn main():even(Node(Ok(0),[Wrapped(Node(Err(\"child\"),[]))]))\n";
    let map = "newtype Wrapped = Wrapped(Node)\ntype Node:\n    value:Result(Int,String)\n    children:Map(String,Wrapped)\nfn even(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    for child in Map.values(node.children):\n        odd(child.0)\nfn odd(node:Node)->Unit:\n    println(Result.is_err(node.value))\n    for child in Map.values(node.children):\n        even(child.0)\nfn main():even(Node(Ok(0),%{\"child\":Wrapped(Node(Err(\"child\"),%{}))}))\n";
    let nested = list
        .replace(
            "newtype Wrapped = Wrapped(Node)",
            "newtype Outer = Outer(Wrapped)\nnewtype Wrapped = Wrapped(Node)",
        )
        .replace("List(Wrapped)", "List(Outer)")
        .replace("child.0)", "child.0.0)")
        .replace(
            "[Wrapped(Node(Err(\"child\"),[]))]",
            "[Outer(Wrapped(Node(Err(\"child\"),[])))]",
        );
    vec![
        ("unboxed list children", true, list.into()),
        ("unboxed map children", true, map.into()),
        ("nested unboxed children", true, nested),
        (
            "rebuilt unboxed root list",
            false,
            list.replace("in node.children:", "in [Wrapped(node)]:"),
        ),
        (
            "rebuilt unboxed root map",
            false,
            map.replace(
                "Map.values(node.children)",
                "Map.values(%{\"root\":Wrapped(node)})",
            ),
        ),
    ]
}
pub fn cases() -> Vec<(&'static str, bool, String)> {
    let mut cases = tree_cases();
    cases.extend(collection_cases());
    cases.extend(nominal_cases());
    cases.extend(boundary_cases());
    cases.extend(guarded_cases());
    cases.extend(alternating_collections());
    cases.extend(wrapped_cases());
    cases
}
