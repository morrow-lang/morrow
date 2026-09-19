//! Immutable private request authority and explicit owner-side teardown.
use super::super::*;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RequestRegistration {
    pub opcode: i64,
    pub request: *const Type,
    pub result: *const Type,
    pub resume: *const Function,
}

/// Register one complete immutable invocation request table.
/// # Safety
/// Table, records, descriptors and callbacks remain immutable/readable through joins.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_supervisor_register(
    exec: *mut Exec,
    _table: *const *const RequestRegistration,
    _count: i64,
) -> i64 {
    unsafe { fail(exec, 11); }
    3
}

#[cfg(test)]
unsafe fn count(_s: *mut Session) -> usize { 0 }

#[cfg(test)]
mod tests {
    use super::*;
    struct Schema {
        types: Vec<Box<Type>>,
        children: Vec<Box<[*const Type]>>,
        arities: Vec<Box<[i64]>>,
        request: *const Type,
        result: *const Type,
    }
    impl Schema {
        fn leaf(&mut self, kind: i64) -> *const Type {
            self.types.push(Box::new(Type { kind, count: 0, children: null(), arities: null() }));
            &**self.types.last().unwrap()
        }
        fn sum(&mut self, arities: &[i64], children: &[*const Type]) -> *const Type {
            self.children.push(children.to_vec().into_boxed_slice());
            self.arities.push(arities.to_vec().into_boxed_slice());
            self.types.push(Box::new(Type { kind: 4, count: arities.len() as i64,
                children: self.children.last().unwrap().as_ptr(), arities: self.arities.last().unwrap().as_ptr() }));
            &**self.types.last().unwrap()
        }
        fn new() -> Self {
            let mut s = Self { types: vec![], children: vec![], arities: vec![], request: null(), result: null() };
            let scalar = s.leaf(0); let string = s.leaf(1); let handle = s.leaf(15);
            let reason = s.sum(&[0,0,1,1,1,0,0,0], &[string,scalar,string]);
            let error = s.sum(&[0,0,0,0,0,0,0,0,0,0,0,2,0], &[string,reason]);
            s.request = s.sum(&[1], &[handle]);
            s.result = s.sum(&[1,1], &[scalar,error]);
            s
        }
        fn stop(&self, resume: *const Function) -> RequestRegistration {
            RequestRegistration { opcode: 3, request: self.request, result: self.result, resume }
        }
    }
    #[test]
    fn registration_is_idempotent_and_close_releases_metadata_with_retained_pid() {
        let f = process_tests::Fixture::new(); let schema = Schema::new();
        let record = schema.stop(null()); let table = [&record as *const RequestRegistration];
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault); let s = (*exec).session; let before = (*s).retained;
            let status = morrow_supervisor_register(exec, table.as_ptr(), 1);
            let admitted = (*s).retained;
            let repeated = morrow_supervisor_register(exec, table.as_ptr(), 1);
            let repeated_bytes = (*s).retained;
            let entries = count(s);
            let mut frame = [process_tests::done as *const () as usize];
            let pid = morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &*f.scalar);
            let root = pid as usize; let _root = memory::root_range(&root, 1);
            morrow_managed_close(exec);
            assert_eq!(status, 0, "a canonical root Stop record is accepted");
            assert_eq!(repeated, 0); assert!(admitted > before);
            assert_eq!(repeated_bytes, admitted); assert_eq!(entries, 1);
            assert_eq!((*s).retained, before, "close refunds metadata although PID retains Session");
            assert_eq!(count(s), 0); assert_eq!(fault, 0);
        }
    }
    #[test]
    fn invalid_last_record_publishes_no_prefix_and_correct_retry_succeeds() {
        let f = process_tests::Fixture::new(); let schema = Schema::new();
        let valid = schema.stop(null()); let invalid = RequestRegistration { opcode: 99, ..valid };
        let bad = [&valid as *const RequestRegistration, &invalid]; let good = [&valid as *const RequestRegistration];
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault); let s = (*exec).session; let before = (*s).retained;
            assert_eq!(morrow_supervisor_register(exec, bad.as_ptr(), 2), 3);
            assert_eq!(fault, 11); assert_eq!(count(s), 0); assert_eq!((*s).retained, before);
            fault = 0;
            let status = morrow_supervisor_register(exec, good.as_ptr(), 1);
            morrow_managed_close(exec);
            assert_eq!(status, 0, "invalid suffix must not freeze a valid prefix");
            assert_eq!(fault, 0); assert_eq!((*s).retained, before);
        }
    }
    #[test]
    fn registration_survives_both_parallel_configuration_orders() {
        for first in [true,false] {
            let f = process_tests::Fixture::new(); let schema = Schema::new();
            let record = schema.stop(f.functions[0]); let table = [&record as *const RequestRegistration];
            let mut fault = 0;
            unsafe {
                let exec = f.open(&mut fault); let s = (*exec).session;
                if first { assert_eq!(morrow_managed_parallel(exec, 2), 0); }
                let registered = morrow_supervisor_register(exec, table.as_ptr(), 1);
                // Keep teardown deterministic even on the intentionally failing baseline.
                if registered == 0 && !first { assert_eq!(morrow_managed_parallel(exec, 2), 0); }
                let entries = count(s);
                morrow_managed_close(exec);
                assert_eq!(registered, 0); assert_eq!(entries, 1); assert_eq!(count(s), 0);
                assert_eq!(fault, 0);
            }
        }
    }
}
