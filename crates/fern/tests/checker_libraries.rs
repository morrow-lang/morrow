use fern_compiler::{check, lowering, parse};

#[test]
fn library_graphs_validate_without_fabricating_an_entry_point() {
    for source in [
        "type User:\n    id: Int\n",
        "fn identity(value): value\n",
        "pub fn value()->Int:42\n",
    ] {
        let syntax = parse::parse(source).unwrap();
        let checked = check::check_library(&syntax).unwrap();
        assert!(
            checked
                .functions
                .iter()
                .all(|function| function.name != "main")
        );
        assert!(check::check(&syntax).unwrap_err().message.contains("main"));
        assert!(lowering::emit(&checked).is_err());
    }
}

#[test]
fn library_mode_retains_body_errors_generic_validation_and_free_names() {
    for source in [
        "pub fn wrong()->Int:true\n",
        "fn wrong(x:a)->a:x+1\n",
        "fn missing(): main()\n",
    ] {
        assert!(
            check::check_library(&parse::parse(source).unwrap()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn library_mode_preserves_existing_main_contracts() {
    for source in ["fn main(x:Int):()\n", "fn main()->String:\"invalid\"\n"] {
        assert!(check::check_library(&parse::parse(source).unwrap()).is_err());
    }
    let syntax = parse::parse("fn main()->Int:42\n").unwrap();
    assert_eq!(
        lowering::emit(&check::check(&syntax).unwrap()).unwrap(),
        lowering::emit(&check::check_library(&syntax).unwrap()).unwrap()
    );
}
