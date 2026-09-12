use fern_compiler::{check, format, parse};

#[test]
fn unused_derived_records_validate_every_payload_and_trait() {
    for (source, message) in [
        (
            "type User derive(Show):\n    age:Int\n",
            "unsupported derive trait",
        ),
        (
            "type User derive(Json):\n    callback:(Int)->Int\n",
            "function",
        ),
        (
            "type User derive(Json):\n    payload:Result(Int,String)\n",
            "Result",
        ),
        ("type User derive(Json):\n    value:Option(Unit)\n", "null"),
    ] {
        let program = parse::parse(&format!("{source}fn main():()\n")).unwrap();
        let error = check::check(&program).unwrap_err();
        assert!(error.message.contains(message), "{error:?}");
    }
}

#[test]
fn derives_round_trip_without_losing_source_metadata() {
    let source =
        "pub type User derive(Json):\n    name:String\n    nick:Option(String)\nfn main():()\n";
    let formatted = format::format(source).unwrap();
    assert!(formatted.contains("derive(Json)"), "{formatted}");
    check::check(&parse::parse(&formatted).unwrap()).unwrap();
}

#[test]
fn forged_type_targets_never_hide_in_value_positions_or_nested_arguments() {
    use fern_compiler::{Type, ast};
    for source in [
        "fn main():1\n",
        "fn main():json.encode([1])\n",
        "fn main():json.decode(\"1\",[Int])\n",
    ] {
        let mut program = parse::parse(source).unwrap();
        let root = &mut program.functions[0].body;
        let target = ast::ExprKind::TypeTarget(Type::Int);
        match &mut root.kind {
            ast::ExprKind::Call { args, .. } => {
                let last = args.last_mut().unwrap();
                let ast::ExprKind::List(items) = &mut last.value.kind else {
                    panic!("list")
                };
                items[0].kind = target;
            }
            _ => root.kind = target,
        }
        let error = check::check(&program).unwrap_err();
        assert!(
            error
                .message
                .contains("type target cannot be used as a value"),
            "{error:?}"
        );
    }
}

#[test]
fn ordinary_codec_results_cannot_be_silently_discarded() {
    for source in [
        "fn main():json.encode(1)\n",
        "fn main():json.decode(\"1\",Int)\n",
    ] {
        let error = check::check(&parse::parse(source).unwrap()).unwrap_err();
        assert!(error.message.contains("Result"), "{error:?}");
    }
}
#[test]
fn direct_ast_checking_rejects_executable_decoder_targets_before_value_checking() {
    let program = parse::parse("fn effect():1\nfn main():json.decode(\"1\",effect())\n").unwrap();
    let error = check::check(&program).unwrap_err();
    assert!(error.message.contains("target must be a type"), "{error:?}");
}
#[test]
fn canonical_codec_identities_cannot_be_redefined_as_source_functions() {
    let mut program = parse::parse("fn replacement(value:Int)->Int:value\nfn main():()\n").unwrap();
    program.functions[0].name = "json.decode".into();
    let error = check::check(&program).unwrap_err();
    assert!(error.message.contains("reserved"), "{error:?}");
}

#[test]
fn unused_derived_sums_are_validated_without_requiring_a_record_shape() {
    let source = "type State derive(Json):\n    On\n    Off\nfn main():()\n";
    check::check(&parse::parse(source).unwrap()).unwrap();
}
