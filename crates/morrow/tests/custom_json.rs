use morrow_compiler::{check, parse, repl::Session};
const CUSTOM: &str = r#"newtype UserId = UserId(Int)
impl Json(UserId):
    fn to_json(value: UserId) -> Result(json.Value, json.Error):
        json.from_string("user-{value.0}")
    fn from_json(value: json.Value) -> Result(UserId, json.Error):
        let text = json.as_string(value)?
        let number = json.parse(String.replace(text, "user-", ""))?
        Ok(UserId(json.as_int(number)?))
type Envelope derive(Json):
    ids: List(UserId)
"#;
#[test]
fn custom_json_runs_inside_derived_records_and_lists() {
    let mut session = Session::default();
    session.evaluate(CUSTOM).unwrap();
    assert_eq!(session.evaluate("match json.encode(Envelope([UserId(9007199254740993), UserId(-7)])):\n    Ok(text) -> text\n    Err(error) -> json.error_message(error)").unwrap(),"\"{\\\"ids\\\":[\\\"user-9007199254740993\\\",\\\"user--7\\\"]}\" : String\n");
    assert_eq!(
        session
            .evaluate(
                r#"match json.decode("\{\"ids\":[\"user-42\"]\}", Envelope):
    Ok(value) -> match value.ids:
        [id, .._] -> println(id.0)
        [] -> println(0)
    Err(_) -> println(-1)"#
            )
            .unwrap(),
        "42\n"
    );
}
#[test]
fn opaque_custom_json_cannot_claim_disjoint_or_non_null_wire_shapes() {
    for target in ["Option(UserId)", "UserId | Int"] {
        let source = format!(
            "{CUSTOM}\nfn decode(text: String) -> Result({target},json.Error):\n    json.decode(text, {target})\n"
        );
        assert!(check::check_library(&parse::parse(&source).unwrap()).is_err());
    }
}

#[test]
fn generic_custom_codecs_use_explicit_json_bounds_and_builtin_wire_values() {
    let source = r#"newtype Box(a) = Box(a)
impl Json(Box(a)) where Json(a):
    fn to_json(value: Box(a)) -> Result(json.Value,json.Error): to_json(value.0)
    fn from_json(value: json.Value) -> Result(Box(a),json.Error): Ok(Box(from_json(value)?))
fn read(text: String) -> Result(Box(Int),json.Error): json.decode(text,Box(Int))
"#;
    let mut session = Session::default();
    session.evaluate(source).unwrap();
    assert_eq!(session.evaluate("match read(\"9007199254740993\"):\n    Ok(value) -> println(value.0)\n    Err(error) -> println(json.error_code(error))").unwrap(),"9007199254740993\n");
    assert_eq!(session.evaluate("match json.encode(Box(42)):\n    Ok(text) -> println(text)\n    Err(error) -> println(json.error_code(error))").unwrap(),"42\n");
}

#[test]
fn custom_json_errors_keep_nested_paths_and_callback_faults_remain_faults() {
    let mut session = Session::default();
    session.evaluate(CUSTOM).unwrap();
    assert_eq!(
        session
            .evaluate(
                r#"match json.decode("\{\"ids\":[\"user-1\",2]\}",Envelope):
    Ok(_) -> println(0)
    Err(error) ->
        println(json.error_code(error))
        println(json.error_path(error))"#
            )
            .unwrap(),
        "5\n/ids/1\n"
    );
    session
        .evaluate(
            r#"newtype Broken = Broken(Int)
impl Json(Broken):
    fn to_json(value: Broken) -> Result(json.Value,json.Error): Ok(json.from_int(1 / value.0))
    fn from_json(value: json.Value) -> Result(Broken,json.Error): Ok(Broken(json.as_int(value)?))"#,
        )
        .unwrap();
    assert!(
        session
            .evaluate(
                "match json.encode(Broken(0)):\n    Ok(_) -> println(1)\n    Err(_) -> println(2)"
            )
            .unwrap_err()
            .contains("division by zero")
    );
    assert_eq!(session.evaluate("1 + 2").unwrap(), "3 : Int\n");
}

#[test]
fn callback_json_operations_share_limits_even_when_inner_errors_are_handled() {
    let mut session = Session::default();
    session.evaluate(r#"newtype Budgeted = Budgeted(Int)
impl Json(Budgeted):
    fn to_json(value: Budgeted) -> Result(json.Value,json.Error):
        let fallback = json.null()
        let text = String.repeat("x", value.0)
        for _ in 0..3:
            match json.from_string(text):
                Ok(_) -> ()
                Err(_) -> ()
        Ok(fallback)
    fn from_json(value: json.Value) -> Result(Budgeted,json.Error): Ok(Budgeted(json.as_int(value)?))"#).unwrap();
    let scope = morrow_json::scope::Scope::enter(100_000, 4096, 100).unwrap();
    assert_eq!(session.evaluate("match json.encode(Budgeted(1500)):\n    Ok(_) -> println(0)\n    Err(error) -> println(json.error_code(error))").unwrap(),"4\n");
    assert!(scope.spent().exhausted);
    drop(scope);
    assert_eq!(session.evaluate("match json.encode(Budgeted(1)):\n    Ok(text) -> println(text)\n    Err(error) -> println(json.error_code(error))").unwrap(),"null\n");
}

#[test]
fn deterministic_custom_json_simulation_preserves_full_width_values() {
    let mut session = Session::default();
    session.evaluate(CUSTOM).unwrap();
    session.evaluate("fn roundtrip(value: Int) -> Result(Int,json.Error):\n    let text=json.encode(UserId(value))?\n    let decoded=json.decode(text,UserId)?\n    Ok(decoded.0)").unwrap();
    let mut seed = 0x6176_391f_a943_faad_u64;
    for index in 0..64 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let value = match index {
            0 => i64::MIN,
            1 => i64::MAX,
            _ => seed as i64,
        };
        let source = format!(
            "match roundtrip({value}):\n    Ok(value) -> println(value)\n    Err(error) -> println(json.error_message(error))"
        );
        assert_eq!(
            session.evaluate(&source).unwrap(),
            format!("{value}\n"),
            "seed {seed}"
        );
    }
}

#[test]
fn recursive_custom_codec_requirements_terminate_and_execution_remains_bounded() {
    let mut session = Session::default();
    session
        .evaluate(
            r#"newtype Loop = Loop(Int)
impl Json(Loop):
    fn to_json(value: Loop) -> Result(json.Value,json.Error): json.parse(json.encode(value)?)
    fn from_json(value: json.Value) -> Result(Loop,json.Error): Ok(Loop(json.as_int(value)?))"#,
        )
        .unwrap();
    let result=session.evaluate("match json.encode(Loop(1)):\n    Ok(_) -> println(0)\n    Err(error) -> println(json.error_code(error))");
    match result {
        Ok(output) => assert_eq!(output, "4\n"),
        Err(message) => assert!(message.contains("limit exceeded"), "{message}"),
    }
    assert_eq!(session.evaluate("2 + 3").unwrap(), "5 : Int\n");
}

#[test]
fn infallible_builder_quota_unwinds_cleanup_and_becomes_a_codec_error() {
    let mut session = Session::default();
    session
        .evaluate(
            r#"newtype Quota = Quota(Int)
impl Json(Quota):
    fn to_json(value: Quota) -> Result(json.Value,json.Error):
        defer println("cleanup")
        let first=json.from_int(value.0)
        let second=json.from_int(value.0)
        Ok(second)
    fn from_json(value: json.Value) -> Result(Quota,json.Error): Ok(Quota(json.as_int(value)?))"#,
        )
        .unwrap();
    let scope = morrow_json::scope::Scope::enter(100_000, 100_000, 1).unwrap();
    assert_eq!(session.evaluate("match json.encode(Quota(1)):\n    Ok(_) -> println(0)\n    Err(error) -> println(json.error_code(error))").unwrap(),"cleanup\n4\n");
    drop(scope);
}

#[test]
fn json_trait_methods_are_available_without_an_explicit_custom_implementation() {
    let mut session = Session::default();
    assert_eq!(session.evaluate("match to_json(42):\n    Ok(value) -> match json.as_int(value):\n        Ok(number) -> println(number)\n        Err(_) -> println(-1)\n    Err(_) -> println(-2)").unwrap(),"42\n");
    session
        .evaluate("fn read(value: json.Value) -> Result(Int,json.Error): from_json(value)")
        .unwrap();
    assert_eq!(session.evaluate("match read(json.from_int(9007199254740993)):\n    Ok(value) -> println(value)\n    Err(_) -> println(-1)").unwrap(),"9007199254740993\n");
}

#[test]
fn checked_outer_records_allow_optional_and_union_custom_payloads() {
    let mut session = Session::default();
    session.evaluate(CUSTOM).unwrap();
    session.evaluate("fn optional() -> Result(Option(Envelope),json.Error):\n    json.decode(json.encode(Some(Envelope([UserId(42)])))?,Option(Envelope))\nfn union(value: Envelope | Int) -> Result(String,json.Error): json.encode(value)").unwrap();
    assert_eq!(session.evaluate("match optional():\n    Ok(Some(value)) -> println(List.len(value.ids))\n    Ok(None) -> println(0)\n    Err(_) -> println(-1)").unwrap(),"1\n");
    assert_eq!(session.evaluate("match union(Envelope([UserId(42)])):\n    Ok(text) -> println(text)\n    Err(_) -> println(0)").unwrap(),"{\"ids\":[\"user-42\"]}\n");
}

#[test]
fn executable_custom_callback_identity_and_signature_are_validated() {
    use morrow_compiler::{
        ir,
        json_codec::{Callback, Kind},
    };
    let source = format!(
        "{CUSTOM}\nfn encode(value: UserId) -> Result(String,json.Error): json.encode(value)\nfn wrong(value: Int) -> Int: value\n"
    );
    let checked = check::check_library(&parse::parse(&source).unwrap()).unwrap();
    for invalid in [
        Callback::Source("unknown".into()),
        Callback::Function(ir::FunctionId(usize::MAX)),
        Callback::Function(
            checked
                .functions
                .iter()
                .find(|f| f.name == "wrong")
                .unwrap()
                .id,
        ),
    ] {
        let mut program = checked.clone();
        let function = program
            .functions
            .iter_mut()
            .find(|f| f.name == "encode")
            .unwrap();
        let ir::ExprKind::JsonCodec { plan, .. } = &mut function.body.kind else {
            panic!("expected direct codec body");
        };
        let Kind::Custom { encode, .. } = &mut std::rc::Rc::make_mut(plan).entries[0].kind else {
            panic!("expected custom root");
        };
        *encode = invalid;
        let error = morrow_compiler::lowering::lower(&program).unwrap_err();
        assert!(error.message.contains("callback"), "{}", error.message);
    }
}

#[test]
fn nested_callback_error_paths_compose_with_the_containing_wire_path() {
    let mut session = Session::default();
    session
        .evaluate(
            r#"newtype Path = Path(Int)
impl Json(Path):
    fn to_json(value: Path) -> Result(json.Value,json.Error): Ok(json.from_int(value.0))
    fn from_json(value: json.Value) -> Result(Path,json.Error):
        let data=json.decode("\{\"inside\":[\"bad\"]\}",Map(String,List(Int)))?
        Ok(Path(Map.len(data)))
type Paths derive(Json):
    values: List(Path)"#,
        )
        .unwrap();
    assert_eq!(
        session
            .evaluate(
                r#"match json.decode("\{\"values\":[null]\}",Paths):
    Ok(_) -> println(0)
    Err(error) ->
        println(json.error_code(error))
        println(json.error_path(error))"#
            )
            .unwrap(),
        "5\n/values/0/inside/0\n"
    );
}
