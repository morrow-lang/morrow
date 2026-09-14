//! Safe, thread-local allowances for reentrant user codec calls.
//!
//! A callback temporarily owns a child allowance. Ordinary JSON operations use
//! scoped limits, and the caller debits the returned usage from its paused budget.
use crate::{DEPTH, Limits, Result, error};
use std::{
    cell::Cell,
    ops::{Deref, DerefMut},
};

#[derive(Clone, Copy)]
struct State {
    work: usize,
    allocated: usize,
    nodes: usize,
    depth: usize,
    exhausted: bool,
}
thread_local! { static CURRENT: Cell<Option<State>> = const { Cell::new(None) }; }

/// Work performed by one callback, including its nested JSON operations.
#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub work: usize,
    pub allocated: usize,
    pub nodes: usize,
    pub exhausted: bool,
}

/// Lexically scoped callback allowance. It never retains a borrowed budget pointer.
/// A live scope belongs to its current thread and cannot be transferred.
/// ```compile_fail
/// let scope = morrow_json::scope::Scope::enter(10, 10, 10).unwrap();
/// std::thread::spawn(move || drop(scope));
/// ```
pub struct Scope {
    parent: Option<State>,
    initial: State,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Scope {
    /// Restrict a callback to the remaining original operation and enclosing scope.
    pub fn enter(work: usize, allocated: usize, nodes: usize) -> Result<Self> {
        CURRENT.with(|current| {
            let parent = current.get();
            let depth = parent.map_or(0, |state| state.depth + 1);
            if depth >= DEPTH {
                mark_exhausted();
                return Err(error(4, -1));
            }
            let initial = State {
                work: parent.map_or(work, |state| work.min(state.work)),
                allocated: parent.map_or(allocated, |state| allocated.min(state.allocated)),
                nodes: parent.map_or(nodes, |state| nodes.min(state.nodes)),
                depth,
                exhausted: false,
            };
            current.set(Some(initial));
            Ok(Self {
                parent,
                initial,
                _thread: std::marker::PhantomData,
            })
        })
    }
    /// Read usage before dropping the scope and charging the caller's paused budget.
    pub fn spent(&self) -> Usage {
        CURRENT.with(|current| {
            let state = current.get().expect("live JSON callback scope");
            Usage {
                work: self.initial.work.saturating_sub(state.work),
                allocated: self.initial.allocated.saturating_sub(state.allocated),
                nodes: self.initial.nodes.saturating_sub(state.nodes),
                exhausted: state.exhausted,
            }
        })
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(self.parent));
    }
}

/// Charge expanded nodes immediately, including nested callbacks once restored.
pub fn charge_nodes(amount: usize) -> Result<()> {
    CURRENT.with(|current| {
        let Some(mut state) = current.get() else {
            return Ok(());
        };
        let result = Limits::charge(&mut state.nodes, amount).map_err(|_| error(4, -1));
        state.exhausted |= result.is_err();
        current.set(Some(state));
        result
    })
}

/// Resource exhaustion remains visible even if user code handles a nested error.
pub fn mark_exhausted() {
    CURRENT.with(|current| {
        if let Some(mut state) = current.get() {
            state.exhausted = true;
            current.set(Some(state));
        }
    });
}

/// Whether a nested operation has exhausted the currently active callback.
pub fn exhausted() -> bool {
    CURRENT.with(|current| current.get().is_some_and(|state| state.exhausted))
}

/// Owned temporary operation counters, debited from the ambient callback on drop.
/// Counters must be retired on the thread that created them.
/// ```compile_fail
/// let limits = morrow_json::scope::limits();
/// std::thread::spawn(move || drop(limits));
/// ```
pub struct ScopedLimits {
    limits: Limits,
    initial_work: usize,
    initial_allocated: usize,
    scoped: bool,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
pub fn limits() -> ScopedLimits {
    CURRENT.with(|current| {
        let state = current.get();
        let work = state.map_or(usize::MAX, |state| state.work);
        let allocated = state.map_or(usize::MAX, |state| state.allocated);
        ScopedLimits {
            limits: Limits { work, allocated },
            initial_work: work,
            initial_allocated: allocated,
            scoped: state.is_some(),
            _thread: std::marker::PhantomData,
        }
    })
}
impl ScopedLimits {
    /// Cap an operation to an evaluator allowance without counting the cap itself as spent work.
    pub fn constrain(&mut self, evaluator: &Limits) {
        self.limits.work = self.limits.work.min(evaluator.work);
        self.limits.allocated = self.limits.allocated.min(evaluator.allocated);
        self.initial_work = self.limits.work;
        self.initial_allocated = self.limits.allocated;
    }
}
impl Deref for ScopedLimits {
    type Target = Limits;
    fn deref(&self) -> &Limits {
        &self.limits
    }
}
impl DerefMut for ScopedLimits {
    fn deref_mut(&mut self) -> &mut Limits {
        &mut self.limits
    }
}
impl Drop for ScopedLimits {
    fn drop(&mut self) {
        if !self.scoped {
            return;
        }
        CURRENT.with(|current| {
            let Some(mut state) = current.get() else {
                return;
            };
            let work = self.initial_work.saturating_sub(self.limits.work);
            let allocated = self.initial_allocated.saturating_sub(self.limits.allocated);
            state.exhausted |= Limits::charge(&mut state.work, work).is_err();
            state.exhausted |= Limits::charge(&mut state.allocated, allocated).is_err();
            current.set(Some(state));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_scopes_restore_the_parent_and_debit_work_once() {
        let parent = Scope::enter(1000, 2000, 100).unwrap();
        {
            let mut limits = limits();
            Limits::charge(&mut limits.work, 17).unwrap();
        }
        charge_nodes(2).unwrap();
        let child = Scope::enter(50, 70, 5).unwrap();
        {
            let mut limits = limits();
            Limits::charge(&mut limits.work, 11).unwrap();
            Limits::charge(&mut limits.allocated, 13).unwrap();
        }
        charge_nodes(3).unwrap();
        let used = child.spent();
        drop(child);
        assert_eq!((parent.spent().work, parent.spent().nodes), (17, 2));
        {
            let mut limits = limits();
            Limits::charge(&mut limits.work, used.work).unwrap();
            Limits::charge(&mut limits.allocated, used.allocated).unwrap();
        }
        charge_nodes(used.nodes).unwrap();
        assert_eq!(
            (
                parent.spent().work,
                parent.spent().allocated,
                parent.spent().nodes
            ),
            (28, 13, 5)
        );
        drop(parent);
        assert_eq!(limits().work, usize::MAX);
    }
    #[test]
    fn caught_exhaustion_does_not_restore_allowance_or_leak_between_threads() {
        let scope = Scope::enter(10, 10, 1).unwrap();
        {
            let mut limits = limits();
            assert!(Limits::charge(&mut limits.work, 11).is_err());
        }
        assert!(charge_nodes(2).is_err());
        assert!(scope.spent().exhausted);
        assert_eq!(limits().work, 0);
        std::thread::spawn(|| assert_eq!(limits().work, usize::MAX))
            .join()
            .unwrap();
        drop(scope);
        assert_eq!(limits().work, usize::MAX);
    }

    #[test]
    fn caught_scope_depth_exhaustion_remains_visible_to_its_parent() {
        fn nested(depth: usize) {
            let scope = Scope::enter(1000, 1000, 1000).unwrap();
            if depth + 1 == DEPTH {
                assert!(Scope::enter(1000, 1000, 1000).is_err());
                assert!(scope.spent().exhausted);
            } else {
                nested(depth + 1);
            }
        }
        nested(0);
        assert!(!exhausted());
    }
}
