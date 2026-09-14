//! The resumable sort yields comparisons to its driver and reproduces a stable order.
use morrow_runtime::{abi, collections::*};

/// Drive the state machine with a host comparator on `(key, original index)` words.
unsafe fn sort_with(values: &[i64], greater: impl Fn(i64, i64) -> bool) -> (Vec<i64>, usize) {
    let list = abi::list(values);
    let state = morrow_sort_begin(values.len() as i64);
    let mut comparisons = 0;
    // SAFETY: the state and list stay live; every pair index is below the list length.
    unsafe {
        loop {
            let pair = morrow_sort_next(state);
            if pair < 0 {
                break;
            }
            let (left, right) = ((pair >> 32) as usize, (pair & 0xffff_ffff) as usize);
            comparisons += 1;
            let ordering = if greater(values[left], values[right]) {
                1
            } else {
                0
            };
            morrow_sort_report(state, ordering);
        }
        let sorted = morrow_sort_finish(state, list);
        (elements(sorted).to_vec(), comparisons)
    }
}

#[test]
fn sort_state_is_stable_and_bounded_by_n_log_n_comparisons() {
    // Keys in the high bits, original positions in the low bits: stability is observable.
    let keys = [5, 3, 5, 1, 3, 9, 0, 5, 1, 2, 7, 7, 4];
    let values: Vec<i64> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (key << 8) | index as i64)
        .collect();
    // SAFETY: host-driven comparisons over live values.
    let (sorted, comparisons) = unsafe { sort_with(&values, |a, b| (a >> 8) > (b >> 8)) };
    let mut expected = values.clone();
    expected.sort_by_key(|value| value >> 8);
    assert_eq!(sorted, expected);
    assert!(comparisons <= values.len() * 4, "{comparisons} comparisons");
}

#[test]
fn sort_state_handles_empty_singleton_and_reversed_inputs() {
    // SAFETY: host-driven comparisons over live values.
    unsafe {
        assert_eq!(sort_with(&[], |a, b| a > b).0, Vec::<i64>::new());
        assert_eq!(sort_with(&[7], |a, b| a > b), (vec![7], 0));
        let reversed: Vec<i64> = (0..100).rev().collect();
        assert_eq!(
            sort_with(&reversed, |a, b| a > b).0,
            (0..100).collect::<Vec<i64>>()
        );
        assert_eq!(
            sort_with(&[1, 2, 3, 4], |a, b| a < b).0,
            vec![4, 3, 2, 1],
            "a descending comparator reverses"
        );
        assert_eq!(
            sort_with(&[3, 1, 2], |_, _| false).0,
            vec![3, 1, 2],
            "an always-equal comparator keeps input order"
        );
    }
}
