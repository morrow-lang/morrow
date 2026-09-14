use morrow_compiler::{check, format, lowering, parse};

fn emitted(source: &str) -> String {
    lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap()
}

#[test]
fn map_and_record_formatting_preserves_checked_emission() {
    for source in [
        "fn main():\n    let empty: Map(String, Int) = %{}\n    let values = Map.put(empty, \"🌿\", 42)\n    println(Map.len(values))\n",
        "fn main():\n    let value = %{\"🌿\": %{1: 42},}\n    println(Map.len(value))\n    println(\"nested {Map.len(%{\"🌿\": %{1: 42}})}\")\n",
        "fn effect(value: Int) -> Int:\n    println(value)\n    value\nfn main():\n    let values = %{effect(1): effect(2), effect(3): effect(4)}\n    println(Map.len(values))\n",
        "type Pair:\n    first: Int\n    second: Float\nfn make() -> Pair:\n    println(0)\n    Pair(1, 2.5)\nfn effect(value: Int) -> Int:\n    println(value)\n    value\nfn main():\n    let original = make()\n    let updated = %{original | second: 4.5, first: effect(2),}\n    println(original.first)\n    println(updated.first)\n    println(updated.second)\n",
        "type Callback:\n    action: (Int) -> Int\nfn main():\n    let bias = 40\n    let callbacks: Map(String, (Int) -> Int) = %{\n        \"add\": (value) -> # capture\n            value + bias\n    }\n    let record = Callback((value) -> value)\n    let changed = %{record | action: (value) ->\n        value + bias\n    }\n    println(changed.action(Map.len(callbacks) + 1))\n",
    ] {
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
        assert_eq!(emitted(source), emitted(&formatted), "{source}");
    }
}
