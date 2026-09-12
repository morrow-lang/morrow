use fern_compiler::{check, parse};

fn accepted(source: &str) {
    let parsed = parse::parse(source).unwrap();
    check::check_library(&parsed).unwrap_or_else(|error| panic!("{source}: {error:?}"));
}
fn rejected(source: &str, needle: &str) {
    let parsed = parse::parse(source).unwrap();
    let error = check::check_library(&parsed).unwrap_err();
    assert!(error.message.contains(needle), "{error:?}");
}

#[test]
fn exact_bool_and_repeated_declared_types_require_labels() {
    rejected(
        "fn choose(enabled:Bool)->Int:if enabled: 1 else: 0\nfn main():println(choose(true))\n",
        "enabled:",
    );
    rejected(
        "fn add(left:Int,right:Int)->Int:left+right\nfn main():println(add(1,2))\n",
        "left:",
    );
    accepted(
        "fn choose(enabled:Bool)->Int:if enabled: 1 else: 0\nfn add(left:Int,right:Int)->Int:left+right\nfn main():println(add(right:2,left:choose(enabled:true)))\n",
    );
}

#[test]
fn generic_requirements_use_schemes_instead_of_callsite_instantiations() {
    accepted("fn first(left:a,right:b)->a:left\nfn main():println(first(1,2))\n");
    accepted("fn keep(value:a)->a:value\nfn main():println(keep(true))\n");
    rejected(
        "fn first(left:a,right:a)->a:left\nfn main():println(first(1,2))\n",
        "left:",
    );
    accepted("fn first(left:a,right:a)->a:left\nfn main():println(first(left:1,right:2))\n");
}

#[test]
fn inferred_parameter_schemes_establish_requirements_after_solving() {
    rejected(
        "fn same(left,right):left==right\nfn main():println(same(1,2))\n",
        "left:",
    );
    rejected(
        "fn choose(enabled):if enabled: 1 else: 0\nfn main():println(choose(true))\n",
        "enabled:",
    );
    accepted("fn first(left,right):left\nfn main():println(first(true,true))\n");
}

#[test]
fn aliases_expand_but_distinct_newtypes_preserve_identity() {
    rejected(
        "type Count=Int\nfn add(left:Count,right:Int)->Int:left+right\nfn main():println(add(1,2))\n",
        "left:",
    );
    accepted(
        "newtype Flag=Flag(Bool)\nnewtype Other=Other(Bool)\nfn choose(first:Flag,second:Other)->Bool:first.0\nfn main():println(choose(Flag(true),Other(false)))\n",
    );
    rejected(
        "newtype Flag=Flag(Bool)\nfn choose(first:Flag,second:Flag)->Bool:first.0\nfn main():println(choose(Flag(true),Flag(false)))\n",
        "first:",
    );
}

#[test]
fn structural_callable_native_and_constructor_calls_remain_positional() {
    accepted(
        "fn add(left:Int,right:Int)->Int:left+right\nfn invoke(call:(Int,Int)->Int)->Int:call(1,2)\nfn main():\n    println(invoke(add))\n    println(String.slice(\"abc\",0,2))\n    match Some(true):\n        Some(value)->println(value)\n        None->()\n",
    );
}

#[test]
fn required_pattern_positions_need_explicit_stable_external_names() {
    rejected(
        "fn choose(true:Bool)->Int:1\nfn choose(false:Bool)->Int:0\nfn main():()\n",
        "external label",
    );
    accepted(
        "fn choose(enabled true:Bool)->Int:1\nfn choose(enabled false:Bool)->Int:0\nfn main():println(choose(enabled:true))\n",
    );
}

#[test]
fn mandatory_pipe_input_has_a_labeled_placeholder() {
    rejected(
        "fn add(left:Int,right:Int)->Int:left+right\nfn main():println(1 |> add(right:2))\n",
        "left:",
    );
    accepted(
        "fn add(left:Int,right:Int)->Int:left+right\nfn main():println(1 |> add(right:2,left:_))\n",
    );
}

#[test]
fn unique_non_bool_parameters_remain_positional_before_labeled_ones() {
    accepted(
        "fn describe(text:String,count:Int,enabled:Bool)->String:if enabled: text else: \"\"\nfn main():println(describe(\"ok\",3,enabled:true))\n",
    );
}

#[test]
fn hostile_label_metadata_uses_the_source_identifier_contract() {
    use fern_compiler::{Span, ast};
    for name in ["9value", "fn", "return", "", "a.b", "two words"] {
        for position in 0..3 {
            let mut program =
                parse::parse("fn f(value:Int)->Int:value\nfn unused()->Int:f(1)\n").unwrap();
            let label = Some(ast::ArgumentLabel {
                name: name.into(),
                span: Span::default(),
            });
            match position {
                0 => program.functions[0].params[0].label = label,
                1 => {
                    let ast::ExprKind::Call { args, .. } = &mut program.functions[1].body.kind
                    else {
                        panic!()
                    };
                    args[0].label = label;
                }
                _ => {
                    program.functions[1].body.kind = ast::ExprKind::Pipe {
                        value: Box::new(ast::Expr {
                            kind: ast::ExprKind::Int(1),
                            span: Span::default(),
                        }),
                        name: "f".into(),
                        args: vec![],
                        position: 0,
                        label,
                    }
                }
            }
            let error = check::check_library(&program).unwrap_err();
            assert!(
                error.message.contains("invalid argument label"),
                "{name}/{position}: {error:?}"
            );
        }
    }
    accepted("fn f(🌿 value:Int)->Int:value\nfn main():println(f(🌿:1))\n");
}

#[test]
fn keyword_labels_are_rejected_in_source_before_checking() {
    for source in ["fn f(if value:Int)->Int:value", "fn main():f(if:1)"] {
        assert!(
            parse::parse(source)
                .unwrap_err()
                .message
                .contains("argument label")
        );
    }
}
