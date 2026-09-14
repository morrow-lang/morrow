use morrow_compiler::{
    Span, Type, check, ir,
    json_codec::{Entry, Kind, Plan, Variant},
    parse,
};
fn source() -> ir::Program {
    check::check(&parse::parse("type Chain derive(Json):\n    End\n    Link(Int, Chain)\nfn main() -> Result(Unit, json.Error):\n    println(json.encode(Link(1, End))?)\n    Ok(())\n").unwrap()).unwrap()
}
#[test]
fn sums_publish_independent_source_names_and_exact_children() {
    let program = source();
    let layout = program
        .types
        .iter()
        .find(|t| matches!(&t.ty,Type::Named(n,_) if n=="Chain"))
        .unwrap();
    assert_eq!(layout.variant_names, ["End", "Link"]);
    let plan = Plan {
        root: 0,
        entries: vec![
            Entry {
                ty: layout.ty.clone(),
                kind: Kind::Sum(vec![
                    Variant {
                        wire_tag: "End".into(),
                        fields: vec![],
                    },
                    Variant {
                        wire_tag: "Link".into(),
                        fields: vec![1, 0],
                    },
                ]),
            },
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
        ],
    };
    plan.validate(&program.types, Span::default()).unwrap();
    for wrong in ["Other", "End", "Link\0", ""] {
        let mut forged = plan.clone();
        let Kind::Sum(variants) = &mut forged.entries[0].kind else {
            panic!()
        };
        variants[1].wire_tag = wrong.into();
        assert!(forged.validate(&program.types, Span::default()).is_err());
    }
    let mut wrong = plan.clone();
    let Kind::Sum(variants) = &mut wrong.entries[0].kind else {
        panic!()
    };
    variants[1].fields.reverse();
    assert!(wrong.validate(&program.types, Span::default()).is_err());
    let mut layouts = program.types.clone();
    layouts
        .iter_mut()
        .find(|t| t.ty == plan.entries[0].ty)
        .unwrap()
        .storage = ir::LayoutStorage::Unboxed;
    assert!(plan.validate(&layouts, Span::default()).is_err());
}
#[test]
fn every_variant_is_checked_even_with_a_finite_nullary_base() {
    for bad in ["Result(Int,String)", "(Int)->Int", "Bad"] {
        let extra = if bad == "Bad" {
            "type Bad derive(Json):\n    Again(Bad)\n"
        } else {
            ""
        };
        let s = format!(
            "{extra}type Choice derive(Json):\n    End\n    Hidden({bad})\nfn main(): ()\n"
        );
        assert!(check::check(&parse::parse(&s).unwrap()).is_err(), "{bad}");
    }
}

#[test]
fn inactive_sum_entries_names_and_storage_are_validated_before_emission() {
    let program = source();
    let mut layout = program
        .types
        .iter()
        .find(|t| matches!(&t.ty,Type::Named(n,_) if n=="Chain"))
        .unwrap()
        .clone();
    let mut plan = Plan {
        root: 0,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
            Entry {
                ty: layout.ty.clone(),
                kind: Kind::Sum(vec![
                    Variant {
                        wire_tag: "End".into(),
                        fields: vec![],
                    },
                    Variant {
                        wire_tag: "Link".into(),
                        fields: vec![0, 1],
                    },
                ]),
            },
        ],
    };
    plan.validate(&program.types, Span::default()).unwrap();
    let Kind::Sum(variants) = &mut plan.entries[1].kind else {
        panic!()
    };
    variants[1].fields[0] = 99;
    assert!(plan.validate(&program.types, Span::default()).is_err());
    layout.ty = Type::Named("Inactive".into(), vec![]);
    layout.variant_names[0] = "Bad\0Name".into();
    let mut forged = program;
    forged.types.push(layout);
    assert!(
        morrow_compiler::lowering::emit(&forged)
            .unwrap_err()
            .message
            .contains("constructor")
    );
}
#[test]
fn sum_name_and_product_work_share_the_whole_plan_allowance() {
    let program = source();
    let layout = program
        .types
        .iter()
        .find(|t| matches!(&t.ty,Type::Named(n,_) if n=="Chain"))
        .unwrap();
    let plan = Plan {
        root: 0,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
            Entry {
                ty: layout.ty.clone(),
                kind: Kind::Sum(vec![Variant {
                    wire_tag: "X".repeat(400_001),
                    fields: vec![],
                }]),
            },
        ],
    };
    assert!(
        plan.validate(&program.types, Span::default())
            .unwrap_err()
            .message
            .contains("work limit")
    );
    let mut forged = program;
    forged.types[0].variant_names = vec!["X".repeat(400_001)];
    assert!(
        morrow_compiler::lowering::emit(&forged)
            .unwrap_err()
            .message
            .contains("work limit")
    );
}

#[test]
fn forged_unused_sum_derives_cannot_hide_invalid_source_wire_names() {
    for name in ["Bad\0Name", "", "bad name"] {
        let mut ast = parse::parse("type Choice derive(Json):\n    Ready\nfn main():()\n").unwrap();
        ast.types[0].variants[0].name = name.into();
        assert!(check::check(&ast).is_err(), "{name:?}");
    }
}
