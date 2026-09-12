use fern_compiler::{check, ir, parse};
fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
const ERRORS: &str = "type First:\n    FirstError(Int)\ntype Second:\n    SecondError(String)\nfn first() -> Result(Int, First): Err(FirstError(1))\nfn second(x: Int) -> Result(String, Second): Err(SecondError(\"bad\"))\n";
#[test]
fn heterogeneous_errors_get_distinct_typed_handlers() {
    let source = format!(
        "{ERRORS}fn run() -> Int:\n    with\n        x <- first(),\n        y <- second(x)\n    do\n        String.len(y)\n    else\n        Err(FirstError(n)) -> n\n        Err(SecondError(text)) -> String.len(text)\nfn main(): println(run())\n"
    );
    let p = checked(&source);
    let f = p.functions.iter().find(|f| f.name == "run").unwrap();
    assert!(format!("{:?}", f.body).contains("With"));
}
#[test]
fn with_without_else_propagates_compatible_errors_and_keeps_success_scope() {
    checked(
        "fn source() -> Result(Int, String): Ok(1)\nfn run() -> Result(String, String):\n    with\n        x <- source(),\n        y <- source()\n    do\n        Ok(\"yes\")\nfn main(): println(Result.unwrap_or(run(), \"no\"))\n",
    );
    for source in [
        "fn run() -> Int:\n    with x <- Ok(1) do x\nfn main(): ()",
        "fn source() -> Result(Int, String): Ok(1)\nfn run() -> Result(Int, Bool):\n    with x <- source() do Ok(x)\nfn main(): ()",
    ] {
        assert!(!rejected(source).is_empty());
    }
}
#[test]
fn generic_with_bodies_and_catchalls_specialize_without_erasing_errors() {
    checked(
        "fn recover(input: Result(a, e), fallback: a) -> a:\n    with value <- input do value else Err(_) -> fallback\nfn main():\n    let a: Result(Int, String) = Err(\"bad\")\n    let b: Result(String, Bool) = Err(true)\n    println(recover(a, 1))\n    println(recover(b, \"ok\"))\n",
    );
}
#[test]
fn with_handlers_require_per_type_coverage_and_outer_scope() {
    for tail in [
        "Err(FirstError(n)) -> n",
        "Err(FirstError(n)) -> y\n        Err(SecondError(text)) -> 0",
        "Err(FirstError(n)) -> n\n        Err(SecondError(text)) -> false",
        "error -> 0",
        "Ok(value) -> 0\n        _ -> 1",
    ] {
        let source = format!(
            "{ERRORS}fn run() -> Int:\n    with x <- first(), y <- second(x) do 0 else\n        {tail}\nfn main(): ()\n"
        );
        assert!(!rejected(&source).is_empty());
    }
}
#[test]
fn with_rejects_unreachable_handlers_non_results_and_cleanup_propagation() {
    for source in [
        "fn main(): with x <- 1 do println(x) else _ -> ()",
        "fn source() -> Result(Int, String): Ok(1)\nfn main(): with x <- source() do () else\n    _ -> ()\n    Err(_) -> ()",
        "fn source() -> Result(Int, String): Ok(1)\nfn main(): defer with x <- source() do () else _ -> ()",
    ] {
        assert!(!rejected(source).is_empty());
    }
}

#[test]
fn handler_arithmetic_infers_previously_unknown_error_payloads() {
    checked(
        "fn main():\n    let outer = 2\n    let value = with x <- Ok(7) do x else Err(error) -> outer + error\n    println(value)\n",
    );
    checked(
        "fn source() -> Result(Int, String): Ok(7)\nfn run() -> Result(Int, String): Ok(with x <- source() do x + 1)\nfn main(): println(Result.unwrap_or(run(), 0))\n",
    );
}

#[test]
fn with_handlers_are_shared_per_error_type_and_bindings_do_not_escape() {
    let p = checked(
        "fn source() -> Result(Int, String): Ok(1)\nfn main(): with x <- source(), y <- source() do println(x+y) else Err(_) -> ()\n",
    );
    let ir::ExprKind::With { handlers, .. } = &p
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .body
        .kind
    else {
        panic!()
    };
    assert_eq!(handlers.len(), 1);
    assert!(rejected("fn main():\n    with x <- Ok(1) do println(x) else Err(error) -> println(error + 1)\n    println(x)\n").contains("unknown"));
}

#[test]
fn with_cartesian_handler_expansion_is_bounded_before_cloning() {
    let mut source =
        String::from("fn operation() -> Result(Int, String): Ok(0)\nfn main():\n    with\n");
    for i in 0..400 {
        source.push_str(&format!("        x{i} <- operation(),\n"));
    }
    source.push_str("    do\n        ()\n    else\n");
    for _ in 0..400 {
        source.push_str("        Err(_) -> ()\n");
    }
    assert!(rejected(&source).contains("limit"));
}
