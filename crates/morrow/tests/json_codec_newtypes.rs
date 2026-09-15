use morrow_compiler::{check, format, parse, repl::Session};
#[test]
fn explicit_newtype_derive_roundtrips_and_checks() {
    let source = "pub newtype Id derive(Json) = Id(Int)\nnewtype Box(a) derive(Json) = Box(a)\nfn main() -> Result(Unit,json.Error):\n    println(json.encode(Box(Id(42)))?)\n    Ok(())\n";
    let ast = parse::parse(source).unwrap();
    check::check(&ast).unwrap();
    let text = format::format(source).unwrap();
    assert!(
        text.contains("pub newtype Id derive(Json) = Id(Int)"),
        "{text}"
    );
    check::check(&parse::parse(&text).unwrap()).unwrap();
}
#[test]
fn nullable_newtype_field_is_required_but_actual_option_is_optional() {
    let mut session = Session::default();
    session.evaluate("newtype Maybe derive(Json) = Maybe(Option(Int))\ntype Record derive(Json):\n    required:Maybe\n    optional:Option(Int)").unwrap();
    let source = "match json.decode(\"\\{\\\"required\\\":null\\}\",Record):\n    Ok(value) -> println(json.encode(value) |> Result.unwrap_or(\"bad\"))\n    Err(error) -> println(json.error_code(error))";
    assert_eq!(
        session.evaluate(source).unwrap(),
        "{\"required\":null,\"optional\":null}\n"
    );
    let source = "match json.decode(\"\\{\\}\",Record):\n    Ok(_) -> println(0)\n    Err(error) ->\n        println(json.error_code(error))\n        println(json.error_path(error))";
    assert_eq!(session.evaluate(source).unwrap(), "6\n/required\n");
}
#[test]
fn newtype_codec_derive_does_not_launder_results_or_null_collisions() {
    for source in [
        "newtype Raw = Raw(Int)\nfn main():match json.encode(Raw(1)):\n    Ok(_) -> ()\n    Err(_) -> ()\n",
        "newtype Bad derive(Json) = Bad(Result(Int,String))\nfn main():()\n",
        "newtype Nullable derive(Json) = Nullable(Unit)\ntype Bad derive(Json):\n    value:Option(Nullable)\nfn main():()\n",
        "newtype Bad derive(UnknownTrait) = Bad(Int)\nfn main():()\n",
    ] {
        let ast = parse::parse(source).unwrap();
        assert!(check::check(&ast).is_err(), "{source}");
    }
    check::check(&parse::parse("newtype Bad derive(Show) = Bad(Int)\nfn main():()\n").unwrap())
        .unwrap();
}
#[test]
fn malformed_newtype_derives_use_the_existing_nonempty_list_grammar() {
    for source in [
        "newtype Id derive() = Id(Int)",
        "newtype Id derive(Json,) = Id(Int)",
        "newtype Id derive(Json,Json) = Id(Int)",
    ] {
        assert!(parse::parse(source).is_err());
    }
}

#[test]
fn public_newtype_plans_reject_storage_and_optional_marker_forgery() {
    use morrow_compiler::{
        Span, Type, ir,
        json_codec::{Entry, Kind, Plan},
    };
    let ty = Type::Named("Id".into(), vec![]);
    let plan = Plan {
        root: 1,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
            Entry {
                ty: ty.clone(),
                kind: Kind::Newtype(0),
            },
        ],
    };
    let layout = ir::TypeLayout {
        variant_names: Vec::new(),
        ty,
        storage: ir::LayoutStorage::Unboxed,
        fields: vec![],
        variants: vec![vec![Type::Int]],
    };
    plan.validate(std::slice::from_ref(&layout), Span::default())
        .unwrap();
    let mut forged = layout.clone();
    forged.storage = ir::LayoutStorage::Tagged;
    assert!(plan.validate(&[forged], Span::default()).is_err());
    let mut forged = layout;
    forged.variants[0][0] = Type::Float;
    assert!(plan.validate(&[forged], Span::default()).is_err());
    let source = "newtype Maybe derive(Json)=Maybe(Option(Int))\ntype Row derive(Json):\n    field:Maybe\nfn read()->Result(Row,json.Error):json.decode(\"\\{\\}\",Row)\nfn main():()\n";
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    let ir::ExprKind::JsonCodec { plan, .. } = &program.functions[0].body.kind else {
        panic!("codec");
    };
    let mut forged = (**plan).clone();
    let Kind::Record(fields) = &mut forged.entries[forged.root].kind else {
        panic!("record");
    };
    fields[0].optional = true;
    assert!(forged.validate(&program.types, Span::default()).is_err());
}
#[test]
fn newtype_docs_keep_explicit_opt_in_and_distinct_source_ownership() {
    use morrow_compiler::documentation::{Output, render};
    let source =
        "@doc \"\"\"Wire ID.\"\"\"\npub newtype Id derive(Json) = Packed(Int)\nfn Id(): 99\n";
    let output = render(source, "ids.mr", Output::Markdown).unwrap();
    assert!(
        output.contains("pub newtype Id derive(Json) = Packed(Int)"),
        "{output}"
    );
    assert_eq!(output.matches("Wire ID.").count(), 1);
}

#[test]
fn native_newtype_sources_have_identical_repl_outputs() {
    for (source, expected) in [
        (
            include_str!("json_newtype_native/values.mr"),
            include_str!("json_newtype_native/values.stdout"),
        ),
        (
            include_str!("json_newtype_native/nullable.mr"),
            include_str!("json_newtype_native/nullable.stdout"),
        ),
    ] {
        let mut session = Session::default();
        session
            .evaluate(&source.replace("fn main(", "fn newtype_case("))
            .unwrap();
        assert_eq!(session.evaluate("match newtype_case():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_message(error))").unwrap(),expected);
    }
}

#[test]
fn native_newtype_constructor_and_codec_input_need_no_wrapper_allocation() {
    use morrow_compiler::lowering;
    let source = "newtype Id derive(Json)=Id(Int)\nfn make(value:Int)->Id:Id(value)\nfn encode(value:Id)->Result(String,json.Error):json.encode(value)\nfn main():()\n";
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    let output = lowering::emit(&program).unwrap();
    assert!(output.contains("call $morrow_json_codec_encode"));
    assert!(!output.contains("call $morrow_alloc"), "{output}");
}
