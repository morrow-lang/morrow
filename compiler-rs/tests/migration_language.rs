//! Executable compatibility boundaries discovered by the C-to-Rust migration audit.
use fern_prototype::{check, format, parse, qbe, repl::Session, runtime};

#[test]
fn shipping_service_aliases_preserve_canonical_identity_and_abi() {
    let mut aliases = 0;
    for name in runtime::names() {
        for (canonical, alias) in [("http.", "Http."), ("sql.", "Sql."), ("actors.", "Actors.")] {
            if let Some(function) = name.strip_prefix(canonical) {
                let compatibility = format!("{alias}{function}");
                assert_eq!(
                    runtime::lookup(&compatibility),
                    runtime::lookup(name),
                    "{compatibility}"
                );
                assert_eq!(
                    runtime::resolve(&compatibility),
                    runtime::resolve(name),
                    "{compatibility}"
                );
                aliases += 1;
            }
        }
    }
    assert_eq!(aliases, 14);
    for invalid in ["HTTP.get", "SQL.open", "Actor.start", "Http.missing"] {
        assert!(runtime::lookup(invalid).is_none(), "{invalid}");
    }
}

#[test]
fn bracket_indexing_reuses_checked_list_access_and_canonical_format() {
    for source in [
        "fn main(): println([10, 20][1])\n",
        "fn main(): println([[42]][0][0])\n",
        "fn main(): println([\"fern\"][0])\n",
        "fn main(): println([1.5, 2.5][0] + 1.0)\n",
        "fn value(xs: List(Int), i: Int) -> Int: xs[i]\nfn main(): println(value([42], 0))\n",
        "fn main(): println([(1, 2)][0].1)\n",
        "fn main(): println([1, 2][\n    1\n])\n",
    ] {
        let parsed = parse::parse(source).unwrap();
        let checked = check::check(&parsed).unwrap();
        qbe::emit(&checked).unwrap();
        let canonical = format::format(source).unwrap();
        assert!(canonical.contains("List.get"), "{canonical}");
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
    assert_eq!(
        Session::default().evaluate("[[42]][0][0]").unwrap(),
        "42 : Int\n"
    );
}

#[test]
fn invalid_brackets_and_wrong_index_types_fail_without_bypassing_results() {
    for source in [
        "fn main(): [1][]\n",
        "fn main(): [1][0, 1]\n",
        "fn main(): [1][0\n",
    ] {
        assert!(parse::parse(source).is_err(), "{source}");
    }
    for source in [
        "fn main(): println([1][true])\n",
        "fn main(): println(1[0])\n",
        "fn main(): println(%{\"key\": 1}[\"key\"])\n",
        "fn main():\n    let xs: List(Result(Int, Int)) = [Ok(1), Err(2)]\n    println(Result.is_ok(xs[0]))\n",
    ] {
        assert!(check::check(&parse::parse(source).unwrap()).is_err(), "{source}");
    }
    let deep = format!("fn main(): [1]{}\n", "[0]".repeat(140));
    assert!(parse::parse(&deep).unwrap_err().message.contains("depth"));
}

#[test]
fn membership_uses_scalar_semantics_precedence_and_source_order() {
    for (expression, expected) in [
        ("2 in [1, 2, 3]", "true : Bool\n"),
        ("4 in [1, 2, 3]", "false : Bool\n"),
        ("1 in []", "false : Bool\n"),
        ("\"fern\" in [\"fern\", \"x\"]", "true : Bool\n"),
        ("-0.0 in [0.0]", "true : Bool\n"),
        ("false in [true]", "false : Bool\n"),
        ("1 + 1 in [2] and 3 in [3]", "true : Bool\n"),
    ] {
        assert_eq!(Session::default().evaluate(expression).unwrap(), expected);
        let source = format!("fn main(): println({expression})\n");
        qbe::emit(&check::check(&parse::parse(&source).unwrap()).unwrap()).unwrap();
        let canonical = format::format(&source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
    let mut session = Session::default();
    session
        .evaluate("fn item() -> Int:\n    println(1)\n    7")
        .unwrap();
    session
        .evaluate("fn items() -> List(Int):\n    println(2)\n    [7]")
        .unwrap();
    assert_eq!(
        session.evaluate("println(item() in items())").unwrap(),
        "1\n2\ntrue\n"
    );
    let source = "fn member(item: a, items: List(a)) -> Bool: item in items\nfn main(): println(member(1, [1]))\n";
    qbe::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap();
    for source in [
        "fn main(): println(1 in [true])\n",
        "fn main(): println([1] in [[1]])\n",
        "fn main(): println(1 in 2)\n",
    ] {
        assert!(
            check::check(&parse::parse(source).unwrap()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn all_shipping_c_builtin_names_have_a_rust_registry_or_intrinsic_contract() {
    let mut module = None;
    let mut names = std::collections::BTreeSet::new();
    for line in include_str!("../../lib/checker.c").lines() {
        if let Some((_, rest)) = line.split_once("if (strcmp(module, \"") {
            module = rest.split('"').next();
        }
        if let Some(module) = module {
            for rest in line.split("strcmp(func, \"").skip(1) {
                let name = rest.split('"').next().unwrap();
                names.insert(format!("{module}.{name}"));
            }
        }
        if let Some((_, rest)) = line.split_once("register_builtin(checker, \"") {
            names.insert(rest.split('"').next().unwrap().to_owned());
        }
    }
    assert!(
        names.len() >= 212,
        "C source inventory extraction lost coverage"
    );
    for name in names {
        let intrinsic = runtime::omissions()
            .iter()
            .any(|entry| entry.names.contains(&name.as_str()));
        assert!(
            runtime::lookup(&name).is_some() || intrinsic,
            "missing shipping C API: {name}"
        );
    }
}

#[test]
fn a_list_statement_after_a_dedent_is_not_a_postfix_index() {
    let source = "fn fallback() -> List(Int):\n    for item in 0..0:\n        println(item)\n    [42]\nfn main(): println(fallback()[0])\n";
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    qbe::emit(&program).unwrap();
    let canonical = format::format(source).unwrap();
    assert_eq!(format::format(&canonical).unwrap(), canonical);
}
