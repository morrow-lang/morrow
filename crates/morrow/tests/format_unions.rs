use morrow_compiler::{Type, check, format, parse, presentation};
fn roundtrip(source: &str) {
    let formatted = format::format(source).unwrap();
    assert_eq!(formatted, format::format(&formatted).unwrap());
    check::check_library(&parse::parse(source).unwrap()).unwrap();
    check::check_library(&parse::parse(&formatted).unwrap()).unwrap();
}
#[test]
fn typed_pattern_spans_and_nested_union_groups_survive_formatting() {
    roundtrip(
        "fn size(x:(Int | String) | Bool)->Int:\n    match x:\n        n:Int if n>0 -> n\n        n:Int -> 0\n        s:String -> String.len(s)\n        b:Bool -> if b:1 else:0\n",
    );
}
#[test]
fn union_function_members_and_function_results_keep_different_grouping() {
    for source in [
        "type Callback=((Int)->String) | Bool\n",
        "type Callback=(Int | String)->Float | Bool\n",
        "type Callback=((Int)->String | Bool) | Int\n",
        "newtype Wrapped=Wrapped(List(Int | String))\n",
        "type Row:\n    value:Option(Int | String)\n",
    ] {
        roundtrip(source);
    }
}
#[test]
fn presentation_is_readable_and_reparses_with_the_same_type_shape() {
    let ty = Type::Union(vec![Type::Int, Type::String]);
    assert_eq!(
        presentation::render_type(&ty, Default::default()).unwrap(),
        "Int | String"
    );
    let ty = Type::Union(vec![
        Type::Function(vec![Type::Int], Box::new(Type::String)),
        Type::Bool,
    ]);
    let text = presentation::render_type(&ty, Default::default()).unwrap();
    assert_eq!(text, "((Int) -> String) | Bool");
    let program = parse::parse(&format!("type Callback={text}\n")).unwrap();
    assert_eq!(program.aliases[0].target, ty);
}
#[test]
fn malformed_public_union_presentation_is_bounded_and_explicit() {
    for ty in [
        Type::Union(vec![]),
        Type::Union(vec![Type::Int]),
        Type::Union(vec![Type::Int; 129]),
    ] {
        assert!(
            presentation::render_type(&ty, Default::default()).is_err(),
            "{ty:?}"
        );
    }
    let ty = Type::Union(vec![Type::Int, Type::String]);
    assert!(
        presentation::render_type(
            &ty,
            presentation::Limits {
                bytes: 3,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn grouped_narrowing_annotations_do_not_consume_the_match_arm_arrow() {
    roundtrip(
        "fn size(x:Int | String | Bool)->Int:\n    match x:\n        small:(Int | String) -> match small:\n            n:Int -> n\n            s:String -> String.len(s)\n        _:Bool -> 0\n",
    );
}
#[test]
fn union_quantifiers_remain_distinct_and_typed_pattern_metadata_is_validated() {
    let generated = "$inferred0".to_owned();
    let ty = Type::Union(vec![
        Type::Generic(generated.clone()),
        Type::Generic("a".into()),
    ]);
    assert_eq!(
        presentation::render_type_with_names(&ty, &[generated], Default::default()).unwrap(),
        "b | a"
    );
    let mut function = parse::parse("fn keep(x:Int)->Int:x\n")
        .unwrap()
        .functions
        .remove(0);
    function.params[0].pattern.kind = morrow_compiler::ast::PatternKind::Typed {
        pattern: Box::new(function.params[0].pattern.clone()),
        annotation: Type::Union(Vec::new()),
    };
    assert!(
        presentation::resolved_signature(
            &function,
            &[Type::Int],
            &Type::Int,
            &[],
            Default::default()
        )
        .is_err()
    );
}
