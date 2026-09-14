use morrow_compiler::{check, format, lowering, parse};

fn emitted(source: &str) -> String {
    lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap()
}

#[test]
fn return_let_else_conditions_and_defer_preserve_checked_emission() {
    for source in [
        "fn choose(value: Int) -> Int:\n    return value if value > 0\n    return 0\nfn main(): println(choose(42))\n",
        "fn choose(value: Option(Int)) -> Int:\n    defer println(\"done\")\n    let Some(number) = value else:\n        return 0\n    match:\n        number > 0 -> return number\n        _ -> 0\nfn main(): println(choose(Some(42)))\n",
        "fn main():\n    let label = \"🌿 first\"\n    defer println(label) # cleanup\n    let label = \"second\"\n    defer println(label)\n    println(\"body\") if true\n    return ()\n",
        "fn main():\n    let callback = (value: Int) ->\n        defer println(value)\n        return value + 1\n    println(callback(41))\n",
        "fn choose(value: Bool) -> Float:\n    if value: return 1.5 else: return 2.5\nfn main(): println(choose(value: true))\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
        assert_eq!(emitted(source), emitted(&canonical), "{source}");
    }
}
