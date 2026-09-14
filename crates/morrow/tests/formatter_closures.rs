use morrow_compiler::{check, format, lowering, parse};

fn emitted(source: &str) -> String {
    lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap()
}

#[test]
fn formatting_captures_nested_calls_and_block_callbacks_preserves_checked_code() {
    for source in [
        "fn make(base: Int) -> (Int) -> Int: (value) -> base + value\nfn main(): println(make(40)(2))\n",
        "fn apply(action: (Int) -> Int, number: Int) -> Int: action(number)\nfn main():\n    let base = 2\n    let result = apply(\n        fn(value) -> # callback\n            let doubled = value * 2\n            doubled + base\n        , 20\n    )\n    println(result)\n",
        "fn main():\n    let bias = 40\n    let values = [1, 2] |> List.map(\n        (value) ->\n            value + bias\n    )\n    println(List.get(values, 1))\n",
        "type Callback:\n    run: fn(Int) -> Int\nfn main():\n    let callback = Callback((n: Int) -> n + 1)\n    println(callback.run(41))\n",
    ] {
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
        assert_eq!(emitted(source), emitted(&formatted), "{source}");
    }
}
