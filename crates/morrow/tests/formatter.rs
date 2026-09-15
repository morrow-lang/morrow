use morrow_compiler::{format::format, parse::parse};

#[test]
fn canonical_spacing_indentation_and_idempotence() {
    let source = "fn add( a:Int,b: Int)-> Int:\n  let answer=a+b*2\n  answer\n";
    let formatted = format(source).unwrap();
    assert!(formatted.starts_with("fn add(a: Int, b: Int) -> Int:\n    let answer = "));
    assert!(formatted.ends_with("    answer\n"));
    assert!(parse(&formatted).is_ok());
    assert_eq!(format(&formatted).unwrap(), formatted);
}

#[test]
fn preserves_unicode_strings_and_all_comments_in_order() {
    let source = "# module note\nfn main(): # entry\n  # user note\n  let names = [\n    \"🌿 # literal\", # first\n    # second note\n    \"Grüße\\n\\\"\\\\\",\n  ] # list\n  println(names) # print\n# final\n";
    let formatted = format(source).unwrap();
    let comments = [
        "# module note",
        "# entry",
        "# user note",
        "# first",
        "# second note",
        "# list",
        "# print",
        "# final",
    ];
    let mut last = 0;
    for comment in comments {
        let at = formatted[last..].find(comment).expect(comment) + last;
        last = at + comment.len();
    }
    assert!(formatted.contains("🌿 # literal"));
    assert!(formatted.contains("Grüße\\n\\\"\\\\"));
    assert!(formatted.contains("println(names)  # print"));
    assert_eq!(format(&formatted).unwrap(), formatted);
}

#[test]
fn formats_modules_generic_records_nested_patterns_and_guards() {
    let source = "module demo\npub import other.{A,b}\npub type Tree(a):\n  Leaf(value:a)\n  Branch(Tree(a),Tree(a))\ntype Person:\n  name:String\npub fn value(tree:Tree(Int))->Int:\n  match tree:\n    Branch(Leaf(x),_) if x>0 ->x\n    _->0\n";
    let formatted = format(source).unwrap();
    assert!(formatted.contains("pub import other.{A, b}"));
    assert!(formatted.contains("    Leaf(value: a)"));
    assert!(formatted.contains("Branch(Leaf(x), _) if "));
    assert_eq!(format(&formatted).unwrap(), formatted);
}

#[test]
fn formats_every_native_collection_and_nominal_fixture() {
    for directory in ["collections", "types"] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(directory);
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "mr") {
                let source = std::fs::read_to_string(&path).unwrap();
                let formatted =
                    format(&source).unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
                assert!(parse(&formatted).is_ok(), "{}", path.display());
                assert_eq!(format(&formatted).unwrap(), formatted, "{}", path.display());
            }
        }
    }
}

#[test]
fn invalid_or_unsupported_source_is_never_rewritten() {
    for source in [
        "fn main(:",
        "fn main(): \"hi {name\"",
        "fn main(): @",
        "fn main():\n \t0",
    ] {
        assert!(format(source).is_err(), "{source}");
    }
}

#[test]
fn preserves_multiline_operator_boundaries_and_unary_grouping() {
    for source in [
        "fn f() -> Int:\n    if true:\n        1\n    else:\n        2\n    + 3\n",
        "fn f() -> Int:\n    1 + if true:\n        2\n    else:\n        3\n",
        "fn f() -> Result(Int, String):\n    if true:\n        Ok(1)\n    else:\n        Err(\"x\")\n    ?\n",
        "fn f(): -(1 + 2) * (if true: 3 else: 4)\n",
    ] {
        let formatted = format(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn formatting_many_comments_and_deep_inputs_is_bounded() {
    let mut source = String::from("fn main():\n");
    for index in 0..1000 {
        source.push_str(&format!(
            "  # note {index}\n  let n{index}={index} # inline {index}\n"
        ));
    }
    let formatted = format(&source).unwrap();
    assert_eq!(formatted.matches("# note ").count(), 1000);
    assert_eq!(formatted.matches("# inline ").count(), 1000);
    assert_eq!(format(&formatted).unwrap(), formatted);
    let deep = format!("fn main(): {}0{}", "(".repeat(300), ")".repeat(300));
    assert!(format(&deep).is_err());
}

#[test]
fn formatting_native_fixtures_preserves_checked_qbe_exactly() {
    for directory in ["collections", "types"] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(directory);
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "mr") {
                let source = std::fs::read_to_string(&path).unwrap();
                let formatted = format(&source).unwrap();
                let compile = |source: &str| {
                    let ast = parse(source).unwrap();
                    let typed = morrow_compiler::check::check(&ast)
                        .unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
                    morrow_compiler::lowering::emit(&typed).unwrap()
                };
                assert_eq!(compile(&source), compile(&formatted), "{}", path.display());
            }
        }
    }
}
