use fern_compiler::{format, parse};

#[test]
fn maps_updates_and_arbitrary_keys_parse_and_roundtrip() {
    for source in [
        "fn main():\n    let empty: Map(String, Int) = %{}\n    let map = %{\n        key(1) |> normalize(): value(2),\n        \"🌿\": 42,\n    }\n    map\n",
        "fn main():\n    let updated = %{ factory() | second: next(1), first: next(2), }\n    updated\n",
        "fn main(): println(\"value {Map.get(%{\"🌿\": %{1: 42}}, \"🌿\")}\")\n",
        "fn main():\n    let callbacks: Map(String, (Int) -> Int) = %{\n        \"double\": (x) ->\n            let result = x * 2\n            result\n        ,\n        \"identity\": (x) -> x,\n    }\n    callbacks\n",
        "fn main():\n    let updated = %{ record | callback: (x: Int) ->\n        x + 1\n    , count: 2\n    }\n    updated\n",
    ] {
        parse::parse(source).unwrap();
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn malformed_entries_updates_and_type_arities_are_located() {
    for source in [
        "fn main(): %{\"a\" 1}\n",
        "fn main(): %{\"a\": }\n",
        "fn main(): %{record |}\n",
        "fn main(): %{record | \"field\": 1}\n",
        "fn main(): %{record | field: 1, field: 2}\n",
        "fn main(): %{record | field: 1 | other: 2}\n",
        "fn main(): let value: Map(Int) = %{}\n",
        "fn main(): let value: Map(Int, String, Bool) = %{}\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.span.start < source.len(), "{source}: {error:?}");
    }
    let source = "fn main(): %{record | field: 1, field: 2}\n";
    let error = parse::parse(source).unwrap_err();
    assert!(error.message.contains("duplicate"), "{error:?}");
    assert_eq!(&source[error.span.start..error.span.end], "field");
}

#[test]
fn map_nesting_and_truncated_unicode_input_remain_bounded() {
    let expression = format!("{}42{}", "%{1: ".repeat(140), "}".repeat(140));
    let error = parse::parse(&format!("fn main(): {expression}\n")).unwrap_err();
    assert!(error.message.contains("limit"));
    let source = "fn main(): %{\"🌿\": %{factory() | field: \"value\"}}\n";
    for end in (0..source.len()).filter(|end| source.is_char_boundary(*end)) {
        let _ = parse::parse(&source[..end]);
    }
}

#[test]
fn map_ast_preserves_key_value_order_and_record_replacement_order() {
    use fern_compiler::{Type, ast::ExprKind};
    let source = "fn data() -> Map(String, Int): %{key(1): value(2), key(3): value(4)}\nfn update(): %{base(0) | second: next(1), first: next(2)}\n";
    let program = parse::parse(source).unwrap();
    assert_eq!(
        program.functions[0].return_type,
        Some(Type::Map(Box::new(Type::String), Box::new(Type::Int)))
    );
    let ExprKind::Map(pairs) = &program.functions[0].body.kind else {
        panic!()
    };
    let values = pairs
        .iter()
        .flat_map(|(key, value)| [key, value])
        .map(|expr| {
            let ExprKind::Call { name, args } = &expr.kind else {
                panic!()
            };
            (name.as_str(), format!("{:?}", args[0].kind))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            ("key", "Int(1)".into()),
            ("value", "Int(2)".into()),
            ("key", "Int(3)".into()),
            ("value", "Int(4)".into())
        ]
    );
    let ExprKind::RecordUpdate { value, fields } = &program.functions[1].body.kind else {
        panic!()
    };
    assert!(matches!(&value.kind,ExprKind::Call{name,..} if name=="base"));
    assert_eq!(
        fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        vec!["second", "first"]
    );
    for field in fields {
        assert_eq!(&source[field.span.start..field.span.end], field.name);
    }
}

#[test]
fn maps_with_block_keys_bases_and_comments_roundtrip() {
    for source in [
        "fn main():\n    let map = %{\n        apply(\n            () ->\n                \"key\"\n        ): 42\n    }\n    map\n",
        "fn main():\n    let record = %{ apply(\n        () ->\n            record\n    ) | value: 42 }\n    record\n",
        "fn main():\n    let map = %{ # map\n        \"key\": (x: Int) -> # callback\n            x + 1 # result\n    } # end\n    map\n",
    ] {
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn compact_record_updates_use_canonical_separator_spacing() {
    assert_eq!(
        format::format("fn main(): %{record|value:42}\n").unwrap(),
        "fn main(): %{ record | value: 42 }\n"
    );
}
