use morrow_compiler::{check, parse};
fn accepts(source: &str) {
    check::check(&parse::parse(source).unwrap()).unwrap_or_else(|e| panic!("{source}: {e:?}"));
}
#[test]
fn inferred_encode_scheme_specializes_independently() {
    accepts(
        "fn write(value):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    println(write(1)?)\n    println(write(\"Morrow\")?)\n    Ok(())\n",
    );
}
#[test]
fn generic_decoder_target_uses_return_context_and_module_free_type_identity() {
    accepts(
        "fn read(text:String)->Result(a,json.Error):json.decode(text,a)\nfn main()->Result(Unit,json.Error):\n    let number:Int=read(\"42\")?\n    let text:String=read(\"\\\"Morrow\\\"\")?\n    println(number)\n    println(text)\n    Ok(())\n",
    );
}
#[test]
fn conditional_option_record_and_function_value_requirements_remain_sound() {
    accepts(
        "type Box(a) derive(Json):\n    value:a\nfn write(value:Option(Box(a))):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    let emit:(Option(Box(Int)))->Result(String,json.Error)=write\n    println(emit(Some(Box(1)))?)\n    println(emit(None)?)\n    Ok(())\n",
    );
}
#[test]
fn invalid_components_and_null_collisions_fail_without_a_witness() {
    for source in [
        "fn bad(value:a):json.encode((value,(x:Int)->x))\nfn main():()\n",
        "fn write(value:Option(a)):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    println(write(Some(()))?)\n    Ok(())\n",
        "fn produced()->Result(Int,String):Ok(1)\nfn write(value):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    println(write(produced())?)\n    Ok(())\n",
    ] {
        assert!(check::check(&parse::parse(source).unwrap()).is_err());
    }
}

#[test]
fn recursive_schemes_actual_fields_and_exact_keys() {
    accepts(
        "type Box(a) derive(Json):\n    value:a\nfn write(value):json.encode(value)\nfn relay(value):write(value)\nfn main()->Result(Unit,json.Error):\n    println(relay(Some(Box(())))?)\n    println(relay(%{\"x\":42})?)\n    Ok(())\n",
    );
}
#[test]
fn generic_map_key_and_newtype_null_predicates_reject_bad_instances() {
    for source in [
        "fn write(value:Map(k,v)):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    println(write(%{1:2})?)\n    Ok(())\n",
        "newtype Wrap(a) derive(Json)=Wrap(a)\nfn write(value:Option(Wrap(a))):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    println(write(Some(Wrap(())))?)\n    Ok(())\n",
    ] {
        let ast = parse::parse(source).unwrap();
        assert!(check::check(&ast).is_err());
    }
}

#[test]
fn template_metadata_publishes_constraints_but_never_template_ir() {
    let ast = parse::parse("fn write(value):json.encode(value)\nfn main():()\n").unwrap();
    let ir = check::check(&ast).unwrap();
    let facts = check::editor::analyze(
        &ast,
        check::editor::Query {
            occurrence: ast.functions[0].span,
            binding: None,
            function: Some("write".into()),
        },
    )
    .unwrap();
    assert!(!format!("{ir:?}").contains("JsonCodecTemplate"));
    assert!(format!("{facts:?}").contains("Json codec"));
}

#[test]
fn unused_templates_retain_codec_result_responsibility() {
    for source in [
        "fn ignored(value:a):\n    json.encode(value)\n    ()\nfn main():()\n",
        "fn read(text:String)->Result(a,json.Error):json.decode(text,a)\nfn main():\n    let value=read(\"42\")\n    ()\n",
        "fn wrong(value:a)->Result(String,json.Error):json.encode((value,Some(())))\nfn main():()\n",
    ] {
        assert!(
            check::check(&parse::parse(source).unwrap()).is_err(),
            "{source}"
        );
    }
}
#[test]
fn mutual_requirements_function_callbacks_and_phantom_types() {
    accepts(
        "type Phantom(a) derive(Json):\n    value:Int\nfn left(value:a,count:Int)->Result(String,json.Error):\n    if count>0:right(value,count-1) else:json.encode(value)\nfn right(value:a,count:Int)->Result(String,json.Error):left(value,count)\nfn write(value):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    println(right(42,1)?)\n    let values=List.map([1,2],write)\n    for value in values:\n        match value:\n            Ok(text)->println(text)\n            Err(error)->println(json.error_message(error))\n    let phantom:Phantom((Int)->Int)=Phantom(7)\n    println(write(phantom)?)\n    let responsibility:Phantom(Result(Int,String))=Phantom(8)\n    println(write(responsibility)?)\n    Ok(())\n",
    );
}
