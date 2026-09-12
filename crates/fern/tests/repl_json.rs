//! JSON REPL parity must preserve the existing native specification oracles.
use fern_compiler::repl::Session;

const CASES: &[(&str, &str)] = &[
    (
        include_str!("json_values/valid/core.fn"),
        r###"42
true
2
{"count":42,"empty":null}
"###,
    ),
    (
        include_str!("json_values/valid/builders.fn"),
        r###"[null,true,-7,1.5,"🌿\n",1.00e2]
{"second":[null,true,-7,1.5,"🌿\n",1.00e2],"first":1}
[]
{}
"###,
    ),
    (
        include_str!("json_values/valid/members.fn"),
        r###""a\u0000b"
1
"🌿"
2
false
{"a\u0000b":1,"🌿":2}
5
0
"###,
    ),
    (
        include_str!("json_values/valid/errors.fn"),
        r###"1
3
invalid JSON syntax
6
-1
JSON object key not found
7
-1
JSON array index out of bounds
11
-1
JSON number is not finite
"###,
    ),
    (
        include_str!("json_values/valid/numeric.fn"),
        r###"9007199254740993
9007199254740993
1.5
-9223372036854775808
1e999999999999999999999999
true
"###,
    ),
    (
        include_str!("json_values/valid/first_class.fn"),
        r###"[1,2,3]
2.5
false
build once
{"x":1}
"###,
    ),
    (
        include_str!("json_values/valid/containers.fn"),
        r###"{"retained":true}
1
1
"###,
    ),
    (
        include_str!("json_values/valid/effects.fn"),
        r###"read done
run done
1
"###,
    ),
    (
        include_str!("json_values/valid/aliases.fn"),
        r###"[]
7
ordinary
9
"###,
    ),
    (
        include_str!("json_values/valid/dag_limits.fn"),
        r###"4
-1
"###,
    ),
];

#[test]
fn all_native_json_programs_have_the_same_interactive_output() {
    for &(source, expected) in CASES {
        let mut session = Session::default();
        session
            .evaluate(&source.replace("fn main(", "fn json_case("))
            .unwrap();
        let call = if source.contains("fn main() -> Result") {
            "match json_case():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_message(error))"
        } else {
            "json_case()"
        };
        assert_eq!(session.evaluate(call).unwrap(), expected, "{source}");
    }
}

#[test]
fn retained_json_aliases_wrappers_and_closures_survive_later_entries() {
    let mut session = Session::default();
    session
        .evaluate("let value = json.from_int(9007199254740993)")
        .unwrap();
    session.evaluate("let alias = value").unwrap();
    session.evaluate("let encode = Json.stringify").unwrap();
    session
        .evaluate("let capture = () -> encode(alias)")
        .unwrap();
    session
        .evaluate("fn unrelated(x: Int) -> Int: x + 1")
        .unwrap();
    assert_eq!(
        session
            .evaluate("match capture():\n    Ok(text) -> println(text)\n    Err(_) -> println(0)")
            .unwrap(),
        "9007199254740993\n"
    );
}

#[test]
fn numeric_failure_does_not_commit_new_json_bindings_or_replace_old_ones() {
    let mut session = Session::default();
    session.evaluate("let old = json.from_int(7)").unwrap();
    session
        .evaluate(
            "fn fail() -> json.Value:\n    let parsed = json.from_int(8)\n    1 / 0\n    parsed",
        )
        .unwrap();
    let error = session.evaluate("let fresh = fail()").unwrap_err();
    assert!(error.contains("integer division by zero"), "{error}");
    assert!(session.evaluate("fresh").is_err());
    assert_eq!(
        session
            .evaluate("match json.as_int(old):\n    Ok(n) -> println(n)\n    Err(_) -> println(0)")
            .unwrap(),
        "7\n"
    );
}

#[test]
fn opaque_json_previews_do_not_implicitly_serialize_contents() {
    let mut session = Session::default();
    assert_eq!(
        session.evaluate("json.null()").unwrap(),
        "<json.Value> : json.Value\n"
    );
}

#[test]
fn shared_json_graph_is_counted_once_but_independent_graphs_are_bounded() {
    let mut session = Session::default();
    session.evaluate("fn text() -> json.Value: Result.unwrap_or(json.from_string(String.repeat(\"x\", 600000)), json.null())").unwrap();
    session.evaluate("let original = text()").unwrap();
    for index in 0..40 {
        session
            .evaluate(&format!("let alias{index} = original"))
            .unwrap();
    }
    let mut rejected = false;
    for index in 0..40 {
        match session.evaluate(&format!("let independent{index} = text()")) {
            Ok(_) => {}
            Err(error) => {
                assert!(error.contains("interactive value storage limit"), "{error}");
                assert!(session.evaluate(&format!("independent{index}")).is_err());
                rejected = true;
                break;
            }
        }
    }
    assert!(rejected, "independent JSON storage was unbounded");
    assert_eq!(
        session.evaluate("json.is_null(original)").unwrap(),
        "false : Bool\n"
    );
}

#[test]
fn syntax_unicode_and_duplicate_offsets_match_native_byte_locations() {
    let mut session = Session::default();
    session.evaluate("fn probe(text: String):\n    match json.parse(text):\n        Ok(_) -> println(-1)\n        Err(error) ->\n            println(json.error_code(error))\n            println(json.error_offset(error))").unwrap();
    for (text, code, offset) in [
        ("", 1, 0),
        ("[1,]", 1, 3),
        ("1e+", 1, 3),
        ("\"\\uD800\"", 2, 1),
        ("\"\\uD800\\u0041\"", 2, 1),
        ("{\"a\":1,\"\\u0061\":2}", 3, 7),
        ("{\"🌿\":1,\"\\ud83c\\udf3f\":2}", 3, 10),
    ] {
        let quoted = format!("{text:?}").replace('{', "\\{").replace('}', "\\}");
        assert_eq!(
            session.evaluate(&format!("probe({quoted})")).unwrap(),
            format!("{code}\n{offset}\n")
        );
    }
}
