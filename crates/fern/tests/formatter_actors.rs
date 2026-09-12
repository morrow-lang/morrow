use fern_compiler::{check, format, lowering, parse};

#[test]
fn actor_suites_format_without_changing_capture_or_timeout_semantics() {
    let source = "fn worker():\n    let first = receive:\n        1 -> 1\n        _ after 0 -> 0\n    receive:\n        value if value > first -> println(value)\n        _ after 10 -> println(first)\nfn main() -> Result((), Int):\n    let pid: Pid(Int) = spawn(worker)\n    send(pid, 42)?\n    Ok(())\n";
    let before = check::check(&parse::parse(source).unwrap()).unwrap();
    let formatted = format::format(source).unwrap();
    let after = check::check(&parse::parse(&formatted).unwrap()).unwrap();
    assert_eq!(
        lowering::emit(&before).unwrap(),
        lowering::emit(&after).unwrap()
    );
    assert_eq!(format::format(&formatted).unwrap(), formatted);
}
