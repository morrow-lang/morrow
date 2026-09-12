use fern_compiler::{Type, ast, check, format, lowering, parse, repl::Session};

fn read(target: &str) -> String {
    format!("fn read(text:String):json.decode(text,{target})\n")
}

#[test]
fn inline_compound_targets_use_the_existing_type_grammar_and_formatter() {
    for target in [
        "Int | String",
        "List(Int | String)",
        "Map(String,Int | Bool)",
        "(Int | String, Bool)",
        "(Int | String,)",
        "Option(Int | String)",
        "List(Map(String,(Int | String,Bool)))",
        "Int | Int | String",
        "List(\n        Int |\n        String\n    )",
    ] {
        let source = read(target) + "fn main():()\n";
        let ast = parse::parse(&source).unwrap_or_else(|e| panic!("{target}: {e:?}"));
        let ir = check::check_library(&ast).unwrap_or_else(|e| panic!("{target}: {e:?}"));
        let formatted = format::format(&source).unwrap();
        assert_eq!(formatted, format::format(&formatted).unwrap());
        let again = check::check_library(&parse::parse(&formatted).unwrap()).unwrap();
        assert_eq!(
            lowering::emit(&ir).unwrap(),
            lowering::emit(&again).unwrap()
        );
    }
}

#[test]
fn new_syntax_never_creates_runtime_type_values_or_weakens_codec_requirements() {
    for source in [
        read("Int | Float"),
        read("List(Int) | List(String)"),
        read("Map(Int,Int | String)"),
        read("Int | Result(Int,String)"),
        read("42 | String"),
        read("compute() | String"),
        "fn f(value:Int):value\nfn main():f(Int | String)\n".into(),
        "fn f(value:Int):value\nfn main():\n    let json=1\n    json.decode(\"1\",Int | String)\n"
            .into(),
        "fn f(value:Int):value\nfn main():\n    let call=f\n    call(Int | String)\n".into(),
    ] {
        assert!(
            parse::parse(&source)
                .and_then(|p| check::check_library(&p))
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn normal_value_grammar_does_not_gain_type_targets() {
    for source in [
        "fn identity(value:Int):value\nfn main():identity(1 ||| 2)\n",
        "type Row:\n    value:Int\nfn identity(value:Row):value\nfn main():identity(%{Row(1)|value:2})\n",
        "fn use(value:(Int)->Int):value(1)\nfn main():use((x)->x+1)\n",
        "fn identity(value:Int):value\nfn main():1 |> identity(_)\n",
    ] {
        let ast = parse::parse(source).unwrap();
        assert!(!std::format!("{ast:?}").contains("TypeTarget"));
        check::check_library(&ast).unwrap();
    }
}

#[test]
fn interactive_decoder_uses_actual_inline_union_plans_and_input_pipes() {
    let mut session = Session::default();
    for (target, text, expected) in [
        ("Int | String", "42", "42\n"),
        ("List(Int | String)", "[1,\\\"leaf\\\"]", "[1,\"leaf\"]\n"),
        ("(Int | String,Bool)", "[1,true]", "[1,true]\n"),
        ("Option(Int | String)", "null", "null\n"),
    ] {
        let source = std::format!(
            "match \"{text}\" |> json.decode({target}):\n    Ok(value)->\n        match json.encode(value):\n            Ok(text)->println(text)\n            Err(error)->println(json.error_code(error))\n    Err(error)->println(json.error_code(error))"
        );
        assert_eq!(session.evaluate(&source).unwrap(), expected);
    }
}

#[test]
fn source_ranges_and_type_limits_remain_exact() {
    let source = read("List(Int | String)");
    let program = parse::parse(&source).unwrap();
    let ast::ExprKind::Call { args, .. } = &program.functions[0].body.kind else {
        panic!("call")
    };
    assert!(matches!(
        &args[1].value.kind,
        ast::ExprKind::TypeTarget(Type::List(_))
    ));
    assert_eq!(
        &source[args[1].span.start..args[1].span.end],
        "List(Int | String)"
    );
    for target in [
        format!("{}Int | String{}", "List(".repeat(130), ")".repeat(130)),
        vec!["Int"; 129].join(" | "),
    ] {
        assert!(parse::parse(&read(&target)).is_err());
    }
}

#[test]
fn native_sources_preserve_generic_requirements_and_interactive_output() {
    for (source, expected) in [
        (
            include_str!("json_unions_native/inline_scalar.fn"),
            include_str!("json_unions_native/inline_scalar.stdout"),
        ),
        (
            include_str!("json_unions_native/inline_containers.fn"),
            include_str!("json_unions_native/inline_containers.stdout"),
        ),
        (
            include_str!("json_unions_native/inline_generic.fn"),
            include_str!("json_unions_native/inline_generic.stdout"),
        ),
    ] {
        let ast = parse::parse(source).unwrap();
        let ir = check::check(&ast).unwrap();
        assert!(lowering::emit(&ir).unwrap().contains("fern_json"));
        let mut session = Session::default();
        session
            .evaluate(&source.replace("fn main(", "fn codec_case("))
            .unwrap();
        assert_eq!(session.evaluate("match codec_case():\n    Ok(()) -> ()\n    Err(error) -> println(json.error_code(error))").unwrap(),expected);
        let formatted = format::format(source).unwrap();
        check::check(&parse::parse(&formatted).unwrap()).unwrap();
    }
}

#[test]
fn nested_codec_calls_remain_executable_arguments_and_lambda_bodies() {
    let source = "fn identity(value):value\nfn main()->Result(Unit,json.Error):\n    let text=\"7\"\n    let value=identity(json.decode(text,Int | String))?\n    println(json.encode(value)?)\n    let results=List.map([text],(word)->json.decode(word,Int | String))\n    for result in results:\n        match result:\n            Ok(value)->\n                match json.encode(value):\n                    Ok(text)->println(text)\n                    Err(error)->println(json.error_code(error))\n            Err(error)->println(json.error_code(error))\n    Ok(())\n";
    check::check(&parse::parse(source).unwrap()).unwrap();
    let mut session = Session::default();
    session
        .evaluate(&source.replace("fn main(", "fn codec_case("))
        .unwrap();
    assert_eq!(session.evaluate("match codec_case():\n    Ok(()) -> ()\n    Err(error) -> println(json.error_code(error))").unwrap(),"7\n7\n");
}

#[test]
fn large_ordinary_constructor_trees_preserve_syntax_and_formatter_acceptance() {
    let mut tree = "Empty".to_owned();
    for _ in 0..13 {
        tree = format!("Branch({tree},{tree})");
    }
    for extra in [
        "",
        "type Choice=Int | String\n",
        "fn decode(text:String):json.decode(text,Int | String)\n",
    ] {
        let source = format!(
            "type Node:\n    Empty\n    Branch(Node,Node)\n{extra}fn main():\n    let value={tree}\n    ()\n"
        );
        assert!(source.len() > 100_000 && source.len() < 120_000);
        parse::parse(&source).unwrap();
        let formatted = format::format(&source).unwrap();
        assert_eq!(formatted, format::format(&formatted).unwrap());
    }
}

#[test]
fn uppercase_callable_aliases_never_establish_a_static_codec_slot() {
    let source = "fn identity(value):value\nfn main()->Result(Unit,json.Error):\n    let Identity=identity\n    let word=\"7\"\n    let result=Identity(json.decode(word,Int | String))?\n    println(json.encode(result)?)\n    let Decode=(text:String)->json.decode(text,Int | String)\n    let nested=Identity(Decode(word))?\n    println(json.encode(nested)?)\n    Ok(())\n";
    check::check(&parse::parse(source).unwrap()).unwrap();
    let mut session = Session::default();
    session
        .evaluate(&source.replace("fn main(", "fn codec_case("))
        .unwrap();
    assert_eq!(session.evaluate("match codec_case():\n    Ok(()) -> ()\n    Err(error) -> println(json.error_code(error))").unwrap(),"7\n7\n");
    let fake = "fn fake(text:String,value:Int):value\nfn main():\n    let Decode=fake\n    Decode(\"7\",Int | String)\n";
    assert!(check::check(&parse::parse(fake).unwrap()).is_err());
}
