use morrow_compiler::{
    Span, Type, ir,
    json_codec::{Entry, Field, Kind, Plan},
};
fn scalar() -> Plan {
    Plan {
        root: 0,
        entries: vec![Entry {
            ty: Type::Int,
            kind: Kind::Int,
        }],
    }
}
#[test]
fn concrete_plan_validates_primitives_and_child_first_storage() {
    let mut plan = scalar();
    plan.entries.push(Entry {
        ty: Type::List(Box::new(Type::Int)),
        kind: Kind::List(0),
    });
    plan.root = 1;
    plan.validate(&[], Span::default()).unwrap();
}
#[test]
fn public_plan_rejects_cycles_unknown_slots_and_hidden_types() {
    for kind in [Kind::List(0), Kind::List(9)] {
        let mut plan = scalar();
        plan.entries[0].kind = kind;
        assert!(plan.validate(&[], Span::default()).is_err());
    }
    for ty in [Type::Infer(0), Type::Generic("a".into()), Type::Never] {
        let mut plan = scalar();
        plan.entries.push(Entry {
            ty,
            kind: Kind::Int,
        });
        assert!(plan.validate(&[], Span::default()).is_err());
    }
}
#[test]
fn public_plan_rejects_mismatched_record_and_nullable_option_storage() {
    let ty = Type::Named("Box".into(), vec![]);
    let mut plan = scalar();
    plan.entries.push(Entry {
        ty: ty.clone(),
        kind: Kind::Record(vec![Field {
            name: "value".into(),
            index: 0,
            codec: 0,
            optional: false,
        }]),
    });
    plan.root = 1;
    assert!(plan.validate(&[], Span::default()).is_err());
    let mut layout = ir::TypeLayout {
        variant_names: Vec::new(),
        ty,
        storage: ir::LayoutStorage::Tagged,
        fields: vec!["value".into()],
        variants: vec![vec![Type::Int]],
    };
    plan.validate(std::slice::from_ref(&layout), Span::default())
        .unwrap();
    layout.storage = ir::LayoutStorage::Unboxed;
    assert!(plan.validate(&[layout], Span::default()).is_err());
    let plan = Plan {
        root: 1,
        entries: vec![
            Entry {
                ty: Type::Unit,
                kind: Kind::Unit,
            },
            Entry {
                ty: Type::Option(Box::new(Type::Unit)),
                kind: Kind::Option(0),
            },
        ],
    };
    assert!(plan.validate(&[], Span::default()).is_err());
}
#[test]
fn unused_plan_entries_share_the_aggregate_work_budget() {
    let ty = Type::Tuple(vec![Type::Int; 300]);
    let entry = Entry {
        ty,
        kind: Kind::Tuple(vec![0; 300]),
    };
    let mut plan = scalar();
    plan.entries.extend(vec![entry; 2000]);
    assert!(
        plan.validate(&[], Span::default())
            .unwrap_err()
            .message
            .contains("work limit")
    );
}

#[test]
fn phantom_metadata_never_enables_unsupported_executable_entries() {
    for arg in [
        Type::Function(vec![Type::Int], Box::new(Type::Int)),
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
    ] {
        let ty = Type::Named("Phantom".into(), vec![arg.clone()]);
        let layout = ir::TypeLayout {
            variant_names: Vec::new(),
            ty: ty.clone(),
            storage: ir::LayoutStorage::Tagged,
            fields: vec!["value".into()],
            variants: vec![vec![Type::Int]],
        };
        let mut plan = scalar();
        plan.entries.push(Entry {
            ty,
            kind: Kind::Record(vec![Field {
                name: "value".into(),
                index: 0,
                codec: 0,
                optional: false,
            }]),
        });
        plan.root = 1;
        plan.validate(std::slice::from_ref(&layout), Span::default())
            .unwrap();
        let mut stored = layout.clone();
        stored.variants[0][0] = arg.clone();
        assert!(plan.validate(&[stored], Span::default()).is_err());
        plan.entries.push(Entry {
            ty: arg,
            kind: Kind::Int,
        });
        assert!(plan.validate(&[layout], Span::default()).is_err());
    }
}
