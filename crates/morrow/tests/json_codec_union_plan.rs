//! Every inactive union plan is checked without attempting a candidate decoder.
use morrow_compiler::{
    Span, Type,
    json_codec::{Entry, Kind, Plan},
};
fn union(a: Type, b: Type) -> Type {
    let mut members = vec![a, b];
    members.sort();
    Type::Union(members)
}
#[test]
fn inactive_number_overlap_cannot_hide_behind_an_unrelated_scalar_root() {
    let plan = Plan {
        root: 0,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
            Entry {
                ty: Type::Float,
                kind: Kind::Float,
            },
            Entry {
                ty: union(Type::Int, Type::Float),
                kind: Kind::Union(vec![0, 1]),
            },
        ],
    };
    assert!(
        plan.validate(&[], Span::default())
            .unwrap_err()
            .message
            .contains("not provably disjoint")
    );
}
#[test]
fn union_child_membership_order_duplicates_and_slots_are_independently_validated() {
    let plan = Plan {
        root: 2,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
            Entry {
                ty: Type::String,
                kind: Kind::String,
            },
            Entry {
                ty: union(Type::Int, Type::String),
                kind: Kind::Union(vec![0, 1]),
            },
        ],
    };
    plan.validate(&[], Span::default()).unwrap();
    for ids in [vec![], vec![0], vec![0, 0], vec![1, 0], vec![0, 99]] {
        let mut wrong = plan.clone();
        wrong.entries[2].kind = Kind::Union(ids);
        assert!(wrong.validate(&[], Span::default()).is_err());
    }
    let mut wrong = plan;
    wrong.entries[2].ty = Type::Union(vec![Type::Int, Type::Int]);
    assert!(wrong.validate(&[], Span::default()).is_err());
}
#[test]
fn inactive_overlapping_array_domains_do_not_execute_a_probe_to_pick_a_member() {
    let a = Type::List(Box::new(Type::Int));
    let b = Type::List(Box::new(Type::String));
    let plan = Plan {
        root: 0,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
            Entry {
                ty: Type::String,
                kind: Kind::String,
            },
            Entry {
                ty: a.clone(),
                kind: Kind::List(0),
            },
            Entry {
                ty: b.clone(),
                kind: Kind::List(1),
            },
            Entry {
                ty: union(a, b),
                kind: Kind::Union(vec![2, 3]),
            },
        ],
    };
    assert!(
        plan.validate(&[], Span::default())
            .unwrap_err()
            .message
            .contains("not provably disjoint")
    );
}

#[test]
fn distinct_nominal_sum_owners_cannot_turn_identical_source_tags_into_discriminators() {
    use morrow_compiler::{
        ir::{LayoutStorage, TypeLayout},
        json_codec::Variant,
    };
    let a = Type::Named("left.Event".into(), vec![]);
    let b = Type::Named("right.Event".into(), vec![]);
    let layouts = [a.clone(), b.clone()]
        .into_iter()
        .map(|ty| TypeLayout {
            ty,
            storage: LayoutStorage::Tagged,
            variants: vec![vec![]],
            variant_names: vec!["Ready".into()],
            fields: vec![],
        })
        .collect::<Vec<_>>();
    let kind = Kind::Sum(vec![Variant {
        wire_tag: "Ready".into(),
        fields: vec![],
    }]);
    let plan = Plan {
        root: 0,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Kind::Int,
            },
            Entry {
                ty: a.clone(),
                kind: kind.clone(),
            },
            Entry {
                ty: b.clone(),
                kind,
            },
            Entry {
                ty: union(a, b),
                kind: Kind::Union(vec![1, 2]),
            },
        ],
    };
    assert!(
        plan.validate(&layouts, Span::default())
            .unwrap_err()
            .message
            .contains("not provably disjoint")
    );
}
