use fern_compiler::{Type, check, format, lowering, parse};

#[test]
fn floating_point_values_keep_their_semantic_type_and_double_abi() {
    let source = "fn scale(x: Float) -> Float: -x * 2.5\nfn main():\n    println(scale(1.25))\n    println(1e2 >= 99.0)\n";
    let syntax = parse::parse(source).unwrap();
    assert_eq!(syntax.functions[0].params[0].annotation, Some(Type::Float));
    let typed = check::check(&syntax).unwrap();
    let output = lowering::emit(&typed).unwrap();
    assert!(output.contains("function d"));
    assert!(output.contains("=d mul"));
    assert!(output.contains("cged"));
    let formatted = format::format(source).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    assert_eq!(
        lowering::emit(&check::check(&parse::parse(&formatted).unwrap()).unwrap()).unwrap(),
        output
    );
}

#[test]
fn floats_cannot_mix_with_ints_or_use_integer_remainder() {
    for expr in ["1.0 + 1", "1.0 % 1.0", "not 1.0"] {
        let source = format!("fn main(): println({expr})\n");
        assert!(
            check::check(&parse::parse(&source).unwrap()).is_err(),
            "{source}"
        );
    }
    for expr in ["1e", "1.2.3", "1e999", "1_0.0"] {
        assert!(
            parse::parse(&format!("fn main(): println({expr})\n")).is_err(),
            "{expr}"
        );
    }
}

#[test]
fn floating_point_payloads_are_bitcast_for_collections_and_records() {
    let source = "type Box:\n    value: Float\nfn main():\n    let xs = [1.5, -0.0]\n    let box = Box(List.head(xs))\n    println(Option.unwrap_or(Some(box.value), 0.0))\n";
    let typed = check::check(&parse::parse(source).unwrap()).unwrap();
    let output = lowering::emit(&typed).unwrap();
    assert!(output.contains("=l cast"));
    assert!(output.contains("=d cast"));
}
