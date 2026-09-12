use super::*;
use crate::json_codec::{Entry, Field};
fn plan() -> Plan {
    Plan {
        root: 4,
        entries: vec![
            Entry {
                ty: Type::Int,
                kind: Wire::Int,
            },
            Entry {
                ty: Type::String,
                kind: Wire::String,
            },
            Entry {
                ty: Type::Named("A".into(), vec![]),
                kind: Wire::Record(vec![Field {
                    name: "a".into(),
                    index: 0,
                    codec: 0,
                    optional: false,
                }]),
            },
            Entry {
                ty: Type::Named("B".into(), vec![]),
                kind: Wire::Record(vec![Field {
                    name: "b".into(),
                    index: 0,
                    codec: 0,
                    optional: false,
                }]),
            },
            Entry {
                ty: Type::Union(vec![
                    Type::Named("A".into(), vec![]),
                    Type::Named("B".into(), vec![]),
                ]),
                kind: Wire::Union(vec![2, 3]),
            },
        ],
    }
}
fn node(kind: Kind) -> Json {
    Rc::new(Node {
        kind,
        offset: 0,
        height: 1,
        nodes: 1,
        encoded: 3,
    })
}
#[test]
fn shallow_selection_never_allocates_candidate_payloads_or_calls_numeric_adapters() {
    let plan = plan();
    let mut limits = Limits::new(64 * 1024 * 1024);
    let budget = Budget {
        limits: &mut limits,
        work: 64 * 1024 * 1024,
        allocated: 128,
        nodes: 0,
        at: 0,
    };
    let mut execution = Execution {
        plan: &plan,
        budget,
        path: Rc::new(String::new()),
    };
    let number = node(Kind::Number("1.5".into()));
    let before = execution.budget.limits.allocated;
    assert_eq!(execution.select(&[0, 1], &number).unwrap(), 0);
    assert_eq!(
        (
            execution.budget.allocated,
            execution.budget.nodes,
            execution.budget.limits.allocated
        ),
        (128, 0, before)
    );
    let object = node(Kind::Object(
        vec![(node(Kind::String("a".into())), number)],
        vec![0],
    ));
    assert_eq!(execution.select(&[2, 3], &object).unwrap(), 0);
    assert_eq!(
        (
            execution.budget.allocated,
            execution.budget.nodes,
            execution.budget.limits.allocated
        ),
        (128, 0, before)
    );
    execution.budget.work = 3;
    let failed = execution.select(&[2, 3], &object).unwrap_err();
    assert_eq!((failed.code, failed.offset), (4, -1));
    assert_eq!(
        (
            execution.budget.allocated,
            execution.budget.nodes,
            execution.budget.limits.allocated
        ),
        (128, 0, before)
    );
}
