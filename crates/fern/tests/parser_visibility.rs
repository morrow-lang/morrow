use fern_compiler::{format, parse};

#[test]
fn public_provenance_survives_parse_and_format_independently_of_exports() {
    let source = "@doc \"\"\"Public API.\"\"\"\npub fn visible(value: Int) -> Int: value\nfn helper(): 42\nfn main(): println(visible(helper()))\n";
    let program = parse::parse(source).unwrap();
    assert!(program.functions[0].public);
    assert!(!program.functions[1].public);
    assert!(!program.functions[2].public);
    assert_eq!(program.exports, ["visible"]);
    let canonical = format::format(source).unwrap();
    assert_eq!(format::format(&canonical).unwrap(), canonical);
    let formatted = parse::parse(&canonical).unwrap();
    assert_eq!(
        program
            .functions
            .iter()
            .map(|f| f.public)
            .collect::<Vec<_>>(),
        formatted
            .functions
            .iter()
            .map(|f| f.public)
            .collect::<Vec<_>>()
    );
}
