use super::registry::Registry;

#[test]
fn slot_reuse_issues_a_fresh_generation_that_invalidates_the_old_identity() {
    let mut registry = Registry::new();
    let first = registry.issue(0).expect("capacity");
    assert_eq!(first.slot, 0);
    assert_eq!(
        first.generation, 1,
        "generations start at 1 so 0 stays invalid"
    );

    registry.retire(first.slot);
    assert!(
        registry.resolve(first.slot, first.generation).is_none(),
        "a retired identity must not resolve"
    );

    let second = registry.issue(0).expect("capacity");
    assert_eq!(second.slot, first.slot, "the vacant slot is reused");
    assert!(
        second.generation > first.generation,
        "reuse must not resurrect the old identity"
    );
    assert!(
        registry.resolve(first.slot, first.generation).is_none(),
        "the old generation stays invalid after the slot is reoccupied"
    );
    assert_eq!(
        registry
            .resolve(second.slot, second.generation)
            .map(|e| e.scheduler),
        Some(0)
    );
}

#[test]
fn placement_scheduler_is_recorded_per_identity() {
    let mut registry = Registry::new();
    let a = registry.issue(0).expect("capacity");
    let b = registry.issue(3).expect("capacity");
    assert_eq!(registry.resolve(a.slot, a.generation).unwrap().scheduler, 0);
    assert_eq!(registry.resolve(b.slot, b.generation).unwrap().scheduler, 3);
    assert_eq!(registry.live(), 2);
}

#[test]
fn exhausted_generations_refuse_to_issue_rather_than_wrapping() {
    let mut registry = Registry::new();
    registry.force_generation_for_test(u64::MAX);
    assert!(
        registry.issue(0).is_none(),
        "generation exhaustion must refuse, matching the old next_id admission check"
    );
}
