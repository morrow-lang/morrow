//! Independent native-profile oracles and aggregate/retained safety boundaries.
use super::*;
mod numeric;
fn limits() -> Limits {
    Limits::new(64 * 1024 * 1024)
}
fn parse(text: &str) -> Json {
    parse::document(text, &mut limits()).unwrap()
}
fn call(name: &str, args: &[Value]) -> Result<Value> {
    operation_value(name, args, &mut limits())
}
fn json(name: &str, args: &[Value]) -> Json {
    let Value::Json(value) = call(name, args).unwrap() else {
        panic!("expected JSON")
    };
    value
}
#[test]
fn immutable_builders_and_all_error_kinds() {
    let value = parse("[1]");
    let list = Value::List(Rc::new(vec![Value::Json(value.clone())]));
    let built = json("from_array", &[list]);
    assert_eq!(
        call("stringify", &[Value::Json(built)]).unwrap(),
        Value::String(Rc::new("[[1]]".into()))
    );
    let cases = [
        ("as_int", vec![Value::Json(parse("true"))], 5),
        (
            "get",
            vec![Value::Json(parse("{}")), Value::String(Rc::new("x".into()))],
            6,
        ),
        ("at", vec![Value::Json(value), Value::Int(-1)], 7),
        ("as_int", vec![Value::Json(parse("9223372036854775808"))], 8),
        ("as_int", vec![Value::Json(parse("1.01"))], 9),
        ("as_string", vec![Value::Json(parse("\"a\\u0000b\""))], 10),
        ("from_float", vec![Value::Float(f64::NAN)], 11),
    ];
    for (name, args, code) in cases {
        assert_eq!(call(name, &args).unwrap_err(), error(code, -1));
    }
}
#[test]
fn input_depth_and_expanded_node_boundaries_match_native() {
    assert_eq!(
        parse::document(&" ".repeat(INPUT + 1), &mut limits()).unwrap_err(),
        error(4, INPUT as i64)
    );
    for depth in [127, 128] {
        let text = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let result = parse::document(&text, &mut limits());
        if depth == 127 {
            assert_eq!(result.unwrap().height, 128);
        } else {
            assert_eq!(result.unwrap_err(), error(4, 128));
        }
    }
    let text = format!("[{}0]", "0,".repeat(NODES - 1));
    assert_eq!(
        parse::document(&text, &mut limits()).unwrap_err(),
        error(4, 199999)
    );
    let mut value = parse("null");
    for _ in 0..15 {
        value = json(
            "from_array",
            &[Value::List(Rc::new(vec![
                Value::Json(value.clone()),
                Value::Json(value),
            ]))],
        );
    }
    assert_eq!(
        call(
            "from_array",
            &[Value::List(Rc::new(vec![
                Value::Json(value.clone()),
                Value::Json(value)
            ]))]
        )
        .unwrap_err(),
        error(4, -1)
    );
}
#[test]
fn logical_allocation_work_and_entry_budgets_charge_before_failure() {
    let mut entry = limits();
    let mut budget = Budget::new(&mut entry, 0).unwrap();
    budget.allocate(ALLOC).unwrap();
    assert_eq!(budget.allocate(1).unwrap_err().code, 4);
    assert_eq!(budget.limits.allocated, ALLOC - 1);
    let mut tiny = Limits::new(1);
    assert_eq!(Budget::new(&mut tiny, 1).err(), Some(error(0, -1)));
    assert_eq!(tiny.work, 0);
    let mut entry = limits();
    let mut budget = Budget::new(&mut entry, 0).unwrap();
    budget.work(64 * NODES).unwrap();
    assert_eq!(budget.work(1).unwrap_err().code, 4);
}
#[test]
fn string_nul_keys_and_raw_input_terminator_match_native() {
    let node = parse("{\"a\\u0000b\":1}");
    assert_eq!(
        call("stringify", &[Value::Json(node.clone())]).unwrap(),
        Value::String(Rc::new("{\"a\\u0000b\":1}".into()))
    );
    assert_eq!(
        call(
            "get",
            &[Value::Json(node), Value::String(Rc::new("a\0b".into()))]
        )
        .unwrap_err(),
        error(6, -1)
    );
    let node = json("parse", &[Value::String(Rc::new("true\0false".into()))]);
    assert_eq!(
        call("as_bool", &[Value::Json(node)]).unwrap(),
        Value::Bool(true)
    );
}
#[test]
fn duplicate_group_order_and_unicode_failure_offsets_are_exact() {
    let cases = [
        ("{\"z\":0,\"a\":1,\"z\":2,\"a\":3}", 3, 19),
        ("\"\\uD800\\u000x\"", 1, 12),
        ("\"\\uD800\\u0041\"", 2, 1),
        ("\u{feff}\u{feff}null", 1, 3),
    ];
    for (text, code, offset) in cases {
        assert_eq!(
            parse::document(text, &mut limits()).unwrap_err(),
            error(code, offset),
            "{text}"
        );
    }
}
#[test]
fn encoding_exact_limit_and_above_are_checked_on_shared_children() {
    let scalar = json(
        "from_string",
        &[Value::String(Rc::new("\u{1}".repeat(1_048_575)))],
    );
    let small = json(
        "from_string",
        &[Value::String(Rc::new("a".repeat(1_048_575)))],
    );
    for extra in [0, 1] {
        let tail = json(
            "from_string",
            &[Value::String(Rc::new("a".repeat(1_048_572 + extra)))],
        );
        let mut children = vec![Value::Json(scalar.clone()), Value::Json(scalar.clone())];
        children.extend((0..3).map(|_| Value::Json(small.clone())));
        children.push(Value::Json(tail));
        let result = call("from_array", &[Value::List(Rc::new(children))]);
        if extra == 1 {
            assert_eq!(result.unwrap_err().code, 4);
        } else {
            let Value::Json(value) = result.unwrap() else {
                panic!()
            };
            assert_eq!(value.encoded, OUTPUT);
            let Value::String(text) = call("stringify", &[Value::Json(value)]).unwrap() else {
                panic!()
            };
            assert_eq!(text.len(), OUTPUT);
        }
    }
}

#[test]
fn failed_oversized_attempts_consume_normal_budget_and_cleanup_has_own_reserve() {
    let program = Rc::new(
        crate::check::check(
            &crate::parse::parse("fn clean(): println(json.is_null(json.null()))\nfn main(): ()")
                .unwrap(),
        )
        .unwrap(),
    );
    let cleanup = program
        .functions
        .iter()
        .find(|f| f.name == "clean")
        .unwrap()
        .id;
    let mut machine = Machine::new(program.clone(), HashMap::new());
    let args = [Value::String(Rc::new("x".repeat(INPUT + 1)))];
    let mut failed = false;
    for _ in 0..65 {
        match machine.json("fern_json_value_parse", &args).unwrap() {
            Ok(Value::Sum(1, fields)) => {
                assert!(matches!(fields[0], Value::JsonError(Error { code: 4, .. })))
            }
            Err(Failure::Message(message)) => {
                assert_eq!(message, "interactive evaluation limit exceeded");
                failed = true;
                break;
            }
            _ => panic!("unexpected parse result"),
        }
    }
    assert!(failed);
    machine.defers.push(Value::Closure(Rc::new(ClosureValue {
        program,
        function: cleanup,
        captures: Vec::new(),
    })));
    let before = machine.json_limits.work;
    let result = machine.finish(Err(fault("original failure")));
    assert!(matches!(result, Err(Failure::Message(message)) if message == "original failure"));
    assert_eq!(machine.output, "true\n");
    assert_eq!(machine.json_limits.work, before);
    assert!(machine.json_cleanup.allocated < 8 * 1024 * 1024);
}

#[test]
fn cleanup_budget_exhaustion_does_not_replenish_normal_budget() {
    let program =
        Rc::new(crate::check::check(&crate::parse::parse("fn main(): ()").unwrap()).unwrap());
    let mut machine = Machine::new(program, HashMap::new());
    machine.cleanup_depth = 1;
    machine.json_cleanup.allocated = 0;
    let before = machine.json_limits.allocated;
    assert!(matches!(
        machine.json("fern_json_value_null", &[]).unwrap(),
        Err(Failure::Message(_))
    ));
    assert_eq!(machine.json_limits.allocated, before);
    assert_eq!(machine.json_cleanup.allocated, 0);
}

#[test]
fn aggregate_allocations_include_semantic_node_and_collection_storage() {
    let mut small = Limits::new(72);
    assert_eq!(
        operation_value("null", &[], &mut small).unwrap_err(),
        error(0, -1)
    );
    let node = parse("[null]");
    let mut small = Limits::new(40);
    assert_eq!(
        value::access("elements", &node, &[], &mut small).unwrap_err(),
        error(0, -1)
    );
    let node = parse("{\"a\":null}");
    let mut small = Limits::new(105);
    assert_eq!(
        value::access("members", &node, &[], &mut small).unwrap_err(),
        error(0, -1)
    );
}

#[test]
fn repeated_common_prefixes_hit_the_same_native_comparison_budget() {
    let mut document = String::from("{");
    for i in 0..4000 {
        if i != 0 {
            document.push(',');
        }
        document.push('"');
        document.push_str(&"a".repeat(220));
        document.push_str(&format!("{:04}\":0", (i * 1741) % 4000));
    }
    document.push('}');
    assert_eq!(
        parse::document(&document, &mut limits()).unwrap_err(),
        error(4, document.len() as i64)
    );
}

#[test]
fn json_elements_preserve_native_lengths_above_generic_list_construction_cap() {
    let document = format!("[{}0]", "0,".repeat(69_999));
    let array = parse(&document);
    let Value::List(values) = call("elements", &[Value::Json(array)]).unwrap() else {
        panic!()
    };
    assert_eq!(values.len(), 70_000);
    assert!(matches!(values.last(), Some(Value::Json(_))));
}
