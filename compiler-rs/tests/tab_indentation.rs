//! Decision3 permits consistent indentation while rejecting ambiguous mixed styles.
use fern_prototype::{format, parse};

#[test]
fn tab_suites_format_to_spaces_with_the_same_structure() {
    let source = "fn main():\n\tif true:\n\t\tprintln(42)\n\telse:\n\t\tprintln(0)\n";
    parse::parse(source).unwrap();
    let formatted = format::format(source).unwrap();
    assert_eq!(
        formatted,
        "fn main():\n    if true:\n        println(42)\n    else:\n        println(0)\n"
    );
    assert_eq!(format::format(&formatted).unwrap(), formatted);
}

#[test]
fn mixed_significant_indentation_is_rejected_with_byte_bounded_spans() {
    for source in [
        "fn main():\n \tprintln(1)\n",
        "fn main():\n\t println(1)\n",
        "fn main():\n\tprintln(1)\n        println(2)\n",
        "fn main():\n        println(1)\n\tprintln(2)\n",
        "fn first():\n\tprintln(1)\nfn main():\n    println(2)\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.message.contains("mixed tabs and spaces"), "{error:?}");
        assert!(error.span.start <= error.span.end && error.span.end <= source.len());
    }
}

#[test]
fn tabs_in_continuations_comments_and_strings_do_not_create_layout() {
    for source in [
        "fn main():\n\tprintln(\n \t  42\n\t)\n",
        "fn main():\n\t# comment\n \t # alignment is irrelevant\n\tprintln(42)\n",
        "fn main():\n\tlet values = [\n \t  1,\n  2\n\t]\n\tprintln(values[0])\n",
        "fn main():\n\tprintln(\"literal\ttext\")\n",
        "fn main():\n\tprintln(\"\"\"a\n \ttext\n\"\"\")\n",
        "fn main():\n\t/* comment\n \t mixed content\n*/\n\tprintln(42)\n",
        "fn\tmain():\n\tprintln(\t42\t)\n",
    ] {
        parse::parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn embedded_suites_accept_tabs_and_keep_delimiter_layout() {
    for source in [
        "fn main():\n\tlet values = List.map([1], (x: Int) ->\n\t\tif x > 0:\n\t\t\tx\n\t\telse:\n\t\t\t0\n\t)\n\tprintln(values[0])\n",
        "fn main():\n\tlet values = List.map(\n        [1],\n        (x: Int) ->\n\t\tx\n\t)\n\tprintln(values[0])\n",
    ] {
        parse::parse(source).unwrap();
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn tab_indentation_keeps_depth_and_source_spans_bounded() {
    let source = "fn main():\n\t\t0\n\t1\n";
    let error = parse::parse(source).unwrap_err();
    assert!(
        error.message.contains("inconsistent indentation"),
        "{error:?}"
    );
    assert!(error.span.end <= source.len());
    let mut deep = "fn main():\n".to_string();
    for depth in 1..140 {
        deep.push_str(&format!("{}if true:\n", "\t".repeat(depth)));
    }
    deep.push_str(&format!("{}0\n", "\t".repeat(140)));
    let error = parse::parse(&deep).unwrap_err();
    assert!(error.message.contains("depth"), "{error:?}");
    assert!(error.span.end <= deep.len());
}
