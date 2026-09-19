//! Canonical bounded reasons, retained independently of sender heaps.
use super::*;

#[derive(Clone)]
pub(super) enum Reason {
    Builtin(i64, i64),
    Text(i64, Arc<Text>),
}
pub(super) struct Text {
    bytes: Box<[u8]>,
    charge: usize,
    budget: Option<Arc<budget::Budget>>,
    local: Arc<controls::Counter>,
    _accounting: memory::ControlAllocation,
}
impl Drop for Text {
    fn drop(&mut self) {
        if let Some(budget) = &self.budget {
            budget.release_bytes(self.charge);
        } else {
            assert!(self.local.fetch_sub(self.charge, Ordering::AcqRel) >= self.charge);
        }
    }
}
impl Reason {
    pub fn tag(&self) -> i64 {
        match self {
            Self::Builtin(tag, _) | Self::Text(tag, _) => *tag,
        }
    }
    pub fn text_bytes(&self) -> usize {
        match self {
            Self::Builtin(..) => 0,
            Self::Text(_, text) => text.bytes.len(),
        }
    }
}
#[derive(Debug)]
pub(super) enum Error {
    Invalid,
    Oversize,
    ResourceLimit,
}

pub(super) unsafe fn retained_bytes(s: *mut Session) -> usize {
    unsafe {
        relations::existing(s).map_or(0, |registry| registry.reason_bytes.load(Ordering::Acquire))
    }
}

pub(super) unsafe fn read(s: *mut Session, value: i64) -> Result<Reason, Error> {
    unsafe {
        if value == 0 {
            return Err(Error::Invalid);
        }
        let value = value as *const i64;
        let tag = *value;
        if !(0..=7).contains(&tag) {
            return Err(Error::Invalid);
        }
        if tag == 3 {
            return Ok(Reason::Builtin(tag, *value.add(1)));
        }
        if !matches!(tag, 2 | 4) {
            return Ok(Reason::Builtin(tag, 0));
        }
        let pointer = *value.add(1) as *const u8;
        if pointer.is_null() {
            return Err(Error::Invalid);
        }
        let mut len = 0;
        while len <= 4096 && *pointer.add(len) != 0 {
            len += 1;
        }
        if len > 4096 {
            return Err(Error::Oversize);
        }
        let bytes = std::slice::from_raw_parts(pointer, len + 1);
        if std::str::from_utf8(&bytes[..len]).is_err() {
            return Err(Error::Invalid);
        }
        let registry = relations::registry(s);
        let charge = std::mem::size_of::<Text>() + 16 + bytes.len();
        let budget = shared(s).map(|g| Arc::clone(&g.budget));
        if let Some(budget) = &budget {
            let Some(reservation) = budget.try_charge(charge) else {
                return Err(Error::ResourceLimit);
            };
            reservation.commit();
        } else {
            let total = (*s)
                .retained
                .checked_add(registry.reason_bytes.load(Ordering::Acquire))
                .and_then(|total| total.checked_add(charge));
            if total.is_none_or(|total| total > BYTES) {
                return Err(Error::ResourceLimit);
            }
            registry.reason_bytes.fetch_add(charge, Ordering::AcqRel);
        }
        Ok(Reason::Text(
            tag,
            Arc::new(Text {
                bytes: bytes.into(),
                charge,
                budget,
                local: Arc::clone(&registry.reason_bytes),
                _accounting: memory::account_control(charge, 2),
            }),
        ))
    }
}

pub(super) unsafe fn materialize(reason: &Reason) -> i64 {
    unsafe {
        let mut roots = Box::new([0_usize; 2]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let native = memory::alloc(16, false).cast::<i64>();
        *native = reason.tag();
        *native.add(1) = 0;
        roots[0] = native as usize;
        process::construction_safepoint();
        match reason {
            Reason::Builtin(_, code) => *native.add(1) = *code,
            Reason::Text(_, text) => {
                let string = memory::alloc(text.bytes.len(), true);
                std::ptr::copy_nonoverlapping(text.bytes.as_ptr(), string, text.bytes.len());
                roots[1] = string as usize;
                process::construction_safepoint();
                *native.add(1) = string as i64;
            }
        }
        native as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
        2
    }

    #[test]
    fn canonical_reason_boundaries_and_extracted_string_survive_precise_collection() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let callback = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&callback as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            let s = (*exec).session;
            for tag in [0, 1, 5, 6, 7] {
                let native = [tag, 0];
                let reason = read(s, native.as_ptr() as i64).unwrap();
                assert_eq!(reason.tag(), tag);
            }
            for code in [i64::MIN, i64::MAX] {
                let native = [3, code];
                let reason = read(s, native.as_ptr() as i64).unwrap();
                let materialized = materialize(&reason) as *const i64;
                assert_eq!(*materialized, 3);
                assert_eq!(*materialized.add(1), code);
            }
            assert!(matches!(
                read(s, [8_i64, 0].as_ptr() as i64),
                Err(Error::Invalid)
            ));
            let invalid_utf8 = [0xff_u8, 0];
            assert!(matches!(
                read(s, [4, invalid_utf8.as_ptr() as i64].as_ptr() as i64),
                Err(Error::Invalid)
            ));
            let unicode = std::ffi::CString::new("é".repeat(2048)).unwrap();
            let unicode_reason = read(s, [2, unicode.as_ptr() as i64].as_ptr() as i64).unwrap();
            assert_eq!(unicode_reason.text_bytes(), 4097);
            drop(unicode_reason);
            for bytes in [4095, 4096, 4097] {
                let text = std::ffi::CString::new("x".repeat(bytes)).unwrap();
                let native = [4, text.as_ptr() as i64];
                let parsed = read(s, native.as_ptr() as i64);
                if bytes > 4096 {
                    assert!(matches!(parsed, Err(Error::Oversize)));
                    continue;
                }
                let reason = parsed.unwrap();
                drop(text);
                process::COLLECT_CONSTRUCTION.with(|enabled| enabled.set(true));
                let native = materialize(&reason) as *const i64;
                let extracted = *native.add(1) as usize;
                let root = memory::root_range(&extracted, 1);
                drop(reason);
                memory::morrow_gc_collect_precise();
                assert_eq!(
                    std::ffi::CStr::from_ptr(extracted as *const _).to_bytes(),
                    vec![b'x'; bytes]
                );
                drop(root);
                process::COLLECT_CONSTRUCTION.with(|enabled| enabled.set(false));
            }
            assert_eq!(retained_bytes(s), 0);
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
        }
    }

    #[test]
    fn retained_reason_can_release_after_close_on_another_thread_without_touching_session() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let callback = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&callback as *const Function];
        for schedulers in [1, 2] {
            let mut fault = 0;
            unsafe {
                let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
                if schedulers > 1 {
                    assert_eq!(morrow_managed_parallel(exec, schedulers), 0);
                }
                let s = (*exec).session;
                let registry = relations::registry(s);
                let group = shared(s).map(|_| shared_arc(s));
                let baseline = group.as_ref().map(|g| g.budget.retained());
                let mut frame = [done as *const () as usize];
                let sender = morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0)
                    .cast::<Pid>();
                let text = std::ffi::CString::new("retained after invocation").unwrap();
                let native = [2, text.as_ptr() as i64];
                let reason = read(s, native.as_ptr() as i64).unwrap();
                drop(text);
                scheduler::finish((*sender).actor);
                morrow_managed_close(exec);
                let releaser = std::thread::spawn(move || {
                    assert_eq!(reason.tag(), 2);
                    drop(reason);
                });
                releaser.join().unwrap();
                assert_eq!(registry.reason_bytes.load(Ordering::Acquire), 0);
                if let Some(group) = group {
                    assert_eq!(group.budget.retained(), baseline.unwrap());
                }
                assert_eq!(fault, 0);
            }
        }
    }

    #[test]
    fn local_reason_charge_participates_in_every_admission_at_exact_invocation_limit() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let callback = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&callback as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            let s = (*exec).session;
            let text = std::ffi::CString::new("charged").unwrap();
            let native = [4, text.as_ptr() as i64];
            let reason = read(s, native.as_ptr() as i64).unwrap();
            let reason_bytes = retained_bytes(s);
            assert!(reason_bytes >= 8);
            let pressure = BYTES - (*s).retained - reason_bytes;
            assert!(charge(s, pressure));
            assert!(!charge(s, 1));
            assert!(reserve_actor(s, 1).is_none());
            assert!(matches!(
                read(s, native.as_ptr() as i64),
                Err(Error::ResourceLimit)
            ));
            assert!(
                read(s, [5_i64, 0].as_ptr() as i64).is_ok(),
                "builtin Kill needs no reason storage"
            );
            release(s, pressure);
            drop(reason);
            assert_eq!(retained_bytes(s), 0);
            morrow_managed_close(exec);
        }
    }
}
