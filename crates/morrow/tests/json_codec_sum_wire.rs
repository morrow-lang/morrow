//! Decision103 sum wire acceptance contract; union continuation stays separately red.
use morrow_compiler::{check, parse, repl::Session};
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
    let source = include_str!("json_sums_native/sums.fn");
    accepts(source);
    roundtrip(source, include_str!("json_sums_native/sums.stdout"));
}
#[test]
fn recursive_wire_contract() {
    let source = include_str!("json_sums_native/recursive.fn");
    accepts(source);
    roundtrip(source, include_str!("json_sums_native/recursive.stdout"));
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
        (r#"{"tag":9}"#, "6\n-1\n/fields\n"),
        (r#"{"tag":9,"fields":[]}"#, "5\n-1\n/tag\n"),
        (
            r#"{"tag":"Missing","fields":[],"z/~/x":0}"#,
            "12\n-1\n/z~1~0~1x\n",
        ),
        (r#"{"b":0,"a":0}"#, "12\n-1\n/b\n"),
        (
            r#"{"tag":"Count","fields":[9223372036854775808]}"#,
            "8\n-1\n/fields/0\n",
        ),
        (r#"{"tag":"Count","fields":false}"#, "5\n-1\n/fields\n"),
        (r#"[]"#, "5\n-1\n\n"),
    ] {
        assert_eq!(error_details(&mut session, text, "State"), expected);
    }
}
#[test]
fn module_aliases_do_not_become_wire_tag_prefixes() {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "morrow-j6-tags-{}-{}",
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
    let loaded = morrow_compiler::modules::load(&directory.join("main.fn")).unwrap();
    let checked = check::check(&loaded.program);
    fs::remove_dir_all(directory).unwrap();
    let checked = checked.unwrap();
    let output = morrow_compiler::lowering::emit(&checked).unwrap();
    assert!(output.contains("Count"));
    // Native exact bytes are separately prescribed by the same constructor-only envelope.
}

#[test]
fn constructor_spelling_preserves_existing_lowercase_and_unicode_tokens() {
    for constructor in ["lowercase", "État", "状态", "🟢"] {
        let source = format!(
            "type Value derive(Json):\n    {constructor}(Int)\nfn main()->Result(Unit,json.Error):\n    println(json.encode({constructor}(7))?)\n    Ok(())\n"
        );
        roundtrip(
            &source,
            &format!("{{\"tag\":\"{constructor}\",\"fields\":[7]}}\n"),
        );
    }
}

#[test]
fn sums_keep_full64_payloads_generic_cycles_and_source_effect_order() {
    for (source, expected) in [
        (
            include_str!("json_sums_native/full64.fn"),
            include_str!("json_sums_native/full64.stdout"),
        ),
        (
            include_str!("json_sums_native/mutual.fn"),
            include_str!("json_sums_native/mutual.stdout"),
        ),
    ] {
        accepts(source);
        roundtrip(source, expected);
    }
    let source = include_str!("json_sums_native/effects.fn");
    accepts(source);
    let mut session = Session::default();
    session
        .evaluate(&source.replace("fn main(", "fn codec_case("))
        .unwrap();
    assert_eq!(
        session.evaluate("codec_case()").unwrap(),
        include_str!("json_sums_native/effects.stdout")
    );
}
