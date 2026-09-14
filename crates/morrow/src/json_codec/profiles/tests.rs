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

#[test]
fn nested_record_proof_matches_exhaustive_independent_finite_wire_intersections() {
    // Each field is absent, required Number/String/Bool, or optional of those.
    // Enumerate all 49 schemas on each side and all 25 tiny JSON objects.
    for a in 0..49usize {
        for b in 0..49usize {
            let left = [a % 7, a / 7];
            let right = [b % 7, b / 7];
            let accepts = |schema: [usize; 2], values: [usize; 2]| {
                schema
                    .into_iter()
                    .zip(values)
                    .all(|(field, value)| match field {
                        0 => value == 0, // absent field
                        1..=3 => value == field + 1,
                        _ => value <= 1 || value == field - 2, // absent/null or payload
                    })
            };
            let overlap = (0..25usize).any(|input| {
                let value = [input % 5, input / 5];
                accepts(left, value) && accepts(right, value)
            });
            let mut nodes = vec![
                Node::leaf(Shape::Number),
                Node::leaf(Shape::String),
                Node::leaf(Shape::Bool),
            ];
            for id in 0..3 {
                nodes.push(Node::follow(vec![id], true, false));
            }
            for schema in [left, right] {
                let mut keys = Vec::new();
                let mut children = Vec::new();
                for (name, field) in ["a", "b"].into_iter().zip(schema) {
                    if field == 0 {
                        continue;
                    }
                    keys.push(Key {
                        name,
                        required: field <= 3,
                    });
                    children.push(field - 1);
                }
                let mut node = Node::leaf(Shape::Object(keys));
                node.children = children;
                nodes.push(node);
            }
            nodes.push(Node::follow(vec![6, 7], false, true));
            assert_eq!(
                validate(&nodes, &mut 0, Span::default()).is_ok(),
                !overlap,
                "schemas {left:?} and {right:?}"
            );
        }
    }
}
