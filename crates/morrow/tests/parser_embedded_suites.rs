use morrow_compiler::{format, parse};

#[test]
fn suites_compose_with_calls_lists_and_inline_closing_delimiters() {
    for source in [
        "fn main():\n    println(match [1,2]:\n        [head,..tail] -> head\n        [] -> 0)\n    println(42)\n",
        "fn main():\n    println(if true:\n        1\n    else:\n        2)\n",
        "fn main():\n    let values = [if true:\n        1\n    else:\n        2, 3]\n    println(List.len(values))\n",
        "fn main():\n    let value = (if true:\n        1\n    else:\n        2,)\n    println(value.0)\n",
        "fn main():\n    sink(for i in 0..=2:\n        println(i))\n",
        "fn main():\n    println(List.fold([1,2], 0, (sum,value) ->\n        sum+value))\n",
        "fn main():\n    println(\n        match [1,2]:\n            [head,..tail] -> head\n            [] -> 0\n    )\n",
        "fn main():\n    println(with\n        value <- load()\n    do\n        value\n    else\n        Err(_) -> 0)\n",
    ] {
        let canonical =
            format::format(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn nested_embedded_suites_preserve_callbacks_comments_strings_and_ranges() {
    for source in [
        "fn main():\n    println(if true:\n        match [1,2]:\n            [head,..tail] -> head\n            [] -> 0\n    else:\n        9) # outer\n",
        "fn main():\n    use(List.map([1,2], (x) ->\n        calculate(match x:\n            1 -> 42\n            _ -> x)))\n",
        "fn main():\n    println(match \"\"\"match if for:\n 🌿\"\"\": # header\n        text -> text) # close\n",
        "fn main():\n    let data = %{\n        \"{if true: 1 else: 2}\":\n            42,\n        \"other\":\n            99\n    }\n    println(Map.len(data))\n",
    ] {
        let canonical =
            format::format(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn malformed_embedded_suites_report_bounded_layout_errors() {
    for source in [
        "fn main(): f(if true:\n)\n",
        "fn main(): f(match 1:\n  _ -> 1]\n",
        "fn main(): f(for i in [1]:\n)\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.span.start <= error.span.end && error.span.end <= source.len());
    }
    let mut source = "fn main():\n".to_owned();
    for depth in 1..140 {
        source.push_str(&format!("{}f(if true:\n", "    ".repeat(depth)));
    }
    source.push_str(&format!("{}0{}\n", "    ".repeat(140), ")".repeat(139)));
    assert!(parse::parse(&source).unwrap_err().message.contains("limit"));
}
