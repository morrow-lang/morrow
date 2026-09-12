use fern_compiler::{Type, lowering, parse, unit_test};

#[test]
fn discovery_uses_real_source_functions_in_order_once_per_clause_group() {
    let source = "@doc \"\"\"fn test_fake():()\"\"\"\nfn helper():()\nfn test_one():()\nfn test_two() if true:()\nfn test_two():()\n";
    let tests = unit_test::discover(source).unwrap();
    assert_eq!(
        tests.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["test_one", "test_two"]
    );
    for test in tests {
        assert!(source[test.span.start..test.span.end].contains(&test.name));
    }
    assert_eq!(
        unit_test::discover("fn test_args(value:Int):()\n")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn checked_test_entry_accepts_unit_and_result_without_replacing_real_main_calls() {
    for result in ["()", "Ok(())"] {
        let annotation = if result == "()" {
            "()"
        } else {
            "Result((),Int)"
        };
        let source =
            format!("fn main():42\nfn test_main()->{annotation}:\n    main()\n    {result}\n");
        let program = unit_test::prepare(&parse::parse(&source).unwrap(), "test_main").unwrap();
        let entry = program.functions.iter().find(|f| f.name == "main").unwrap();
        assert_eq!(
            entry.return_type,
            if result == "()" {
                Type::Unit
            } else {
                Type::Result(Box::new(Type::Unit), Box::new(Type::Int))
            }
        );
        assert!(lowering::emit(&program).is_ok());
    }
}

#[test]
fn invalid_entry_selection_preserves_checked_program() {
    for source in [
        "fn test_bad():false\n",
        "fn test_bad():256\n",
        "fn test_bad(x:Int):()\n",
        "fn test_bad()->Result(Int,Int):Ok(1)\n",
    ] {
        let syntax = parse::parse(source).unwrap();
        let before = format!("{syntax:?}");
        assert!(unit_test::prepare(&syntax, "test_bad").is_err());
        assert_eq!(format!("{syntax:?}"), before);
    }
}

#[test]
fn source_test_discovery_is_bounded_and_ignores_benchmark_helpers() {
    assert!(
        unit_test::discover("fn bench_work():()\nfn helper():()\n")
            .unwrap()
            .is_empty()
    );
    use std::fmt::Write;
    let mut source = String::new();
    for n in 0..257 {
        writeln!(source, "fn test_{n}():()").unwrap();
    }
    assert!(
        unit_test::discover(&source)
            .unwrap_err()
            .message
            .contains("256")
    );
}

#[test]
fn generic_tests_cannot_be_selected_from_incidental_concrete_specializations() {
    for extra in ["", "fn demand_other()->Result((),Int):test_generic()\n"] {
        let source = format!(
            "fn test_generic()->Result((), e):Ok(())\nfn demand()->Result((),String):test_generic()\n{extra}"
        );
        let syntax = parse::parse(&source).unwrap();
        assert!(
            unit_test::prepare(&syntax, "test_generic")
                .unwrap_err()
                .message
                .contains("generic")
        );
    }
}

#[test]
fn discovery_keeps_invalid_parameterized_tests_for_independent_reporting() {
    let source = "fn test_bad(value:Int):()\nfn test_later():()\n";
    let tests = unit_test::discover(source).unwrap();
    assert_eq!(
        tests
            .iter()
            .map(|test| test.name.as_str())
            .collect::<Vec<_>>(),
        ["test_bad", "test_later"]
    );
    assert!(unit_test::prepare(&parse::parse(source).unwrap(), "test_bad").is_err());
    assert!(unit_test::prepare(&parse::parse(source).unwrap(), "test_later").is_ok());
}

#[test]
fn unrelated_wide_function_schemes_do_not_consume_editor_metadata_for_test_execution() {
    use std::fmt::Write;
    let wide = vec!["Int"; 255].join(",");
    let mut source = format!("type Wide=({wide})\n");
    for n in 0..65 {
        writeln!(source, "fn helper_{n}(value:Wide):()").unwrap();
    }
    source.push_str("fn test_ok():()\n");
    assert!(unit_test::prepare(&parse::parse(&source).unwrap(), "test_ok").is_ok());
}

#[test]
fn native_test_mode_preserves_newtype_capabilities_and_result_payload_layouts() {
    let source = "newtype Key=Key(String)\nfn test_keys()->Result((),Key):\n    if List.contains([Key(\"fern\")],Key(\"fer\"+\"n\")):Ok(())\n    else:Err(Key(\"missing\"))\n";
    let program = unit_test::prepare(&parse::parse(source).unwrap(), "test_keys").unwrap();
    let emitted = lowering::emit_test(&program).unwrap();
    assert!(emitted.contains("fern_rs_test_exit"));
    // List membership uses the runtime collection primitive, not scalar equality.
    assert!(emitted.contains("fern_list_contains_str"), "{emitted}");
}
