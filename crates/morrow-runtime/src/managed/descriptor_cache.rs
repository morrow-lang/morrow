//! Scheduler-local immutable descriptor lookup memoization.
use super::{Function, c_void};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Entry {
    identity: *const c_void,
    descriptor: *const Function,
    index: usize,
}

/// A bounded hint, never an alternate descriptor registry. Only successful
/// lookups in this Session's immutable registration populate entries. Collisions
/// replace hints and fall back to the original bounded scan; no pointers escape.
#[repr(C)]
#[derive(Default)]
pub(super) struct Cache {
    entries: [Entry; 8],
}
impl Cache {
    fn slot(identity: *const c_void) -> usize {
        (identity as usize >> 3) & 7
    }

    pub(super) fn get(&self, identity: *const c_void) -> Option<(*const Function, usize)> {
        let entry = self.entries[Self::slot(identity)];
        (entry.identity == identity && !entry.descriptor.is_null())
            .then_some((entry.descriptor, entry.index))
    }

    pub(super) fn insert(
        &mut self,
        identity: *const c_void,
        descriptor: *const Function,
        index: usize,
    ) {
        self.entries[Self::slot(identity)] = Entry {
            identity,
            descriptor,
            index,
        };
    }
}

#[cfg(test)]
thread_local! { static COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(super) fn compared() {
    COMPARISONS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::*;
    thread_local! { static CALLBACKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
    unsafe extern "C" fn countdown(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            CALLBACKS.with(|count| count.set(count.get() + 1));
            let words = frame.cast::<i64>();
            if *words.add(1) == 1 {
                return 2;
            }
            let mut next = [*words, *words.add(1) - 1];
            morrow_managed_continue(exec, next.as_mut_ptr().cast())
        }
    }
    #[test]
    fn repeated_real_callbacks_stop_rescanning_registered_descriptors() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type];
        let identities = [0u64; 32];
        let functions: Vec<_> = identities
            .iter()
            .map(|identity| Function {
                identity: (identity as *const u64).cast(),
                step: Some(countdown),
                select: None,
                capture_count: 1,
                captures: captures.as_ptr(),
                mailbox: &scalar,
            })
            .collect();
        let table: Vec<_> = functions.iter().map(|f| f as *const Function).collect();
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, table.as_ptr(), table.len() as i64);
            assert!(!exec.is_null());
            let s = (*exec).session;
            (*s).reduction_budget = quantum::Budget::new(1).unwrap();
            let mut frame = [functions[31].identity as i64, 64];
            assert!(!morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).is_null());
            CALLBACKS.with(|count| count.set(0));
            assert!(scheduler::turn(s));
            COMPARISONS.with(|count| count.set(0));
            for _ in 0..32 {
                assert!(scheduler::turn(s));
            }
            let comparisons = COMPARISONS.with(|count| count.get());
            assert_eq!(CALLBACKS.with(|count| count.get()), 33);
            assert_eq!(fault, 0);
            morrow_managed_close(exec);
            assert_eq!(
                comparisons, 0,
                "warm callbacks must perform useful work without registry scans"
            );
        }
    }

    #[test]
    fn collisions_alternation_and_budget_failures_preserve_registry_semantics() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type];
        let identities = [0u64; 16];
        let functions: Vec<_> = identities
            .iter()
            .map(|identity| Function {
                identity: (identity as *const u64).cast(),
                step: Some(countdown),
                select: None,
                capture_count: 1,
                captures: captures.as_ptr(),
                mailbox: &scalar,
            })
            .collect();
        let table: Vec<_> = functions.iter().map(|f| f as *const Function).collect();
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, table.as_ptr(), table.len() as i64);
            let s = (*exec).session;
            for index in [0, 8, 0, 8, 1, 2, 1, 2] {
                let frame = [functions[index].identity as i64, 1];
                let mut work = 7;
                assert_eq!(
                    function_work(s, frame.as_ptr().cast(), &mut work),
                    &functions[index] as *const Function
                );
                assert_eq!(work, 7 + index + 2);
            }
            COMPARISONS.with(|count| count.set(0));
            for index in [1, 2, 1, 2] {
                let frame = [functions[index].identity as i64, 1];
                assert_eq!(
                    function(s, frame.as_ptr().cast()),
                    &functions[index] as *const Function
                );
            }
            assert_eq!(COMPARISONS.with(|count| count.get()), 0);
            let frame = [functions[15].identity as i64, 1];
            for warm in [false, true] {
                for available in 0..=18 {
                    (*s).function_cache = Cache::default();
                    if warm {
                        assert_eq!(
                            function(s, frame.as_ptr().cast()),
                            &functions[15] as *const Function
                        );
                    }
                    let mut work = WORK - available;
                    let found = function_work(s, frame.as_ptr().cast(), &mut work);
                    assert_eq!(
                        found.is_null(),
                        available < 17,
                        "warm={warm}, available={available}"
                    );
                    assert_eq!(
                        work,
                        if available < 17 {
                            WORK + 1
                        } else {
                            WORK - available + 17
                        }
                    );
                }
            }
            let unknown = [1i64];
            let mut work = 0;
            assert!(function_work(s, unknown.as_ptr().cast(), &mut work).is_null());
            assert_eq!(work, 17);
            assert!(function_work(s, null(), &mut work).is_null());
            assert_eq!(work, 17, "null frames consume no lookup work");
            assert_eq!(fault, 0);
            morrow_managed_close(exec);
        }
    }

    #[test]
    fn sessions_never_reuse_another_registrations_cached_identity() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type];
        let identity = 0u64;
        let functions: Vec<_> = (0..3)
            .map(|_| Function {
                identity: (&identity as *const u64).cast(),
                step: Some(countdown),
                select: None,
                capture_count: 1,
                captures: captures.as_ptr(),
                mailbox: &scalar,
            })
            .collect();
        let frame = [functions[0].identity as i64, 1];
        for function in &functions {
            let table = [function as *const Function];
            let mut fault = 0;
            unsafe {
                let exec = host::open_local(&mut fault, table.as_ptr(), 1);
                for _ in 0..3 {
                    assert_eq!(
                        super::super::function((*exec).session, frame.as_ptr().cast()),
                        function as *const Function
                    );
                }
                assert_eq!(fault, 0);
                morrow_managed_close(exec);
            }
        }
        let duplicate = [&functions[0] as *const Function, &functions[1]];
        let mut fault = 0;
        unsafe {
            assert!(host::open_local(&mut fault, duplicate.as_ptr(), 2).is_null());
        }
        assert_eq!(
            fault, 11,
            "warming a prior registry cannot bypass registration validation"
        );
    }

    struct Moved {
        completed: std::sync::mpsc::Sender<usize>,
    }
    unsafe extern "C" fn migrated(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let words = frame.cast::<i64>();
            let s = (*exec).session;
            // Scheduler dispatch already resolved this frame. A second lookup
            // must use this owner's hint with the original registry work charge.
            COMPARISONS.with(|count| count.set(0));
            let mut work = 0;
            let descriptor = function_work(s, frame, &mut work);
            assert!(!descriptor.is_null());
            assert_eq!((*descriptor).identity, *words as *const c_void);
            assert_eq!(work, 33);
            assert_eq!(COMPARISONS.with(|count| count.get()), 0);
            if *words.add(2) == 1 {
                let probe = &*(*words.add(1) as *const Moved);
                probe.completed.send((*s).scheduler).unwrap();
                2
            } else {
                let mut next = [*words, *words.add(1), 1];
                morrow_managed_continue(exec, next.as_mut_ptr().cast())
            }
        }
    }
    #[test]
    fn migrated_callbacks_warm_the_destination_scheduler_cache() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type, &scalar];
        let identities = [0u64; 32];
        let functions: Vec<_> = identities
            .iter()
            .map(|identity| Function {
                identity: (identity as *const u64).cast(),
                step: Some(migrated),
                select: None,
                capture_count: 2,
                captures: captures.as_ptr(),
                mailbox: &scalar,
            })
            .collect();
        let table: Vec<_> = functions.iter().map(|f| f as *const Function).collect();
        let (completed, receive) = std::sync::mpsc::channel();
        let probe = Moved { completed };
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, table.as_ptr(), table.len() as i64);
            assert_eq!(morrow_managed_parallel(exec, 2), 0);
            let s = (*exec).session;
            shared(s).unwrap().stealing.store(false, Ordering::Release);
            (*s).reduction_budget = quantum::Budget::new(1).unwrap();
            let mut frame = [
                functions[31].identity as i64,
                &probe as *const Moved as i64,
                2,
            ];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let a = (*pid).actor;
            let retained = control::Owned::retain(a);
            assert!(scheduler::turn(s));
            assert!(migration::transfer(s, a, 1));
            assert_eq!(
                receive
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap(),
                1
            );
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
            assert!(!(*retained.as_ptr()).identity.alive.load(Ordering::Acquire));
        }
    }
}
