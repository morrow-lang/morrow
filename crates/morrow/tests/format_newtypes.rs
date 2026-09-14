use morrow_compiler::{format, parse};
#[test]
fn newtype_formatting_preserves_docs_visibility_and_generic_constructor_spelling() {
    let source = "@doc \"\"\"A wrapper.\"\"\"\npub newtype Wrapper(a)=Packed(a)\nfn main(): ()\n";
    let formatted = format::format(source).unwrap();
    assert!(
        formatted.contains("pub newtype Wrapper(a) = Packed(a)"),
        "{formatted}"
    );
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    assert!(parse::parse(&formatted).is_ok());
}

#[test]
fn same_spelled_value_and_type_documentation_remain_on_their_source_declarations() {
    let source = "@doc \"\"\"Type documentation.\"\"\"\nnewtype Box=Packed(Int)\n@doc \"\"\"Function documentation.\"\"\"\nfn Box(value: Int) -> Int: value\nfn main(): println(Box(1))\n";
    let formatted = format::format(source).unwrap();
    assert!(
        formatted.contains("@doc \"\"\"Type documentation.\"\"\"\nnewtype Box = Packed(Int)"),
        "{formatted}"
    );
    assert!(
        formatted.contains("@doc \"\"\"Function documentation.\"\"\"\nfn Box("),
        "{formatted}"
    );
    assert_eq!(format::format(&formatted).unwrap(), formatted);
}

#[test]
fn newtype_native_sources_roundtrip_without_changing_generated_code() {
    let mut files = std::fs::read_dir("tests/newtypes_native")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "fn"))
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty());
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let formatted =
            format::format(&source).unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
        assert_eq!(
            format::format(&formatted).unwrap(),
            formatted,
            "{}",
            path.display()
        );
        let before = morrow_compiler::check::check(&parse::parse(&source).unwrap()).unwrap();
        let after = morrow_compiler::check::check(&parse::parse(&formatted).unwrap()).unwrap();
        assert_eq!(
            morrow_compiler::lowering::emit(&before).unwrap(),
            morrow_compiler::lowering::emit(&after).unwrap(),
            "{}",
            path.display()
        );
    }
}
