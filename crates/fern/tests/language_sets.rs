use fern_compiler::{check, parse, repl::Session};

#[test]
fn sets_preserve_nominal_identity_and_reject_unhandled_results() {
    for source in [
        "fn main():\n    let s: Set(Int) = Map.new()\n    ()",
        "fn main():\n    let s: Set(Int) = Set.new()\n    Map.len(s)",
        "fn main():\n    let s: Set(Float) = Set.new()\n    ()",
        "fn main():\n    let s: Set(Result(Int, String)) = Set.new()\n    ()",
        "fn main():\n    let s: Set(Int) = Set.new()\n    s.0",
    ] {
        assert!(
            check::check(&parse::parse(source).unwrap()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn immutable_sets_execute_algebra_and_function_values() {
    let mut session = Session::default();
    session
        .evaluate("let a = Set.from_list([3, 1, 3, 2])")
        .unwrap();
    session
        .evaluate("let b = Set.from_list([2, 4, 2])")
        .unwrap();
    for (source, expected) in [
        ("Set.to_list(a)", "[3, 1, 2] : List(Int)\n"),
        ("Set.to_list(Set.union(a, b))", "[3, 1, 2, 4] : List(Int)\n"),
        ("Set.to_list(Set.intersection(a, b))", "[2] : List(Int)\n"),
        ("Set.to_list(Set.difference(a, b))", "[3, 1] : List(Int)\n"),
        ("Set.is_subset(b, a)", "false : Bool\n"),
        ("Set.is_subset(Set.intersection(a, b), a)", "true : Bool\n"),
        ("Set.equal(a, Set.from_list([2, 1, 3]))", "true : Bool\n"),
        (
            "Set.to_list(Set.insert(Set.delete(a, 1), 1))",
            "[3, 2, 1] : List(Int)\n",
        ),
        ("Set.len(a)", "3 : Int\n"),
        ("Set.is_empty(Set.difference(a, a))", "true : Bool\n"),
    ] {
        assert_eq!(session.evaluate(source).unwrap(), expected, "{source}");
    }
    session
        .evaluate("let make: () -> Set(String) = Set.new")
        .unwrap();
    assert_eq!(session.evaluate("Set.len(make())").unwrap(), "0 : Int\n");
}

#[test]
fn sets_keep_unicode_full_width_and_nominal_key_equality() {
    let mut session = Session::default();
    session
        .evaluate("let words = Set.from_list([\"🌿\", \"café\", \"\" + \"🌿\"])")
        .unwrap();
    assert_eq!(session.evaluate("Set.len(words)").unwrap(), "2 : Int\n");
    assert_eq!(
        session
            .evaluate("Set.contains(words, \"caf\" + \"é\")")
            .unwrap(),
        "true : Bool\n"
    );
    session
        .evaluate(
            "let wide = Set.from_list([-9223372036854775808, 9223372036854775807, 4294967296, 0])",
        )
        .unwrap();
    assert_eq!(session.evaluate("Set.len(wide)").unwrap(), "4 : Int\n");
    session.evaluate("newtype UserId = UserId(Int)").unwrap();
    session
        .evaluate("let ids = Set.from_list([UserId(4294967296), UserId(0), UserId(4294967296)])")
        .unwrap();
    assert_eq!(session.evaluate("Set.len(ids)").unwrap(), "2 : Int\n");
    assert!(session.evaluate("Set.contains(ids, 4294967296)").is_err());
    assert_eq!(
        session
            .evaluate("Set.contains(ids, UserId(4294967296))")
            .unwrap(),
        "true : Bool\n"
    );
    session.evaluate("type Users = Set(UserId)").unwrap();
    session
        .evaluate("fn remember(xs: Set(a), x: a) -> Set(a): Set.insert(xs, x)")
        .unwrap();
    session
        .evaluate("let added: Users = remember(ids, UserId(7))")
        .unwrap();
    assert_eq!(session.evaluate("Set.len(added)").unwrap(), "3 : Int\n");
    assert_eq!(session.evaluate("Set.len(ids)").unwrap(), "2 : Int\n");
}

#[test]
fn seeded_set_simulation_matches_independent_membership_and_persistence_model() {
    use std::collections::BTreeSet;
    for seed in [1u64, 42, 0xfeed_beef] {
        let mut rng = seed;
        let mut model = BTreeSet::new();
        let mut order = Vec::new();
        let mut session = Session::default();
        session.evaluate("let state: Set(Int) = Set.new()").unwrap();
        session.evaluate("let empty = state").unwrap();
        for step in 0..80 {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let key = ((rng >> 32) % 23) as i64 - 11;
            let method = if rng & 3 == 0 { "delete" } else { "insert" };
            if method == "delete" {
                model.remove(&key);
                order.retain(|v| *v != key);
            } else if model.insert(key) {
                order.push(key);
            }
            session
                .evaluate(&format!("let state = Set.{method}(state, {key})"))
                .unwrap();
            let context = format!("seed={seed} step={step}");
            assert_eq!(
                session.evaluate("Set.len(state)").unwrap(),
                format!("{} : Int\n", model.len()),
                "{context}"
            );
            assert_eq!(
                session
                    .evaluate(&format!("Set.contains(state, {key})"))
                    .unwrap(),
                format!("{} : Bool\n", model.contains(&key)),
                "{context}"
            );
            assert_eq!(
                session.evaluate("Set.to_list(state)").unwrap(),
                format!("{order:?} : List(Int)\n"),
                "{context}"
            );
            assert_eq!(
                session.evaluate("Set.is_empty(empty)").unwrap(),
                "true : Bool\n",
                "{context}"
            );
            if step % 10 == 0 {
                assert_eq!(
                    session
                        .evaluate("Set.equal(Set.union(state, empty), state)")
                        .unwrap(),
                    "true : Bool\n",
                    "{context}"
                );
                assert_eq!(
                    session
                        .evaluate("Set.is_empty(Set.difference(state, state))")
                        .unwrap(),
                    "true : Bool\n",
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn set_membership_retains_source_evaluation_order_and_display() {
    let mut session = Session::default();
    session
        .evaluate("fn key() -> String:\n    println(\"key\")\n    \"🌿\"")
        .unwrap();
    session.evaluate("fn values() -> Set(String):\n    println(\"set\")\n    Set.from_list([\"🌿\", \"café\"])").unwrap();
    assert_eq!(
        session.evaluate("key() in values()").unwrap(),
        "key\nset\ntrue : Bool\n"
    );
    assert_eq!(
        session.evaluate("Set.from_list([1, 2, 1])").unwrap(),
        "Set([1, 2]) : Set(Int)\n"
    );
}

#[test]
fn set_and_map_key_views_keep_list_shape_during_result_analysis() {
    for view in [
        "Set.to_list(Set.from_list([7, 8]))",
        "Map.keys(%{7: 0, 8: 0})",
    ] {
        let source = format!(
            "fn main():\n    let result: Result(Int, String) = Ok(1)\n    println(List.head({view}))\n    match result:\n        Ok(x) -> println(x)\n        Err(e) -> println(e)\n"
        );
        check::check(&parse::parse(&source).unwrap()).unwrap();
    }
}
