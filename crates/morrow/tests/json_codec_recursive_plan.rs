use morrow_compiler::{
    Span, Type, ir,
    json_codec::{Entry, Field, Kind, Plan},
};
fn recursive(list: bool) -> (Plan, Vec<ir::TypeLayout>) {
    let named = Type::Named("Node".into(), vec![]);
    let payload = if list {
        Type::List(Box::new(named.clone()))
    } else {
        named.clone()
    };
    let plan = Plan {
        root: 0,
        entries: vec![
            Entry {
                ty: named.clone(),
                kind: Kind::Record(vec![Field {
                    name: "next".into(),
                    index: 0,
                    codec: 1,
                    optional: false,
                }]),
            },
            Entry {
                ty: payload.clone(),
                kind: if list {
                    Kind::List(0)
                } else {
                    Kind::Record(vec![Field {
                        name: "next".into(),
                        index: 0,
                        codec: 0,
                        optional: false,
                    }])
                },
            },
        ],
    };
    (
        plan,
        vec![ir::TypeLayout {
            variant_names: Vec::new(),
            ty: named,
            storage: ir::LayoutStorage::Tagged,
            fields: vec!["next".into()],
            variants: vec![vec![payload]],
        }],
    )
}
#[test]
fn finite_record_list_cycle_validates_forward_and_backward_edges() {
    let (plan, layouts) = recursive(true);
    plan.validate(&layouts, Span::default()).unwrap();
}
#[test]
fn strict_record_cycle_rejects_without_claiming_a_finite_value() {
    let (plan, layouts) = recursive(false);
    let error = plan.validate(&layouts, Span::default()).unwrap_err();
    assert!(error.message.contains("no finite value"), "{error:?}");
}
#[test]
fn valid_cycle_does_not_hide_an_inactive_unsupported_entry() {
    let (mut plan, layouts) = recursive(true);
    plan.entries.push(Entry {
        ty: Type::Generic("a".into()),
        kind: Kind::Int,
    });
    assert!(plan.validate(&layouts, Span::default()).is_err());
}
