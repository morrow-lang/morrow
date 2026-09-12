use fern_compiler::parse;

#[test]
fn record_derivation_preserves_exact_trait_source_identity() {
    let source = "pub type Box(a) derive(Json):\n    value: a\nfn main():()\n";
    let program = parse::parse(source).unwrap();
    let declaration = &program.types[0];
    assert_eq!(declaration.derives.len(), 1);
    assert_eq!(declaration.derives[0].name, "Json");
    assert_eq!(
        &source[declaration.derives[0].span.start..declaration.derives[0].span.end],
        "Json"
    );
}

#[test]
fn derive_list_is_bounded_and_does_not_silently_accept_empty_or_duplicate_entries() {
    for source in [
        "type User derive():\n    name:String\n",
        "type User derive(Json,Json):\n    name:String\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.message.contains("derive"), "{error:?}");
    }
}

#[test]
fn unsupported_trait_names_remain_source_metadata_for_specific_checking_errors() {
    let program = parse::parse("type User derive(Show,Json):\n    name:String\n").unwrap();
    assert_eq!(program.types[0].derives.len(), 2);
    assert_eq!(program.types[0].derives[0].name, "Show");
}
