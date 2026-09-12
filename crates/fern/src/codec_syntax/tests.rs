use super::*;
fn target(source: &str) -> Type {
    let mut program = crate::parse::parse(source).unwrap();
    let expr = &mut program.functions[0].body;
    prepare(expr, "json.decode").unwrap();
    let ast::ExprKind::Call { args, .. } = &expr.kind else {
        panic!("call")
    };
    let ast::ExprKind::TypeTarget(ty) = &args[1].value.kind else {
        panic!("target")
    };
    ty.clone()
}
#[test]
fn static_names_and_type_constructors_do_not_become_value_calls() {
    assert_eq!(
        target("fn f():json.decode(\"[]\",List(User))\n"),
        Type::List(Box::new(Type::Named("User".into(), vec![])))
    );
    assert_eq!(
        target("fn f():json.decode(\"[]\",(Int,String))\n"),
        Type::Tuple(vec![Type::Int, Type::String])
    );
}
#[test]
fn static_target_rejects_arbitrary_executable_syntax_without_rewriting_it() {
    for target in ["42", "effect()", "User(value:effect())", "[User]", "(x)->x"] {
        let mut program =
            crate::parse::parse(&format!("fn f():json.decode(\"x\",{target})\n")).unwrap();
        let before = format!("{:?}", program);
        assert!(prepare(&mut program.functions[0].body, "json.decode").is_err());
        assert_eq!(format!("{:?}", program), before);
    }
}
#[test]
fn a_loader_proven_builtin_alias_uses_canonical_identity_not_source_spelling() {
    let mut program = crate::parse::parse("fn f():alias.read(\"1\",Int)\n").unwrap();
    let expression = &mut program.functions[0].body;
    let ast::ExprKind::Call { name, args } = expression.kind.clone() else {
        panic!("call")
    };
    expression.kind = ast::ExprKind::GlobalCall {
        name,
        resolved: "json.decode".into(),
        args,
    };
    prepare(expression, "json.decode").unwrap();
    assert_eq!(static_slot(expression), Some(1));
    let ast::ExprKind::GlobalCall { name, args, .. } = &expression.kind else {
        panic!("global")
    };
    assert_eq!(name, "alias.read");
    assert!(matches!(
        args[1].value.kind,
        ast::ExprKind::TypeTarget(Type::Int)
    ));
}
