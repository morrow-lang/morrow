use morrow_compiler::{check, parse, repl::Session};

const USER_TRAIT: &str = r#"
trait Describe(a):
    fn describe(value: a) -> String

type Point:
    x: Int

impl Describe(Point):
    fn describe(value: Point) -> String:
        "point {value.x}"

fn render(value: a) -> String where Describe(a):
    describe(value)
"#;

#[test]
fn custom_trait_dispatches_through_a_generic_function() {
    let mut session = Session::default();
    session.evaluate(USER_TRAIT.trim()).unwrap();
    assert_eq!(
        session.evaluate("render(Point(42))").unwrap(),
        "\"point 42\" : String\n"
    );
}

#[test]
fn constraints_are_checked_even_when_the_body_does_not_use_a_method() {
    let source = format!(
        "{USER_TRAIT}\nfn require(value: a) -> Int where Describe(a):\n    1\nfn main():\n    print(require(42))\n"
    );
    let syntax = parse::parse(&source).unwrap();
    let error = check::check(&syntax).unwrap_err();
    assert!(
        error.message.contains("Describe") && error.message.contains("implementation"),
        "{error:?}"
    );
}

#[test]
fn trait_implementations_are_coherent_and_complete() {
    for suffix in [
        "impl Describe(Point):\n    fn describe(value: Point) -> String:\n        \"duplicate\"\n",
        "impl Describe(Int):\n    fn different(value: Int) -> String:\n        \"wrong method\"\n",
        "impl Describe(Int):\n    fn describe(value: Int) -> Int:\n        value\n",
    ] {
        let source = format!("{USER_TRAIT}\n{suffix}\nfn main():\n    ()\n");
        let syntax = parse::parse(&source).unwrap();
        assert!(check::check(&syntax).is_err(), "accepted {suffix}");
    }
}

#[test]
fn trait_formatting_preserves_contracts_and_implementation_bodies() {
    let source = format!("{USER_TRAIT}\nfn main():\n    print(render(Point(42)))\n");
    let formatted = morrow_compiler::format::format(&source).unwrap();
    assert!(formatted.contains("where Describe(a)"));
    assert!(formatted.contains("impl Describe(Point):"));
    let program = check::check(&parse::parse(&formatted).unwrap()).unwrap();
    morrow_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn structural_derives_have_independent_value_semantics() {
    let mut session = Session::default();
    session
        .evaluate("type Point derive(Show, Eq, Ord, Clone):\n    x: Int\n    y: Int")
        .unwrap();
    assert_eq!(
        session.evaluate("show(Point(3, 7))").unwrap(),
        "\"Point(x: 3, y: 7)\" : String\n"
    );
    assert_eq!(
        session
            .evaluate("eq(left: Point(3, 7), right: Point(3, 7))")
            .unwrap(),
        "true : Bool\n"
    );
    assert_eq!(
        session
            .evaluate("eq(left: Point(3, 7), right: Point(7, 3))")
            .unwrap(),
        "false : Bool\n"
    );
    assert_eq!(
        session
            .evaluate("compare(left: Point(3, 7), right: Point(7, 3))")
            .unwrap(),
        "Less : Ordering\n"
    );
    assert_eq!(
        session.evaluate("show(clone(Point(3, 7)))").unwrap(),
        "\"Point(x: 3, y: 7)\" : String\n"
    );
}

#[test]
fn recursive_and_generic_derives_preserve_nested_values() {
    let mut session = Session::default();
    session
        .evaluate("type Tree(a) derive(Show, Eq, Clone):\n    Leaf(a)\n    Branch(List(Tree(a)))")
        .unwrap();
    let value = "Branch([Leaf(1), Branch([Leaf(2), Leaf(3)])])";
    assert_eq!(
        session.evaluate(&format!("show(clone({value}))")).unwrap(),
        "\"Branch([Leaf(1), Branch([Leaf(2), Leaf(3)])])\" : String\n"
    );
    assert_eq!(
        session
            .evaluate(&format!("eq(left: {value}, right: clone({value}))"))
            .unwrap(),
        "true : Bool\n"
    );
}

#[test]
fn default_methods_and_generic_implementations_share_constraints() {
    let mut session = Session::default();
    session.evaluate("type Box(a):\n    value: a\nimpl Show(Box(a)) where Show(a):\n    fn show(value: Box(a)) -> String:\n        show(value.value)").unwrap();
    assert_eq!(
        session.evaluate("show(Box(123))").unwrap(),
        "\"123\" : String\n"
    );
    session
        .evaluate("type Point derive(Eq):\n    value: Int")
        .unwrap();
    assert_eq!(
        session
            .evaluate("neq(left: Point(1), right: Point(2))")
            .unwrap(),
        "true : Bool\n"
    );
}

#[test]
fn derived_order_is_lexicographic_for_unicode_strings() {
    let mut session = Session::default();
    session
        .evaluate("type User derive(Eq, Ord):\n    name: String\n    age: Int")
        .unwrap();
    assert_eq!(session.evaluate("match compare(left: User(\"A\", 99), right: User(\"🌿\", 1)):\n    Less -> 1\n    _ -> 0").unwrap(),"1 : Int\n");
}

#[test]
fn traits_cannot_hide_dropped_results_in_concrete_or_unused_bodies() {
    for source in [
        "trait Consume(a):\n    fn consume(value: a) -> Int\nimpl Consume(Result(Int,String)):\n    fn consume(value: Result(Int,String)) -> Int: 1\nfn main():\n    let value:Result(Int,String)=Err(\"bad\")\n    println(consume(value))\n",
        "trait Make(a):\n    fn make(value: a) -> Result(Int,String)\nimpl Make(Int):\n    fn make(value: Int) -> Result(Int,String): Err(\"bad\")\nfn ignored(value: a) -> Int where Make(a):\n    make(value)\n    1\nfn main(): ()\n",
        "trait Keep(a):\n    fn keep(value: a) -> a\nimpl Keep(Result(Int,String)):\n    fn keep(value: Result(Int,String)) -> Result(Int,String): value\nfn main():\n    let value:Result(Int,String)=Err(\"bad\")\n    keep(value)\n    ()\n",
    ] {
        let syntax = parse::parse(source).unwrap();
        let error = check::check(&syntax).expect_err(source);
        assert!(error.message.contains("Result"), "{error:?}: {source}");
    }
}

#[test]
fn deterministic_trait_simulation_matches_independent_integer_oracles() {
    let mut session = Session::default();
    session
        .evaluate("type Key derive(Eq, Ord, Show, Clone):\n    first: Int\n    second: Int")
        .unwrap();
    let mut state = 0x6a09_e667_f3bc_c909_u64;
    for _ in 0..64 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let a = (state as i64) % 1000;
        let b = ((state >> 17) as i64) % 1000;
        let c = ((state >> 33) as i64) % 1000;
        let d = ((state >> 49) as i64) % 1000;
        let expected = match (a, b).cmp(&(c, d)) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        let source = format!(
            "match compare(left: clone(Key({a}, {b})), right: Key({c}, {d})):\n    Less -> -1\n    Equal -> 0\n    Greater -> 1"
        );
        assert_eq!(
            session.evaluate(&source).unwrap(),
            format!("{expected} : Int\n"),
            "seed state {state}"
        );
        assert_eq!(
            session
                .evaluate(&format!(
                    "eq(left: Key({a}, {b}), right: clone(Key({a}, {b})))"
                ))
                .unwrap(),
            "true : Bool\n"
        );
    }
}

#[test]
fn trait_parents_overlap_and_unknown_bounds_fail_deterministically() {
    for source in [
        "trait A(a) with B(a):\n    fn first(value:a)->Int\ntrait B(a) with A(a):\n    fn second(value:a)->Int\n",
        "trait T(a):\n    fn method(value:a)->Int\nimpl T(List(a)):\n    fn method(value:List(a))->Int: 0\nimpl T(List(Int)):\n    fn method(value:List(Int))->Int: 1\n",
        "fn invalid(value:a)->Int where Missing(a): 1\n",
        "fn invalid(value:a)->Int where Show(b): 1\n",
        "type OnlyOrd derive(Ord):\n    value:Int\n",
    ] {
        let syntax = parse::parse(&format!("{source}\nfn main(): ()\n")).unwrap();
        assert!(
            check::check(&syntax).is_err(),
            "accepted invalid contract {source}"
        );
    }
}

#[test]
fn public_traits_and_impls_resolve_across_modules() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Project(std::path::PathBuf);
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let project = Project(std::env::temp_dir().join(format!(
        "morrow-traits-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir(&project.0).unwrap();
    std::fs::write(project.0.join("model.fn"),"@doc \"\"\"Named developer-facing rendering contract.\"\"\"\npub trait Describe(a):\n    fn describe(value:a)->String\npub type Point:\n    x:Int\nimpl Describe(Point):\n    fn describe(value:Point)->String: \"point {value.x}\"\n").unwrap();
    let main = project.0.join("main.fn");
    std::fs::write(&main,"import model.*\nfn render(value:a)->String where Describe(a): describe(value)\nfn main(): println(render(Point(42)))\n").unwrap();
    let loaded = morrow_compiler::modules::load(&main).unwrap();
    let checked = check::check(&loaded.program).unwrap();
    morrow_compiler::lowering::emit(&checked).unwrap();
}

#[test]
fn map_and_tuple_derives_use_structural_value_semantics() {
    let mut session = Session::default();
    session.evaluate("type Data derive(Show, Eq, Clone):\n    entries:Map(String,Int)\n    pair:(Int,String)").unwrap();
    let a = "Data(%{\"a\": 1, \"b\": 2}, (7, \"morrow\"))";
    let b = "Data(%{\"b\": 2, \"a\": 1}, (7, \"morrow\"))";
    assert_eq!(
        session
            .evaluate(&format!("eq(left: {a}, right: clone({b}))"))
            .unwrap(),
        "true : Bool\n"
    );
    assert_eq!(
        session.evaluate(&format!("show({a})")).unwrap(),
        "\"Data(entries: %{\\\"a\\\": 1, \\\"b\\\": 2}, pair: (7, \\\"morrow\\\"))\" : String\n"
    );
}

#[test]
fn unused_trait_constraints_include_requirements_of_every_implementation_method() {
    let source = "type Box(a):\n    value:a\nimpl Show(Box(a)):\n    fn show(value:Box(a))->String: show(value.value)\nfn need(value:a)->Int where Show(a): 1\nfn main(): println(need(Box((x:Int)->x)))\n";
    let error = check::check(&parse::parse(source).unwrap()).unwrap_err();
    assert!(error.message.contains("implementation"), "{error:?}");
}

#[test]
fn implementation_methods_support_exhaustive_pattern_clauses_and_formatting() {
    let source = r#"trait Describe(a):
    fn describe(value: a) -> String
impl Describe(Option(Int)):
    fn describe(None: Option(Int)) -> String: "none"
    fn describe(Some(number): Option(Int)) -> String: "some {number}"
"#;
    let formatted = morrow_compiler::format::format(source).unwrap();
    let mut session = morrow_compiler::repl::Session::default();
    session.evaluate(&formatted).unwrap();
    assert_eq!(
        session.evaluate("describe(Some(42))").unwrap(),
        "\"some 42\" : String\n"
    );
    session
        .evaluate("fn absent() -> Option(Int): None")
        .unwrap();
    assert_eq!(
        session.evaluate("describe(absent())").unwrap(),
        "\"none\" : String\n"
    );
}
