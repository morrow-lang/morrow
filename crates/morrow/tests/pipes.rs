use morrow_compiler::{check, format, lowering, parse};
#[test]
fn standard_and_placeholder_pipes_lower_in_source_order() {
    let source = "fn sub(x: Int, y: Int) -> Int: x - y\nfn main():\n    let value = 10\n        |> sub(x: _, y: 3)\n        |> sub(x: 20, y: _)\n    println(value)\n";
    let syntax = parse::parse(source).unwrap();
    let il = lowering::emit(&check::check(&syntax).unwrap()).unwrap();
    let formatted = format::format(source).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    assert_eq!(
        lowering::emit(&check::check(&parse::parse(&formatted).unwrap()).unwrap()).unwrap(),
        il
    );
}
#[test]
fn pipes_require_one_callable_target_and_at_most_one_placeholder() {
    for source in ["fn main(): 1 |> 2", "fn main(): 1 |> println(_, _)"] {
        assert!(parse::parse(source).is_err(), "{source}");
    }
}
