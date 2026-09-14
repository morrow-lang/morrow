//! Published rejection contracts evolve alongside positive actor execution coverage.
#[test]
fn current_actor_rejections_remain_atomic_and_specific() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("actors/invalid.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let result = morrow_compiler::parse::parse(source)
            .and_then(|ast| morrow_compiler::check::check(&ast));
        let error = result.unwrap_err();
        assert!(
            error.message.contains(case["diagnostic"].as_str().unwrap()),
            "{}: {}",
            case["name"],
            error.message
        );
    }
}
#[test]
fn retired_actor_rejections_are_positive_continuation_contracts() {
    for source in [
        "fn worker():\n    defer println(\"cleanup\")\n    receive:\n        1 -> ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
        "fn worker():\n    for index in 0..2:\n        receive:\n            1 -> ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
        "fn worker():\n    receive:\n        1 -> worker()\n    println(\"after recursive actor\")\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
    ] {
        let ast = morrow_compiler::parse::parse(source).unwrap();
        let checked = morrow_compiler::check::check(&ast).unwrap();
        morrow_compiler::lowering::lower(&checked).unwrap();
    }
}

#[test]
fn recursive_actor_match_guards_are_rejected_before_execution() {
    let source = "fn again(n: Int) -> Bool: again(n)\nfn worker():\n    match 1:\n        value if again(value) -> ()\n        _ -> ()\nfn main():\n    let pid: Pid(()) = spawn(worker)\n    ()";
    let ast = morrow_compiler::parse::parse(source).unwrap();
    let checked = morrow_compiler::check::check(&ast).unwrap();
    let error = morrow_compiler::lowering::lower(&checked).unwrap_err();
    assert!(
        error.message.contains("actor match guard must be finite"),
        "{}",
        error.message
    );
}

#[test]
fn deterministic_repl_callback_campaign_matches_independent_list_models() {
    for mut seed in [7_u64, 91, 0x4645524e] {
        let mut values = Vec::new();
        for _ in 0..40 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            values.push((seed % 41) as i64 - 20);
        }
        let literal = values
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            r#"
fn descend(n: Int) -> Int:
    if n == 0: 0
    else: 1 + descend(n - 1)
fn worker():
    let offset = 4294967297
    let values = List.map([{literal}], (value: Int) -> value + offset + descend(2))
    let kept = List.filter(values, (value: Int) -> value % 2 == 0)
    for value in kept: println(value)
    println(List.fold(kept, 0, (acc: Int, value: Int) -> acc + value))
    println(Option.unwrap_or(List.find(values, (value: Int) -> value > offset), -1))
    println(List.any(values, (value: Int) -> value > offset))
    println(List.all(values, (value: Int) -> value > offset))
let first: Pid(()) = spawn(worker)
let second: Pid(()) = spawn(() -> println("sibling"))
()
"#
        );
        let mut expected = String::from("sibling\n");
        let mapped: Vec<_> = values.iter().map(|v| v + 4294967299_i64).collect();
        let kept: Vec<_> = mapped.iter().copied().filter(|v| v % 2 == 0).collect();
        for value in &kept {
            expected.push_str(&format!("{value}\n"));
        }
        expected.push_str(&format!(
            "{}\n{}\n{}\n{}\n",
            kept.iter().sum::<i64>(),
            mapped
                .iter()
                .copied()
                .find(|v| *v > 4294967297)
                .unwrap_or(-1),
            mapped.iter().any(|v| *v > 4294967297),
            mapped.iter().all(|v| *v > 4294967297)
        ));
        for _ in 0..2 {
            let mut session = morrow_compiler::repl::Session::default();
            let (definitions, statements) = source.split_once("\nlet first:").unwrap();
            session.evaluate(definitions).unwrap();
            assert_eq!(
                session
                    .evaluate(&format!("let first:{statements}"))
                    .unwrap(),
                expected
            );
            assert_eq!(session.actor_report().live, 0);
        }
    }
}
