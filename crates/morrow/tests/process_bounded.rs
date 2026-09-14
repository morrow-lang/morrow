use morrow_compiler::{Type, check, lowering, parse, runtime};

fn lowered(source: &str) -> String {
    let ast = parse::parse(source).unwrap();
    let checked = check::check(&ast).unwrap();
    lowering::emit(&checked).unwrap()
}

#[test]
fn bounded_process_signature_retains_result_and_full_width_arguments() {
    let signature = runtime::lookup("System.exec_args_bounded").expect("bounded process API");
    assert_eq!(
        signature.parameters,
        vec![Type::List(Box::new(Type::String)), Type::Int, Type::Int]
    );
    assert_eq!(
        signature.return_type,
        Type::Result(
            Box::new(Type::Tuple(vec![Type::Int, Type::String, Type::String])),
            Box::new(Type::Int)
        )
    );
    assert!(signature.requires_adapter());
    let il = lowered(
        "fn main():\n    match System.exec_args_bounded([\"tool\"], 4294967297, -4294967295):\n        Ok((status, out, err)) -> println(status)\n        Err(code) -> println(code)\n",
    );
    let call = il
        .lines()
        .find(|line| line.contains("call $morrow_exec_args_bounded("))
        .unwrap();
    assert_eq!(
        call.split_once("bounded(").unwrap().1.matches("l ").count(),
        3,
        "{call}"
    );
    assert!(
        il.contains("4294967297") && il.contains("4294967295"),
        "{il}"
    );
}

#[test]
fn bounded_result_converts_only_success_tuple_after_testing_tag() {
    let il = lowered(
        "fn main():\n    match System.exec_args_bounded([\"tool\"], 1000, 4096):\n        Ok((status, out, err)) -> println(out)\n        Err(code) -> println(code)\n",
    );
    assert_eq!(
        il.matches("call $morrow_exec_args_bounded(").count(),
        1,
        "{il}"
    );
    let call = il.find("call $morrow_exec_args_bounded(").unwrap();
    let test = il[call..].find("call $morrow_result_is_ok(").unwrap() + call;
    let branch = il[test..].find("jnz ").unwrap() + test;
    let unwrap = il[branch..].find("call $morrow_result_unwrap(").unwrap() + branch;
    let wrap = il[unwrap..].find("call $morrow_result_ok(").unwrap() + unwrap;
    assert!(call < test && test < branch && branch < unwrap && unwrap < wrap);
    assert!(
        il[unwrap..wrap].contains("32"),
        "tagged tuple allocation missing: {il}"
    );
    assert!(
        il[wrap..].contains("phi "),
        "error/result merge missing: {il}"
    );
}

#[test]
fn bounded_process_function_values_propagation_and_result_obligations() {
    let source = "fn invoke() -> Result((Int, String, String), Int):\n    let call: (List(String), Int, Int) -> Result((Int, String, String), Int) = System.exec_args_bounded\n    let result = call([\"tool\"], 1000, 4096)?\n    Ok(result)\nfn main():\n    match invoke():\n        Ok((status, out, err)) -> println(err)\n        Err(code) -> println(code)\n";
    assert!(lowered(source).contains("call $morrow_exec_args_bounded("));
    for body in [
        "System.exec_args_bounded([\"tool\"], 1000, 4096)\n    ()",
        "let _ = System.exec_args_bounded([\"tool\"], 1000, 4096)\n    ()",
        "System.exec_args_bounded([1], 1000, 4096)",
    ] {
        let ast = parse::parse(&format!("fn main():\n    {body}\n")).unwrap();
        assert!(check::check(&ast).is_err(), "{body}");
    }
}
