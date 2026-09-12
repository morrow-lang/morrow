//! Proposed Decision103 acceptance contract; intentionally red on frozen J5c.
use fern_compiler::{check, parse, repl::Session};
fn accepts(source: &str) {
    check::check(&parse::parse(source).unwrap()).unwrap();
}
fn roundtrip(source: &str, expected: &str) {
    let mut session = Session::default();
    session
        .evaluate(&source.replace("fn main(", "fn codec_case("))
        .unwrap();
    let actual=session.evaluate("match codec_case():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_message(error))").unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn sums_wire_contract() {
    let source = include_str!("json_j6_proposal/sums.fn");
    accepts(source);
    roundtrip(source, include_str!("json_j6_proposal/sums.stdout"));
}
#[test]
fn recursive_wire_contract() {
    let source = include_str!("json_j6_proposal/recursive.fn");
    accepts(source);
    roundtrip(source, include_str!("json_j6_proposal/recursive.stdout"));
}
#[test]
fn scalar_union_wire_contract() {
    let source = include_str!("json_j6_proposal/scalar_union.fn");
    accepts(source);
    roundtrip(source, include_str!("json_j6_proposal/scalar_union.stdout"));
}
#[test]
fn float_union_wire_contract() {
    let source = include_str!("json_j6_proposal/float_union.fn");
    accepts(source);
    roundtrip(source, include_str!("json_j6_proposal/float_union.stdout"));
}
#[test]
fn record_union_wire_contract() {
    let source = include_str!("json_j6_proposal/record_union.fn");
    accepts(source);
    roundtrip(source, include_str!("json_j6_proposal/record_union.stdout"));
}
#[test]
fn array_union_wire_contract() {
    let source = include_str!("json_j6_proposal/array_union.fn");
    accepts(source);
    roundtrip(source, include_str!("json_j6_proposal/array_union.stdout"));
}
#[test]
fn sum_union_wire_contract() {
    let source = include_str!("json_j6_proposal/sum_union.fn");
    accepts(source);
    roundtrip(source, include_str!("json_j6_proposal/sum_union.stdout"));
}
#[test]
fn conditional_union_wire_contract() {
    let source = include_str!("json_j6_proposal/conditional_union.fn");
    accepts(source);
    roundtrip(
        source,
        include_str!("json_j6_proposal/conditional_union.stdout"),
    );
}

#[test]
fn ambiguous_union_domains_reject_without_definition_order_priority() {
    for (prefix, ty) in [
        ("", "Int | Float"),
        ("", "List(Int) | List(String)"),
        ("", "Option(String) | Unit"),
        ("", "Option(String) | String"),
        ("", "json.Value | Int"),
        ("newtype Id derive(Json)=Id(Int)\n", "Id | Int"),
        (
            "type A derive(Json):\n    value:Int\ntype B derive(Json):\n    value:String\n",
            "A | B",
        ),
        (
            "type A derive(Json):\n    left:Option(Int)\ntype B derive(Json):\n    right:Option(String)\n",
            "A | B",
        ),
    ] {
        let source = format!(
            "{prefix}type Choice={ty}\nfn read()->Result(Choice,json.Error):json.decode(\"null\",Choice)\nfn main():()\n"
        );
        let ast = parse::parse(&source).unwrap();
        let error = check::check(&ast).unwrap_err();
        assert!(
            error.message.contains("not provably disjoint"),
            "{source}: {error:?}"
        );
    }
}
#[test]
fn result_payload_in_an_unused_sum_variant_stays_forbidden() {
    let source = "type Event derive(Json):\n    Safe\n    Duty(Result(Int,String))\nfn main():()\n";
    assert!(check::check(&parse::parse(source).unwrap()).is_err());
}
#[test]
fn canonical_duplicate_unions_and_exact_decimal_primitives_stay_compatible() {
    accepts(
        "type Choice=Int | Int\nfn read()->Result(Choice,json.Error):json.decode(\"1.0\",Choice)\nfn main():()\n",
    );
    let mut session = Session::default();
    for text in ["1.0", "1e0", "1.00e+0"] {
        let source = format!(
            "match json.decode(\"{text}\",Int):\n    Ok(value) -> println(value)\n    Err(error) -> println(json.error_code(error))"
        );
        assert_eq!(session.evaluate(&source).unwrap(), "1\n");
    }
}
#[test]
fn sum_metadata_bounds_remain_checked_before_plan_construction() {
    let mut source = String::from("type TooMany derive(Json):\n");
    for index in 0..256 {
        source.push_str(&format!("    Case{index}\n"));
    }
    source.push_str("fn main():()\n");
    assert!(
        parse::parse(&source)
            .and_then(|p| check::check(&p))
            .is_err()
    );
}

#[test]
fn declaration_order_does_not_change_source_constructor_wire_tags() {
    for variants in ["    Ready\n    Count(Int)\n", "    Count(Int)\n    Ready\n"] {
        let source = format!(
            "type State derive(Json):\n{variants}fn main() -> Result(Unit,json.Error):\n    println(json.encode(Count(42))?)\n    Ok(())\n"
        );
        roundtrip(&source, "{\"tag\":\"Count\",\"fields\":[42]}\n");
    }
}
fn error_details(session: &mut Session, text: &str, target: &str) -> String {
    let text = text
        .replace('\\', "\\\\")
        .replace('\"', "\\\"")
        .replace('{', "\\{")
        .replace('}', "\\}");
    session.evaluate(&format!("match json.decode(\"{text}\",{target}):\n    Ok(_) -> println(0)\n    Err(error) ->\n        println(json.error_code(error))\n        println(json.error_offset(error))\n        println(json.error_path(error))")).unwrap()
}
#[test]
fn sum_envelope_failures_have_stable_codes_and_payload_paths() {
    let mut session = Session::default();
    session
        .evaluate("type State derive(Json):\n    Ready\n    Count(Int)")
        .unwrap();
    for (text, expected) in [
        (r#"{"tag":"Missing","fields":[]}"#, "13\n-1\n/tag\n"),
        (r#"{"fields":[]}"#, "6\n-1\n/tag\n"),
        (r#"{"tag":"Ready"}"#, "6\n-1\n/fields\n"),
        (
            r#"{"tag":"Ready","fields":[],"extra":0}"#,
            "12\n-1\n/extra\n",
        ),
        (r#"{"tag":"Count","fields":[1.5]}"#, "9\n-1\n/fields/0\n"),
        (r#"{"tag":"Ready","fields":[0]}"#, "5\n-1\n/fields\n"),
    ] {
        assert_eq!(error_details(&mut session, text, "State"), expected);
    }
}
#[test]
fn union_selection_does_not_change_numeric_adapter_errors_or_invent_a_candidate() {
    let mut session = Session::default();
    session.evaluate("type Choice=Int | String").unwrap();
    assert_eq!(error_details(&mut session, "1.5", "Choice"), "9\n-1\n\n");
    assert_eq!(
        error_details(&mut session, "9223372036854775808", "Choice"),
        "8\n-1\n\n"
    );
    assert_eq!(error_details(&mut session, "true", "Choice"), "14\n-1\n\n");
}

#[test]
fn module_aliases_do_not_become_wire_tag_prefixes() {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "fern-j6-tags-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("model.fn"),
        "pub type State derive(Json):\n    Ready\n    Count(Int)\n",
    )
    .unwrap();
    fs::write(directory.join("main.fn"),"import model as renamed\nfn main()->Result(Unit,json.Error):\n    println(json.encode(renamed.Count(42))?)\n    Ok(())\n").unwrap();
    let loaded = fern_compiler::modules::load(&directory.join("main.fn")).unwrap();
    let checked = check::check(&loaded.program);
    fs::remove_dir_all(directory).unwrap();
    let checked = checked.unwrap();
    let output = fern_compiler::lowering::emit(&checked).unwrap();
    assert!(output.contains("Count"));
    // Native exact bytes are separately prescribed by the same constructor-only envelope.
}
