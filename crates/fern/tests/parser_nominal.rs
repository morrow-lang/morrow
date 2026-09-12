use fern_compiler::{
    Type,
    ast::{ExprKind, PatternKind, Stmt},
    parse::parse,
};

#[test]
fn generic_sum_and_record_declarations_preserve_layout() {
    let program = parse("pub type Tree(a):\n    Leaf(a)\n    Branch(left: Tree(a), right: Tree(a))\n\ntype Person:\n    name: String\n    age: Int\n\nfn id(value: a) -> a: value\n").unwrap();
    assert_eq!(program.types.len(), 2);
    assert_eq!(program.types[0].parameters, vec!["a"]);
    assert_eq!(
        program.types[0].variants[0].fields[0].ty,
        Type::Generic("a".into())
    );
    assert_eq!(
        program.types[0].variants[1].fields[0].name.as_deref(),
        Some("left")
    );
    assert!(program.types[1].record);
    assert_eq!(program.types[1].variants[0].name, "Person");
    assert_eq!(program.exports, vec!["Tree"]);
    assert_eq!(
        program.functions[0].return_type,
        Some(Type::Generic("a".into()))
    );
}

#[test]
fn modules_import_forms_and_public_exports() {
    let program = parse("module math.geometry\nimport math.core\nimport math.core as core\nimport math.core.*\npub import math.core.{\n    Point, origin,\n}\npub fn area(shape: core.Shape(Int)) -> Int: 0\n").unwrap();
    assert_eq!(program.module.as_deref(), Some("math.geometry"));
    assert_eq!(program.imports.len(), 4);
    assert_eq!(program.imports[1].alias.as_deref(), Some("core"));
    assert_eq!(program.imports[2].items, Some(vec!["*".into()]));
    assert_eq!(
        program.imports[3].items,
        Some(vec!["Point".into(), "origin".into()])
    );
    assert!(program.imports[3].public);
    assert_eq!(program.exports, vec!["area"]);
    assert_eq!(
        program.functions[0].params[0].annotation.clone().unwrap(),
        Type::Named("core.Shape".into(), vec![Type::Int])
    );
}

#[test]
fn nested_patterns_guards_and_postfix_field_access() {
    let program = parse("fn main():\n    match make().value:\n        Branch(Leaf(x), _) if x > 0 -> x\n        None -> 0\n        Empty -> 0\n").unwrap();
    let ExprKind::Block(stmts) = &program.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Expr(matching) = &stmts[0] else {
        panic!()
    };
    let ExprKind::Match { value, arms } = &matching.kind else {
        panic!()
    };
    assert!(matches!(&value.kind, ExprKind::Field { name, .. } if name == "value"));
    assert!(arms[0].guard.is_some());
    assert!(
        matches!(&arms[0].pattern.kind, PatternKind::NamedConstructor {name, fields} if name == "Branch" && fields.len() == 2)
    );
    assert!(
        matches!(&arms[2].pattern.kind, PatternKind::NamedConstructor {name, fields} if name == "Empty" && fields.is_empty())
    );
}

#[test]
fn declarations_and_nested_patterns_are_bounded_and_diagnosed() {
    for source in [
        "module a\nmodule b\n",
        "pub module a\n",
        "type Person:\n    name: String\n    Variant\n",
        "import module.*",
        "type Tree(a a):\n    Leaf(a)\n",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    let source = format!(
        "fn main():\n    match 0:\n        {}0{} -> 1",
        "Some(".repeat(200),
        ")".repeat(200)
    );
    assert!(parse(&source).unwrap_err().message.contains("depth"));
}

#[test]
fn builtin_nested_literal_patterns_and_guards_are_now_supported() {
    for source in [
        "fn main():\n    match 0:\n        Some(Some(x)) -> 0",
        "fn main():\n    match 0:\n        Some(1) -> 0",
        "fn main():\n    match 0:\n        x if true -> 0",
    ] {
        assert!(parse(source).is_ok(), "{source}: {:?}", parse(source));
    }
}

#[test]
fn unit_named_annotation_uses_the_primitive_unit_type() {
    let program = parse("pub fn describe(value: Int) -> Unit: println(value)\n").unwrap();
    assert_eq!(program.functions[0].return_type, Some(Type::Unit));
    assert!(parse("fn bad(value: Unit(Int)): value").is_err());
}
