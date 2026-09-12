use fern_compiler::{check, lowering, parse, unit_test};

#[test]
fn test_entries_reject_direct_and_first_class_process_exit_invocation() {
    for source in [
        "fn test_exit(): System.exit(0)\n",
        "fn test_exit():\n    let quit = System.exit\n    quit(0)\n",
        "fn quit(): System.exit(0)\nfn test_exit(): quit()\n",
    ] {
        let program = unit_test::prepare(&parse::parse(source).unwrap(), "test_exit").unwrap();
        let ordinary = lowering::emit(&program).unwrap();
        let test = lowering::emit_test(&program).unwrap();
        assert!(
            ordinary.contains("call $fern_exit(l 0)") || ordinary.contains("call $fern_exit(l %")
        );
        assert!(!ordinary.contains("fern_rs_test_exit"));
        assert!(test.contains("call $fern_rs_test_exit(l "), "{test}");
        assert!(!test.contains("call $fern_exit(l 0)"), "{test}");
        assert!(test.contains("call $fern_exit(l 1)"), "{test}");
        assert!(test.contains("System.exit cannot terminate a test"));
        assert_eq!(lowering::emit(&program).unwrap(), ordinary);
    }
}

#[test]
fn unused_application_exit_does_not_invalidate_a_test_entry() {
    let source = "fn main(): System.exit(0)\nfn test_ok(): ()\n";
    let program = unit_test::prepare(&parse::parse(source).unwrap(), "test_ok").unwrap();
    let il = lowering::emit_test(&program).unwrap();
    let entry = program
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let wrapper = il.split("export function w $fern_main()").nth(1).unwrap();
    assert!(wrapper.contains(&format!("call $f{}(", entry.id.0)));
    assert!(il.contains("call $fern_rs_test_exit(l 0)"));
}

#[test]
fn test_mode_retains_public_ir_validation_and_result_entry_contracts() {
    for source in ["fn main()->Result((),Int): Ok(())\n", "fn main(): ()\n"] {
        let program = check::check(&parse::parse(source).unwrap()).unwrap();
        assert!(lowering::emit_test(&program).is_ok());
        let mut bad = program;
        bad.functions[0].params.push(fern_compiler::ir::Param {
            id: fern_compiler::ir::LocalId(0),
            ty: fern_compiler::Type::Int,
        });
        assert!(lowering::emit_test(&bad).is_err());
    }
}
