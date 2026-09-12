use super::*;
#[test]
fn strict_key_proof_equals_independent_finite_set_intersection() {
    let names = ["a", "b", "c"];
    for allowed_a in 0..8u8 {
        for required_a in 0..8u8 {
            if required_a & !allowed_a != 0 {
                continue;
            }
            for allowed_b in 0..8u8 {
                for required_b in 0..8u8 {
                    if required_b & !allowed_b != 0 {
                        continue;
                    }
                    let keys = |allowed: u8, required: u8| {
                        names
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| allowed & (1 << i) != 0)
                            .map(|(i, name)| Key {
                                name,
                                required: required & (1 << i) != 0,
                            })
                            .collect::<Vec<_>>()
                    };
                    let overlap = (0..8u8).any(|input| {
                        input & !allowed_a == 0
                            && input & required_a == required_a
                            && input & !allowed_b == 0
                            && input & required_b == required_b
                    });
                    let a = Shape::Object(keys(allowed_a, required_a));
                    let b = Shape::Object(keys(allowed_b, required_b));
                    for (left, right) in [(&a, &b), (&b, &a)] {
                        assert_eq!(
                            compare::disjoint(left, right, &mut 0, Span::default()).unwrap(),
                            !overlap
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn map_domain_is_not_an_empty_strict_record() {
    let record = Shape::Object(vec![Key {
        name: "id",
        required: true,
    }]);
    assert!(!compare::disjoint(&Shape::Map, &record, &mut 0, Span::default()).unwrap());
    assert!(
        compare::disjoint(&Shape::Object(Vec::new()), &record, &mut 0, Span::default()).unwrap()
    );
}
#[test]
fn every_profile_propagation_uses_the_original_allowance() {
    let nodes = vec![
        Node::leaf(Shape::Number),
        Node::leaf(Shape::String),
        Node::follow(vec![0, 1], false, true),
    ];
    let mut work = super::super::MAX_WORK - 11;
    assert!(
        validate(&nodes, &mut work, Span::default())
            .unwrap_err()
            .message
            .contains("work limit")
    );
    let mut measured = 0;
    validate(&nodes, &mut measured, Span::default()).unwrap();
    let mut work = super::super::MAX_WORK - measured + 1;
    assert!(
        validate(&nodes, &mut work, Span::default())
            .unwrap_err()
            .message
            .contains("work limit")
    );
}
#[test]
fn symbolic_unknown_does_not_hide_a_concrete_overlapping_pair() {
    let nodes = vec![
        Node::leaf(Shape::Unknown),
        Node::leaf(Shape::Number),
        Node::leaf(Shape::Number),
        Node::follow(vec![0, 1, 2], false, true),
    ];
    assert!(
        validate(&nodes, &mut 0, Span::default())
            .unwrap_err()
            .message
            .contains("not provably disjoint")
    );
}
