//! Inline match arms retain the existing AST, checker and canonical multiline formatting.
use fern_prototype::{check, format, parse, qbe, repl::Session};

#[test]
fn inline_arms_share_typed_matching_and_canonical_formatting() {
    let source = "fn choose(n: Int) -> Int:\n    match n: 0 -> 10, value if value > 0 -> 20, _ -> 30\nfn main(): println(choose(1))\n";
    let checked = check::check(&parse::parse(source).unwrap()).unwrap();
    let emitted = qbe::emit(&checked).unwrap();
    let formatted = format::format(source).unwrap();
    assert!(
        formatted.contains("match n:\n        0 -> 10\n"),
        "{formatted}"
    );
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    assert_eq!(
        qbe::emit(&check::check(&parse::parse(&formatted).unwrap()).unwrap()).unwrap(),
        emitted
    );
    assert_eq!(
        Session::default()
            .evaluate("match 2: 0 -> 10, _ -> 20")
            .unwrap(),
        "20 : Int\n"
    );
}

#[test]
fn inline_match_commas_do_not_consume_enclosing_call_or_collection_arguments() {
    for source in [
        "fn main(): println((match 1: 0 -> 10, _ -> 20))\n",
        "fn main(): println(List.get([match 1: 0 -> 10, _ -> 20, 30], 0))\n",
        "fn pair(first: Int, second: String) -> Int: first\nfn main(): println(pair(match 1: 0 -> 10, _ -> 20, \"tail\"))\n",
        "fn main(): println((match 1: 0 -> (1, 2), _ -> (3, 4)).0)\n",
    ] {
        let program = parse::parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
        check::check(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn inline_nested_matches_and_result_handlers_preserve_their_arm_boundaries() {
    for source in [
        "fn main(): println(match 1: 0 -> 1, _ -> (match 2: 0 -> 3, _ -> 4))\n",
        "fn unwrap(value: Result(Int, Int)) -> Int: match value: Ok(item) -> item, Err(_) -> 0\nfn main(): println(unwrap(Ok(1)))\n",
        "fn unwrap(value: Result(Int, Int)) -> Int:\n    with x <- value do x else Err(_) -> 0\nfn main(): println(unwrap(Ok(2)))\n",
    ] {
        let program = parse::parse(source).unwrap();
        check::check(&program).unwrap();
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn malformed_inline_arms_remain_errors_with_bounded_source_spans() {
    for source in [
        "fn main(): match 1:\n",
        "fn main(): match 1: -> 2\n",
        "fn main(): match 1: 0 -> 2, _ ->\n",
        "fn main(): match 1: 0 -> 2, _ 3\n",
        "fn main(): match 1: 0 -> 2,\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.span.start <= error.span.end && error.span.end <= source.len());
    }
}

#[test]
fn inline_match_depth_and_token_limits_still_reject_excessive_input() {
    let mut nested = "1".to_string();
    for _ in 0..140 {
        nested = format!("(match 0: _ -> {nested})");
    }
    let error = parse::parse(&format!("fn main(): {nested}\n")).unwrap_err();
    assert!(error.message.contains("depth"), "{error:?}");
    let arms = (0..20_000)
        .map(|n| format!("{n} -> 0"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("fn main(): match 0: {arms}\n");
    let error = parse::parse(&source).unwrap_err();
    assert!(error.message.contains("token"), "{error:?}");
    assert!(error.span.end <= source.len());
}

#[test]
fn a_caller_lambda_after_an_inline_match_requires_grouping() {
    let header = "fn apply(value: Int, action: (Int) -> Int) -> Int: action(value)\n";
    for callback in ["(x: Int) -> x + 1", "(x) -> x + 1"] {
        let grouped =
            format!("{header}fn main(): println(apply((match 0: 0 -> 1, _ -> 2), {callback}))\n");
        check::check(&parse::parse(&grouped).unwrap()).unwrap();
        let formatted = format::format(&grouped).unwrap();
        check::check(&parse::parse(&formatted).unwrap()).unwrap();
        let ungrouped =
            format!("{header}fn main(): println(apply(match 0: 0 -> 1, _ -> 2, {callback}))\n");
        assert!(parse::parse(&ungrouped)
            .and_then(|program| check::check(&program).map(|_| program))
            .is_err());
    }
}

#[test]
fn comma_arms_belong_to_the_nearest_unclosed_inline_match() {
    for (source, expected) in [
        ("match 1: 0 -> 0, _ -> match 2: 0 -> 1, _ -> 2", "2 : Int\n"),
        (
            "match 1: 0 -> (match 2: 0 -> 1, _ -> 2), _ -> 4",
            "4 : Int\n",
        ),
    ] {
        assert_eq!(Session::default().evaluate(source).unwrap(), expected);
        let program = format!("fn main(): println({source})\n");
        let formatted = format::format(&program).unwrap();
        check::check(&parse::parse(&formatted).unwrap()).unwrap();
    }
}
