use morrow_compiler::{Type, check, ir, parse};
fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
#[test]
fn maps_infer_from_literals_later_uses_and_return_context() {
    let program = checked(
        "fn empty() -> Map(String, Int): %{}\nfn main():\n    let m = Map.new()\n    let n = Map.put(m, \"a\", 1)\n    println(Map.len(n))\n    println(Map.is_empty(empty()))\n    println(Map.contains(n, \"a\"))\n    println(Option.unwrap_or(Map.get(n, \"a\"), 0))\n    println(List.len(Map.keys(n)))\n    println(List.len(Map.values(Map.delete(n, \"a\"))))\n",
    );
    assert!(format!("{:?}", program.functions[0].body.ty).contains("Map"));
}
#[test]
fn generic_maps_and_first_class_new_preserve_concrete_types() {
    checked(
        "fn singleton(key: k, value: v) -> Map(k, v): %{key: value}\nfn main():\n    let make: () -> Map(String, Int) = Map.new\n    println(Map.len(make()))\n    println(Map.len(singleton(true, 1.5)))\n    println(Map.len(singleton(1, \"one\")))\n",
    );
}
#[test]
fn expected_map_value_types_reach_callbacks_and_generic_function_values() {
    checked(
        "type Item:\n    value: Int\nfn id(x: a) -> a: x\nfn callbacks() -> Map(String, (Item) -> Int): %{\"read\": (item) -> item.value}\nfn main():\n    let m: Map(String, (Int) -> Int) = %{\"id\": id}\n    println(Map.len(m))\n    println(Map.len(callbacks()))\n",
    );
}
#[test]
fn maps_reject_bad_keys_mixed_values_and_unresolved_empty_types() {
    for (source, needle) in [
        ("fn main(): Map.len(%{1.5: 1})", "map key"),
        (
            "fn bad(m: Map(Float, Int)) -> Int: 0\nfn main(): ()",
            "map key",
        ),
        ("fn main(): Map.len(%{[1]: 1})", "map key"),
        ("fn main(): Map.len(%{1: 2, true: 3})", "map key"),
        ("fn main(): Map.len(%{1: 2, 3: false})", "map value"),
        ("fn main(): Map.len(%{})", "infer"),
        ("fn main(): Map.get(%{1: 2}, true)", "argument"),
    ] {
        let error = rejected(source);
        assert!(error.contains(needle), "{error}");
    }
}
#[test]
fn maps_keep_result_obligations_and_capture_restriction() {
    for source in [
        "fn main():\n    let m: Map(String, Result(Int, String)) = %{\"a\": Ok(1)}\n    ()",
        "fn main():\n    let m: Map(String, Result(Int, String)) = %{\"a\": Ok(1)}\n    let f = () -> Map.len(m)\n    println(f())",
    ] {
        assert!(rejected(source).contains("Result"));
    }
}
#[test]
fn record_updates_lower_base_and_rhs_once_in_source_order() {
    let p = checked(
        "type Pair:\n    a: Int\n    b: String\nfn pair() -> Pair: Pair(1, \"old\")\nfn main():\n    let changed = %{pair() | b: \"new\", a: 2}\n    println(changed.a)\n",
    );
    let ir::ExprKind::Block(main) = &p
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .body
        .kind
    else {
        panic!()
    };
    let ir::Stmt::Let { value, .. } = &main[0] else {
        panic!()
    };
    let ir::ExprKind::Block(update) = &value.kind else {
        panic!()
    };
    assert_eq!(update.len(), 4);
    assert!(matches!(
        &update[0],
        ir::Stmt::Let {
            value: ir::Expr {
                kind: ir::ExprKind::Call { .. },
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        &update[1],
        ir::Stmt::Let {
            value: ir::Expr {
                ty: Type::String,
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        &update[2],
        ir::Stmt::Let {
            value: ir::Expr { ty: Type::Int, .. },
            ..
        }
    ));
}
#[test]
fn record_updates_check_field_names_types_and_callback_context() {
    checked(
        "type Item:\n    value: Int\ntype Action:\n    run: (Item) -> Int\nfn main():\n    let a = Action((item) -> item.value)\n    let b = %{a | run: (item) -> item.value + 1}\n    println(b.run(Item(2)))\n",
    );
    for (source, needle) in [
        (
            "type Item:\n    value: Int\nfn main(): %{Item(1) | nope: 2}",
            "field",
        ),
        (
            "type Item:\n    value: Int\nfn main(): %{Item(1) | value: true}",
            "field",
        ),
        ("fn main(): %{1 | value: 2}", "record"),
    ] {
        let error = rejected(source);
        assert!(error.contains(needle), "{error}");
    }
}

#[test]
fn nested_maps_and_specialized_key_constraints_are_checked() {
    checked(
        "fn id(m: Map(k, v)) -> Map(k, v): m\nfn main(): println(Map.len(id(%{\"a\": %{1: true}})))\n",
    );
    for source in [
        "fn empty(key: k) -> Map(k, Int): %{}\nfn main(): Map.len(empty(1.5))",
        "fn take(m: Map(k, Int)) -> Int: 0\nfn main(): take(%{1.5: 1})",
        "fn main():\n    let m = Map.new()\n    Map.put(m, 1, m)",
        "fn main():\n    let bad: () -> Map(Float, Int) = Map.new\n    ()",
    ] {
        let message = rejected(source);
        assert!(
            message.contains("map key") || message.contains("recursive"),
            "{message}"
        );
    }
}

#[test]
fn manually_constructed_update_duplicate_fields_and_deep_maps_are_bounded() {
    use morrow_compiler::{Span, ast};
    let mut program =
        parse::parse("type Item:\n    value: Int\nfn main(): %{Item(1) | value: 2}\n").unwrap();
    let ast::ExprKind::RecordUpdate { fields, .. } = &mut program.functions[0].body.kind else {
        panic!()
    };
    fields.push(fields[0].clone());
    assert!(
        check::check(&program)
            .unwrap_err()
            .message
            .contains("duplicate record field")
    );
    let mut expr = ast::Expr {
        kind: ast::ExprKind::Int(0),
        span: Span::default(),
    };
    for _ in 0..140 {
        expr = ast::Expr {
            kind: ast::ExprKind::Map(vec![(
                ast::Expr {
                    kind: ast::ExprKind::Int(1),
                    span: Span::default(),
                },
                expr,
            )]),
            span: Span::default(),
        };
    }
    program.functions[0].body = expr;
    assert!(
        check::check(&program)
            .unwrap_err()
            .message
            .contains("nesting")
    );
}
