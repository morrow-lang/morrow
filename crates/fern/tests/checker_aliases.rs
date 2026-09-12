use fern_compiler::{Type, check, ir, lowering, parse};
fn checked(source: &str) -> ir::Program {
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    lowering::emit(&program).unwrap();
    program
}
#[test]
fn scalar_aliases_are_transparent_in_arguments_returns_and_arithmetic() {
    let program = checked(
        "type UserId = Int\npub fn next(id: UserId) -> UserId: id + 1\nfn main(): println(next(41))\n",
    );
    let function = program.functions.iter().find(|f| f.name == "next").unwrap();
    assert_eq!(function.params[0].ty, Type::Int);
    assert_eq!(function.return_type, Type::Int);
}
#[test]
fn aliases_expand_forward_through_nominal_and_container_annotations() {
    checked(
        "type Headers = Map(String, Value)\ntype Value = Option(Row)\ntype Row:\n    value: Int\nfn make() -> Headers: %{\"key\": Some(Row(42))}\nfn main(): println(Map.len(make()))\n",
    );
    checked(
        "type Link = Node\ntype Node:\n    value: Int\n    next: Option(Link)\nfn read(node: Link) -> Int: node.value\nfn main(): println(read(Node(42, None)))\n",
    );
}
#[test]
fn generic_alias_substitution_keeps_formal_names_capture_free() {
    checked(
        "type Pair(a, b) = (a, b)\nfn pair(x: b, y: a) -> Pair(b, a): (x, y)\nfn main(): println(pair(42, \"s\").1)\n",
    );
}
#[test]
fn callback_and_result_aliases_keep_existing_semantic_obligations() {
    checked(
        "type Callback(a) = (a) -> a\ntype Outcome(a) = Result(a, String)\nfn apply(f: Callback(Int), x: Int) -> Outcome(Int): Ok(f(x))\nfn main() -> Outcome(Unit):\n    println(apply((x) -> x + 1, 41)?)\n    Ok(())\n",
    );
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
#[test]
fn alias_cycles_arities_and_undeclared_variables_are_diagnostics() {
    for source in [
        "type A = A",
        "type A = B\ntype B = List(A)",
        "type A(a) = A(List(a))",
    ] {
        assert!(rejected(&format!("{source}\nfn main(): ()\n")).contains("cycle"));
    }
    assert!(
        rejected("type Pair(a,b) = (a,b)\nfn f(x: Pair(Int)) -> Int: 1\nfn main(): ()\n")
            .contains("argument")
    );
    assert!(rejected("type Value(a) = (a,b)\nfn main(): ()\n").contains("undeclared"));
    assert!(rejected("type Value = Missing\nfn main(): ()\n").contains("unknown"));
}
#[test]
fn aliases_add_no_constructors_and_cannot_shadow_other_types() {
    assert!(rejected("type Id = Int\nfn main(): Id(1)\n").contains("alias"));
    checked("type Id = Int\nfn Id() -> Int: 1\nfn main(): println(Id())\n");
    for source in ["type Id = Int\ntype Id = String", "type Int = String"] {
        assert!(
            check::check(&parse::parse(&format!("{source}\nfn main(): ()\n")).unwrap()).is_err()
        );
    }
    assert!(rejected("type Outcome = Result(Int, String)\nfn main():\n    let ignored: Outcome = Ok(1)\n    ()\n").contains("Result"));
}
#[test]
fn expanded_type_trees_are_bounded_before_retention() {
    let mut source = String::from("type Pair(a) = (a,a)\ntype T0 = Int\n");
    for n in 1..30 {
        source.push_str(&format!("type T{n} = Pair(T{})\n", n - 1));
    }
    source.push_str("fn main(): ()\n");
    let message = rejected(&source);
    assert!(message.contains("limit"), "{message}");
}
#[test]
fn aliases_work_in_local_lambda_and_pattern_annotations() {
    checked(
        "type Id = Int\ntype Pair = (Id, String)\nfn make():\n    let pair: Pair = (42, \"s\")\n    let (id, label): Pair = pair\n    let f = (x: Id) -> x + id\n    (f, label)\nfn main(): println(make().0(1))\n",
    );
}

#[test]
fn raw_alias_trees_are_preflight_bounded_before_ast_cloning() {
    let mut program = parse::parse("fn main(): ()\n").unwrap();
    program.aliases = (0..40)
        .map(|n| fern_compiler::ast::TypeAlias {
            public: false,
            name: format!("Alias{n}"),
            parameters: vec![],
            target: Type::Tuple(vec![Type::Int; 3000]),
            span: Default::default(),
        })
        .collect();
    let message = check::check(&program).unwrap_err().message;
    assert!(message.contains("syntax size limit"), "{message}");
}
#[test]
fn invalid_alias_formals_and_non_source_types_are_rejected() {
    for source in [
        "type Value(a,a) = a",
        "type Value(A) = Int",
        "type value = Int",
    ] {
        assert!(
            parse::parse(&format!("{source}\nfn main(): ()\n"))
                .map(|p| check::check(&p).is_err())
                .unwrap_or(true)
        );
    }
    for ty in [
        Type::Infer(0),
        Type::Never,
        Type::Named("Int".into(), vec![]),
    ] {
        let mut program = parse::parse("type Alias = Int\nfn main(): ()\n").unwrap();
        program.aliases[0].target = ty;
        assert!(check::check(&program).is_err());
    }
}

#[test]
fn aliases_expand_before_map_key_capability_validation() {
    checked(
        "type Key = String\ntype Values = Map(Key, Int)\nfn build() -> Values: %{\"answer\": 42}\nfn main(): println(Map.len(build()))\n",
    );
    assert!(
        rejected("type Key = Float\ntype Values = Map(Key, Int)\nfn main(): ()\n")
            .contains("map key")
    );
}

#[test]
fn capitalized_unicode_alias_names_follow_source_identifier_rules() {
    let source = "type Δείκτης = Int\nfn identity(value: Δείκτης) -> Δείκτης: value\nfn main(): println(identity(42))\n";
    let parsed = fern_compiler::parse::parse(source).unwrap();
    let checked = fern_compiler::check::check(&parsed).unwrap();
    fern_compiler::lowering::emit(&checked).unwrap();
}
