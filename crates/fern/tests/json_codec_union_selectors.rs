//! Decision103 J6b independent acceptance oracles for shallow union selection.
use fern_compiler::{check, parse, repl::Session};
fn session(types: &str) -> Session {
    let mut session = Session::default();
    session.evaluate(types).unwrap();
    session
}
fn literal(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('{', "\\{")
            .replace('}', "\\}")
    )
}
fn decode(session: &mut Session, text: &str, target: &str) -> String {
    session.evaluate(&format!("match json.decode({}, {target}):\n    Ok(value) ->\n        match json.encode(value):\n            Ok(text) -> println(text)\n            Err(error) -> println(json.error_code(error))\n    Err(error) ->\n        println(json.error_code(error))\n        println(json.error_offset(error))\n        println(json.error_path(error))",literal(text))).unwrap()
}
fn rejects(source: &str, needle: &str) {
    let ast = parse::parse(source).unwrap();
    let failure = check::check(&ast).unwrap_err();
    assert!(failure.message.contains(needle), "{source}\n{failure:?}");
}
const MIXED: &str = "type State derive(Json):\n    Ready\n    Count(Int)\ntype Extended derive(Json):\n    tag:String\n    fields:List(Int)\n    extra:Bool\n";
#[test]
fn mixed_sum_record_uses_strict_keys_before_any_matching_sum_tag() {
    for order in ["State | Extended", "Extended | State"] {
        let mut s = session(&format!("{MIXED}type Choice={order}"));
        for (text, expected) in [
            (
                r#"{"tag":"Ready","fields":[],"extra":true}"#,
                "{\"tag\":\"Ready\",\"fields\":[],\"extra\":true}\n",
            ),
            (
                r#"{"tag":"Ready","fields":[]}"#,
                "{\"tag\":\"Ready\",\"fields\":[]}\n",
            ),
            (
                r#"{"tag":"Count","fields":[42]}"#,
                "{\"tag\":\"Count\",\"fields\":[42]}\n",
            ),
            (r#"{"tag":"Count","fields":[1.5]}"#, "9\n-1\n/fields/0\n"),
            (r#"{"tag":"Other","fields":[]}"#, "13\n-1\n/tag\n"),
            (r#"{"tag":"Ready"}"#, "14\n-1\n\n"),
            (
                r#"{"tag":"Ready","fields":[],"unexpected":1}"#,
                "14\n-1\n\n",
            ),
            (
                r#"{"tag":"Ready","fields":[],"extra":0}"#,
                "5\n-1\n/extra\n",
            ),
        ] {
            assert_eq!(decode(&mut s, text, "Choice"), expected, "{order}: {text}");
        }
    }
}
#[test]
fn disjoint_sum_tags_select_before_specific_envelope_error_reporting() {
    for order in ["Left | Right", "Right | Left"] {
        let mut s = session(&format!(
            "type Left derive(Json):\n    Ready\n    Count(Int)\ntype Right derive(Json):\n    Stop\n    Text(String)\ntype Choice={order}"
        ));
        for (text, expected) in [
            (r#"{"tag":"Count","fields":[1.5]}"#, "9\n-1\n/fields/0\n"),
            (
                r#"{"tag":"Count","fields":[],"extra":0}"#,
                "12\n-1\n/extra\n",
            ),
            (r#"{"tag":"Count"}"#, "6\n-1\n/fields\n"),
            (r#"{"tag":"Unknown","fields":[]}"#, "14\n-1\n\n"),
            (r#"{"fields":[]}"#, "14\n-1\n\n"),
        ] {
            assert_eq!(decode(&mut s, text, "Choice"), expected, "{order}: {text}");
        }
    }
}
#[test]
fn unique_json_kind_preserves_original_numeric_and_text_failures() {
    let mut s = session("type Choice=Int | String");
    for (text, expected) in [
        ("1.0", "1\n"),
        ("1e0", "1\n"),
        ("9007199254740993", "9007199254740993\n"),
        ("1.5", "9\n-1\n\n"),
        ("9223372036854775808", "8\n-1\n\n"),
        (r#""a\u0000b""#, "10\n-1\n\n"),
        ("true", "14\n-1\n\n"),
    ] {
        assert_eq!(decode(&mut s, text, "Choice"), expected, "{text}");
    }
    let mut s = session("type Numeric=Float | Bool");
    assert_eq!(decode(&mut s, "-0.0", "Numeric"), "-0\n");
    assert_eq!(decode(&mut s, "1e9999", "Numeric"), "8\n-1\n\n");
}
#[test]
fn strict_record_keysets_are_conservative_and_definition_order_independent() {
    let a = "type A derive(Json):\n    id:Int\n    note:Option(String)\n";
    let b = "type B derive(Json):\n    title:String\n";
    for decls in [format!("{a}{b}"), format!("{b}{a}")] {
        let mut s = session(&format!("{decls}type Choice=A | B"));
        assert_eq!(
            decode(&mut s, r#"{"id":3}"#, "Choice"),
            "{\"id\":3,\"note\":null}\n"
        );
        assert_eq!(
            decode(&mut s, r#"{"title":"x"}"#, "Choice"),
            "{\"title\":\"x\"}\n"
        );
        for text in ["{}", r#"{"id":3,"title":"x"}"#, r#"{"id":3,"other":0}"#] {
            assert_eq!(decode(&mut s, text, "Choice"), "14\n-1\n\n");
        }
    }
    for order in ["A | B", "B | A"] {
        let s = format!(
            "type A derive(Json):\n    value:Int\ntype B derive(Json):\n    value:Float\ntype Choice={order}\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n"
        );
        rejects(&s, "not provably disjoint");
    }
}
#[test]
fn generic_decoders_keep_whole_union_requirements_and_normalized_duplicate_arms() {
    let source = "type Choice(a)=a | String\nfn read(text:String, witness:a)->Result(Choice(a),json.Error):json.decode(text,Choice(a))\nfn main()->Result(Unit,json.Error):\n    let numbers:(String,Int)->Result(Int | String,json.Error)=read\n    let words:(String,String)->Result(String,json.Error)=read\n    println(json.encode(numbers(\"7\",0)?)?)\n    println(words(\"\\\"word\\\"\",\"\")?)\n    Ok(())\n";
    check::check(&parse::parse(source).unwrap()).unwrap();
    let mut s = session(&source.replace("fn main(", "fn example("));
    assert_eq!(
        s.evaluate(
            "match example():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_code(error))"
        )
        .unwrap(),
        "7\nword\n"
    );
    for ty in ["a | Int | Float", "Float | a | Int"] {
        rejects(
            &format!("fn bad(value:{ty}):json.encode(value)\nfn main():()\n"),
            "not provably disjoint",
        );
    }
}
#[test]
fn nullability_and_nominal_wrappers_never_create_order_based_fallbacks() {
    for ty in [
        "Unit | Option(Int)",
        "Option(Int) | Int",
        "Int | Float",
        "List(Int) | List(String)",
    ] {
        rejects(
            &format!(
                "type Choice={ty}\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n"
            ),
            "not provably disjoint",
        );
    }
    rejects(
        "newtype Text derive(Json)=Text(String)\nfn write(value:a | String):json.encode(value)\nfn main():\n    match write(Text(\"x\")):\n        Ok(text) -> println(text)\n        Err(error) -> println(json.error_code(error))\n",
        "not provably disjoint",
    );
}
#[test]
fn phantom_result_metadata_is_distinct_from_actual_stored_union_payloads() {
    let source = "type Token(a) derive(Json):\n    Done\ntype Choice=Token(Result(Int,String)) | String\nfn read()->Result(Choice,json.Error):json.decode(\"\\{\\\"tag\\\":\\\"Done\\\",\\\"fields\\\":[]\\}\",Choice)\nfn main():()\n";
    check::check(&parse::parse(source).unwrap()).unwrap();
    rejects(
        "type Token(a) derive(Json):\n    Stored(a)\ntype Choice=Token(Result(Int,String)) | String\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n",
        "Result",
    );
}

#[test]
fn union_targets_accept_aliases_and_inline_types_without_runtime_type_values() {
    let accepted = "type Choice(a)=a | String\nfn read()->Result(Choice(Int),json.Error):json.decode(\"7\",Choice(Int))\nfn main():()\n";
    check::check(&parse::parse(accepted).unwrap()).unwrap();
    let inline = "fn read()->Result(Int | String,json.Error):json.decode(\"7\",Int | String)\nfn main():()\n";
    check::check(&parse::parse(inline).unwrap()).unwrap();
    rejects(
        "fn accept(value:Int):value\nfn main():accept(Int | String)\n",
        "cannot be used as a value",
    );
}
#[test]
fn map_overlap_and_nullable_union_payloads_stay_rejected() {
    rejects(
        "type Row derive(Json):\n    id:Int\ntype Choice=Map(String,Int) | Row\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n",
        "not provably disjoint",
    );
    rejects(
        "type Choice=Option(Unit | String)\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n",
        "can encode null",
    );
}
#[test]
fn recursive_union_records_retain_whole_generic_wire_requirements() {
    let declarations =
        "type Node(a) derive(Json):\n    next:Node(a) | a\ntype Choice=Node(String) | String\n";
    let mut s = session(declarations);
    assert_eq!(
        decode(&mut s, r#"{"next":{"next":"done"}}"#, "Choice"),
        "{\"next\":{\"next\":\"done\"}}\n"
    );
    rejects(
        "type Node(a) derive(Json):\n    next:Node(a) | a\ntype Choice=Node(Map(String,Int))\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n",
        "not provably disjoint",
    );
}
#[test]
fn independent_generic_parameters_collapse_before_whole_union_discharge() {
    for signature in ["value:a | b,left:a,right:b", "left:a,right:b,value:a | b"] {
        let args = if signature.starts_with("value") {
            "\"word\",\"a\",\"b\""
        } else {
            "\"a\",\"b\",\"word\""
        };
        let src = format!(
            "fn write({signature})->Result(String,json.Error):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    let callback:(String,String,String)->Result(String,json.Error)=write\n    println(callback({args})?)\n    Ok(())\n"
        );
        check::check(&parse::parse(&src).unwrap()).unwrap();
        let mut s = session(&src.replace("fn main(", "fn example("));
        assert_eq!(s.evaluate("match example():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_code(error))").unwrap(),"\"word\"\n");
    }
}
#[test]
fn selector_failures_retain_the_parent_path_before_allocating_any_payload() {
    let mut s = session("type Choice=Int | String\ntype Outer derive(Json):\n    choice:Choice\n");
    assert_eq!(
        decode(&mut s, r#"{"choice":true}"#, "Outer"),
        "14\n-1\n/choice\n"
    );
}

#[test]
fn nullable_member_is_valid_when_every_other_member_excludes_null() {
    let mut s = session("type Choice=Option(Int) | String");
    assert_eq!(decode(&mut s, "null", "Choice"), "null\n");
    assert_eq!(decode(&mut s, "42", "Choice"), "42\n");
    assert_eq!(decode(&mut s, "\"word\"", "Choice"), "\"word\"\n");
}

#[test]
fn symbolic_maps_inside_union_profiles_retain_exact_string_key_constraints() {
    let accepted = "fn write(value:Map(k,v) | String,key:k,item:v)->Result(String,json.Error):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    let callback:(Map(String,Int) | String,String,Int)->Result(String,json.Error)=write\n    println(callback(%{\"a\":7},\"\",0)?)\n    Ok(())\n";
    check::check(&parse::parse(accepted).unwrap()).unwrap();
    let mut s = session(&accepted.replace("fn main(", "fn example("));
    assert_eq!(
        s.evaluate(
            "match example():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_code(error))"
        )
        .unwrap(),
        "{\"a\":7}\n"
    );
    let invalid = accepted
        .replace(
            "Map(String,Int) | String,String,Int",
            "Map(Int,Int) | String,Int,Int",
        )
        .replace("%{\"a\":7},\"\",0", "%{1:7},0,0");
    rejects(&invalid, "keys must have type String");
}
#[test]
fn source_map_key_domain_is_not_broadened_by_json_union_support() {
    let accepted = "type Keys(a)=a | String\nfn write(value:Map(Keys(a),Int),anchor:a)->Result(String,json.Error):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    let callback:(Map(String,Int),String)->Result(String,json.Error)=write\n    println(callback(%{\"a\":7},\"\")?)\n    Ok(())\n";
    rejects(accepted, "map key must be Int, Bool, or String");
}

#[test]
fn record_discrimination_uses_actual_option_fields_not_nullable_newtype_representation() {
    let mut s = session(
        "newtype Maybe derive(Json)=Maybe(Option(Int))\ntype A derive(Json):\n    value:Maybe\ntype B derive(Json):\n    other:Option(String)\ntype Choice=A | B",
    );
    assert_eq!(decode(&mut s, "{}", "Choice"), "{\"other\":null}\n");
    assert_eq!(
        decode(&mut s, r#"{"value":null}"#, "Choice"),
        "{\"value\":null}\n"
    );
    assert_eq!(
        decode(&mut s, r#"{"value":null,"other":null}"#, "Choice"),
        "14\n-1\n\n"
    );
}
