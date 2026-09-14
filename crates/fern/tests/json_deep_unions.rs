//! Independent structural discrimination oracles, without trial decoding or arm priority.
use fern_compiler::{check, parse, repl::Session};

fn literal(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('{', "\\{")
            .replace('}', "\\}")
    )
}
fn roundtrip(session: &mut Session, text: &str) -> String {
    session.evaluate(&format!("match json.decode({}, Choice):\n    Ok(value) ->\n        match json.encode(value):\n            Ok(text) -> println(text)\n            Err(error) -> println(json.error_code(error))\n    Err(error) -> println(json.error_code(error))", literal(text))).unwrap()
}
#[test]
fn nested_record_discrimination_is_independent_of_declaration_and_union_order() {
    let a = "type LeftPayload derive(Json):\n    value:Int\ntype Left derive(Json):\n    payload:LeftPayload\n";
    let b = "type RightPayload derive(Json):\n    value:String\ntype Right derive(Json):\n    payload:RightPayload\n";
    for declarations in [format!("{a}{b}"), format!("{b}{a}")] {
        for union in ["Left | Right", "Right | Left"] {
            let mut session = Session::default();
            session
                .evaluate(&format!("{declarations}type Choice={union}"))
                .unwrap();
            for text in [
                r#"{"payload":{"value":9007199254740993}}"#,
                r#"{"payload":{"value":"🌿"}}"#,
            ] {
                assert_eq!(roundtrip(&mut session, text), format!("{text}\n"));
            }
            assert_eq!(
                roundtrip(&mut session, r#"{"payload":{"value":true}}"#),
                "14\n"
            );
        }
    }
}
#[test]
fn tuples_use_element_shapes_and_preserve_full_width_payloads() {
    let mut session = Session::default();
    session
        .evaluate("type Choice=(Int,String) | (String,Int)")
        .unwrap();
    for text in [
        r#"[9223372036854775807,"x"]"#,
        r#"["x",-9223372036854775808]"#,
    ] {
        assert_eq!(roundtrip(&mut session, text), format!("{text}\n"));
    }
    assert_eq!(roundtrip(&mut session, "[true,false]"), "14\n");
}
#[test]
fn seeded_nested_union_roundtrips_use_an_independent_wire_oracle() {
    let mut session = Session::default();
    session.evaluate("type Number derive(Json):\n    value:(Int,Bool)\ntype Text derive(Json):\n    value:(String,Bool)\ntype Choice=Number | Text").unwrap();
    let mut seed = 0x4645_524e_u64;
    for _ in 0..128 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let value = if seed & 1 == 0 {
            (seed as i64).to_string()
        } else {
            format!("\"🌿{}\"", seed >> 32)
        };
        let text = format!("{{\"value\":[{value},{}]}}", seed & 2 != 0);
        assert_eq!(roundtrip(&mut session, &text), format!("{text}\n"));
    }
}
#[test]
fn optional_absence_numeric_overlap_and_empty_lists_remain_ambiguous() {
    for declarations in [
        "type A derive(Json):\n    value:Option(Int)\ntype B derive(Json):\n    value:Option(String)\ntype Choice=A | B",
        "type A derive(Json):\n    value:Int\ntype B derive(Json):\n    value:Float\ntype Choice=A | B",
        "type Choice=List(Int) | List(String)",
    ] {
        let source = format!(
            "{declarations}\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n"
        );
        let error = check::check(&parse::parse(&source).unwrap()).unwrap_err();
        assert!(error.message.contains("not provably disjoint"), "{error:?}");
    }
}

#[test]
fn recursive_records_require_a_finite_discriminator_and_optional_fields_stay_optional() {
    let mut session = Session::default();
    session.evaluate("type A derive(Json):\n    next:Option(A)\n    value:Int\ntype B derive(Json):\n    next:Option(B)\n    value:String\ntype Choice=A | B").unwrap();
    for text in [
        r#"{"next":{"next":null,"value":42},"value":7}"#,
        r#"{"next":{"next":null,"value":"x"},"value":"y"}"#,
    ] {
        assert_eq!(roundtrip(&mut session, text), format!("{text}\n"));
    }
    let mut session = Session::default();
    session.evaluate("type A derive(Json):\n    value:Int\ntype B derive(Json):\n    value:Option(String)\ntype Choice=A | B").unwrap();
    assert_eq!(roundtrip(&mut session, "{}"), "{\"value\":null}\n");
    assert_eq!(roundtrip(&mut session, r#"{"value":7}"#), "{\"value\":7}\n");
    assert_eq!(roundtrip(&mut session, r#"{"value":1.5}"#), "9\n");
}

#[test]
fn shared_sum_tags_use_payload_shapes_without_trial_decoding() {
    for union in ["Event(Int) | Event(String)", "Event(String) | Event(Int)"] {
        let mut session = Session::default();
        session
            .evaluate(&format!(
                "type Event(a) derive(Json):\n    Payload(a)\ntype Choice={union}"
            ))
            .unwrap();
        for text in [
            r#"{"tag":"Payload","fields":[9007199254740993]}"#,
            r#"{"tag":"Payload","fields":["🌿"]}"#,
        ] {
            assert_eq!(roundtrip(&mut session, text), format!("{text}\n"));
        }
        assert_eq!(
            roundtrip(&mut session, r#"{"tag":"Payload","fields":[false]}"#),
            "14\n"
        );
        assert_eq!(
            roundtrip(&mut session, r#"{"tag":"Payload","fields":[1.5]}"#),
            "9\n"
        );
    }
    let source = "type Event(a) derive(Json):\n    Empty\n    Payload(a)\ntype Choice=Event(Int) | Event(String)\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n";
    assert!(
        check::check(&parse::parse(source).unwrap())
            .unwrap_err()
            .message
            .contains("not provably disjoint")
    );
}
