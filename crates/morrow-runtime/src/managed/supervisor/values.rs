//! Owned payload representations for typed supervision values.
use super::*;
use std::ffi::c_char;

#[repr(C)]
pub(in crate::managed) struct Handle {
    pub identity: process::Identity,
}

pub(in crate::managed) struct KeyToken {
    pub epoch: Arc<relations::Epoch>,
    pub serial: u64,
    pub mailbox: *const Type,
}

#[repr(C)]
pub(in crate::managed) struct ChildKey {
    pub token: *mut KeyToken,
    pub name: *mut c_char,
}

#[repr(C)]
#[derive(Default)]
pub(in crate::managed) struct ChildSpec {
    pub kind: u64,
    pub key: *mut ChildKey,
    pub name: *mut c_char,
    pub initializer: *mut c_void,
    pub children: *mut *mut ChildSpec,
    pub children_len: u64,
    pub restart: i64,
    pub shutdown_kind: i64,
    pub shutdown_ms: i64,
    pub significant: i64,
    pub strategy: i64,
    pub intensity: i64,
    pub period_seconds: i64,
    pub auto_shutdown: i64,
}

/// Maximum valid UTF-8 name size, including its trailing native NUL.
/// The ABI guarantees readable native string storage through its terminator.
pub(in crate::managed) unsafe fn name_bytes(name: *const c_char) -> Option<usize> {
    if name.is_null() {
        return None;
    }
    unsafe {
        for len in 0..=4096 {
            if *name.add(len) == 0 {
                return std::str::from_utf8(std::slice::from_raw_parts(name.cast(), len))
                    .ok()
                    .map(|_| len + 1);
            }
        }
    }
    None
}

pub(in crate::managed) unsafe fn valid_handle(s: *mut Session, value: *const Handle) -> bool {
    unsafe {
        !s.is_null()
            && !value.is_null()
            && process::valid_identity(s, &raw const (*value).identity)
            && super::is_handle_actor((*value).identity.actor)
    }
}

pub(in crate::managed) unsafe fn valid_key(
    s: *mut Session,
    key: *const ChildKey,
    mailbox: *const Type,
) -> bool {
    unsafe {
        !s.is_null()
            && !key.is_null()
            && !(*key).token.is_null()
            && (*(*key).token).serial != 0
            && !mailbox.is_null()
            && (*(*key).token).mailbox == mailbox
            && relations::existing(s)
                .is_some_and(|r| Arc::as_ptr(&r.epoch) == Arc::as_ptr(&(*(*key).token).epoch))
            && name_bytes((*key).name).is_some()
    }
}

fn policy_valid(restart: i64, shutdown: i64, ms: i64, significant: i64) -> bool {
    (0..=2).contains(&restart)
        && (0..=2).contains(&shutdown)
        && (if shutdown == 0 {
            (0..=600_000).contains(&ms)
        } else {
            ms == 0
        })
        && (0..=1).contains(&significant)
        && !(significant == 1 && restart == 0)
}
fn flags_valid(strategy: i64, intensity: i64, period: i64, auto: i64) -> bool {
    (0..=2).contains(&strategy)
        && (0..=1024).contains(&intensity)
        && (1..=86_400).contains(&period)
        && (0..=2).contains(&auto)
}

/// Check only this initialized native header. Its payload graph is traversed by
/// the caller with the same work budget; this never starts a fresh cost walk.
pub(in crate::managed) unsafe fn validate_header(
    s: *mut Session,
    pointer: *const ChildSpec,
    work: &mut usize,
) -> Result<(), ValidationError> {
    use ValidationError::{Malformed, Options};
    unsafe {
        if pointer.is_null() {
            return Err(Malformed);
        }
        if !cost::work(work) {
            return Err(Options(1));
        }
        let v = &*pointer;
        if !(0..=2).contains(&v.restart)
            || !(0..=2).contains(&v.shutdown_kind)
            || !(0..=1).contains(&v.significant)
        {
            return Err(Malformed);
        }
        if !policy_valid(v.restart, v.shutdown_kind, v.shutdown_ms, v.significant) {
            return Err(Options(0));
        }
        match v.kind {
            0 => {
                if v.key.is_null()
                    || v.initializer.is_null()
                    || !v.name.is_null()
                    || !v.children.is_null()
                    || v.children_len != 0
                    || v.strategy != 0
                    || v.intensity != 0
                    || v.period_seconds != 0
                    || v.auto_shutdown != 0
                {
                    return Err(Malformed);
                }
                let token = (*v.key).token;
                if token.is_null() {
                    return Err(Malformed);
                }
                if !valid_key(s, v.key, (*token).mailbox) {
                    return Err(Options(2));
                }
                if !cost::descriptor((*token).mailbox, false, work) {
                    return Err(Malformed);
                }
                let f = function_work(s, v.initializer, work);
                if f.is_null()
                    || (*f).step.is_none()
                    || (*f).select.is_some()
                    || (*f).mailbox != (*token).mailbox
                {
                    return Err(Malformed);
                }
            }
            1 => {
                if !v.key.is_null()
                    || !v.initializer.is_null()
                    || (v.children_len == 0) != v.children.is_null()
                {
                    return Err(Malformed);
                }
                if v.name.is_null()
                    || !(0..=2).contains(&v.strategy)
                    || !(0..=2).contains(&v.auto_shutdown)
                {
                    return Err(Malformed);
                }
                if v.children_len > 1024
                    || name_bytes(v.name).is_none()
                    || !flags_valid(v.strategy, v.intensity, v.period_seconds, v.auto_shutdown)
                {
                    return Err(Options(0));
                }
            }
            _ => return Err(Malformed),
        }
        Ok(())
    }
}

/// Owner-local transient construction charge. It cannot escape into heap metadata.
struct Charge {
    session: *mut Session,
    bytes: usize,
}
impl Charge {
    unsafe fn new(session: *mut Session, bytes: usize) -> Option<Self> {
        unsafe { charge(session, bytes).then(|| Self { session, bytes }) }
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        // SAFETY: created and dropped synchronously on this Session's owner.
        unsafe {
            release(self.session, self.bytes);
        }
    }
}

unsafe fn context(exec: *mut Exec) -> Option<*mut Session> {
    unsafe {
        (!exec.is_null()
            && !(*exec).session.is_null()
            && !(*exec).fault.is_null()
            && *(*exec).fault == 0)
            .then(|| (*exec).session)
    }
}
unsafe fn result_error(exec: *mut Exec, error: ValidationError) -> i64 {
    unsafe {
        match error {
            ValidationError::Options(tag) => process::error(tag),
            ValidationError::Malformed => {
                fail(exec, 11);
                0
            }
        }
    }
}
unsafe fn policy(pointer: *const i64) -> Result<(i64, i64, i64, i64), ValidationError> {
    unsafe {
        if pointer.is_null() || *pointer != 0 {
            return Err(ValidationError::Malformed);
        }
        let restart = *pointer.add(1) as *const i64;
        let shutdown = *pointer.add(2) as *const i64;
        if restart.is_null() || shutdown.is_null() {
            return Err(ValidationError::Malformed);
        }
        let (restart, shutdown, significant) = (*restart, *shutdown, *pointer.add(3));
        if !(0..=2).contains(&restart)
            || !(0..=2).contains(&shutdown)
            || !(0..=1).contains(&significant)
        {
            return Err(ValidationError::Malformed);
        }
        let ms = if shutdown == 0 {
            *(*pointer.add(2) as *const i64).add(1)
        } else {
            0
        };
        if !policy_valid(restart, shutdown, ms, significant) {
            return Err(ValidationError::Options(0));
        }
        Ok((restart, shutdown, ms, significant))
    }
}
unsafe fn flags(pointer: *const i64) -> Result<(i64, i64, i64, i64), ValidationError> {
    unsafe {
        if pointer.is_null() || *pointer != 0 {
            return Err(ValidationError::Malformed);
        }
        let strategy = *pointer.add(1) as *const i64;
        let auto = *pointer.add(4) as *const i64;
        if strategy.is_null() || auto.is_null() {
            return Err(ValidationError::Malformed);
        }
        let tuple = (*strategy, *pointer.add(2), *pointer.add(3), *auto);
        if !(0..=2).contains(&tuple.0) || !(0..=2).contains(&tuple.3) {
            return Err(ValidationError::Malformed);
        }
        if !flags_valid(tuple.0, tuple.1, tuple.2, tuple.3) {
            return Err(ValidationError::Options(0));
        }
        Ok(tuple)
    }
}

/// Create immutable key authority and copy its native UTF-8 name.
/// # Safety
/// Exec is owner-local; name and mailbox are valid rooted native ABI values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_supervisor_child_key(
    exec: *mut Exec,
    name: *const c_char,
    mailbox: *const Type,
) -> *mut c_void {
    unsafe {
        let Some(s) = context(exec) else {
            return null_mut();
        };
        if !cost::descriptor(mailbox, false, &mut 0) {
            fail(exec, 11);
            return null_mut();
        }
        let Some(bytes) = name_bytes(name) else {
            fail(exec, 9);
            return null_mut();
        };
        let Some(_charge) = Charge::new(
            s,
            std::mem::size_of::<ChildKey>() + std::mem::size_of::<KeyToken>() + bytes,
        ) else {
            fail(exec, 9);
            return null_mut();
        };
        let Some((epoch, serial)) = super::key_identity(s) else {
            fail(exec, 9);
            return null_mut();
        };
        let source_root = name as usize;
        let _source = memory::root_range(&source_root, 1);
        let name_copy = memory::alloc(bytes, true).cast::<c_char>();
        std::ptr::copy_nonoverlapping(name, name_copy, bytes);
        let name_root = name_copy as usize;
        let _name = memory::root_range(&name_root, 1);
        process::construction_safepoint();
        let key = allocate::<ChildKey>();
        let token = control::Owned::new(KeyToken {
            epoch,
            serial,
            mailbox,
        });
        *key = ChildKey {
            token: token.as_ptr(),
            name: name_copy,
        };
        memory::retain_control(key.cast(), token.token());
        key.cast()
    }
}

unsafe fn finish_spec(exec: *mut Exec, s: *mut Session, spec: &ChildSpec) -> i64 {
    unsafe {
        if let Err(error) = validate_header(s, spec, &mut 0) {
            return result_error(exec, error);
        }
        let Some(bytes) = cost::child_spec(s, spec) else {
            fail(exec, 11);
            return 0;
        };
        let Some(_charge) = Charge::new(s, bytes + 32) else {
            return process::error(1);
        };
        let copied = copy::child_spec(s, spec);
        process::construction_safepoint();
        abi::result_ok(copied.value)
    }
}

/// Retain an immutable copied initializer for future worker generations.
/// # Safety
/// Arguments obey the canonical rooted key, entry and ChildPolicy ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_supervisor_worker(
    exec: *mut Exec,
    key: *const c_void,
    entry: *const c_void,
    policy_value: *const i64,
) -> i64 {
    unsafe {
        let Some(s) = context(exec) else {
            return 0;
        };
        let roots = [key as usize, entry as usize, policy_value as usize];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let (restart, shutdown_kind, shutdown_ms, significant) = match policy(policy_value) {
            Ok(value) => value,
            Err(error) => return result_error(exec, error),
        };
        let spec = ChildSpec {
            key: key.cast_mut().cast(),
            initializer: entry.cast_mut(),
            restart,
            shutdown_kind,
            shutdown_ms,
            significant,
            ..ChildSpec::default()
        };
        finish_spec(exec, s, &spec)
    }
}

/// Copy a branch tree into the current owner heap after bounded validation.
/// # Safety
/// Arguments obey canonical rooted name, Flags, List(ChildSpec), ChildPolicy ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_supervisor_branch(
    exec: *mut Exec,
    name: *const c_char,
    flags_value: *const i64,
    children: *const abi::List,
    policy_value: *const i64,
) -> i64 {
    unsafe {
        let Some(s) = context(exec) else {
            return 0;
        };
        let roots = [
            name as usize,
            flags_value as usize,
            children as usize,
            policy_value as usize,
        ];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        if children.is_null()
            || (*children).len < 0
            || (*children).cap < 1
            || (*children).len > (*children).cap
            || (*children).data.is_null()
        {
            return result_error(exec, ValidationError::Malformed);
        }
        if (*children).len > 1024 || name_bytes(name).is_none() {
            return process::error(0);
        }
        let (restart, shutdown_kind, shutdown_ms, significant) = match policy(policy_value) {
            Ok(value) => value,
            Err(error) => return result_error(exec, error),
        };
        let (strategy, intensity, period_seconds, auto_shutdown) = match flags(flags_value) {
            Ok(value) => value,
            Err(error) => return result_error(exec, error),
        };
        let child_slice = std::slice::from_raw_parts(
            (*children).data.cast::<*mut ChildSpec>(),
            (*children).len as usize,
        );
        if let Err(error) = super::validate_specs(s, child_slice, auto_shutdown) {
            return result_error(exec, error);
        }
        let spec = ChildSpec {
            kind: 1,
            name: name.cast_mut(),
            children: if child_slice.is_empty() {
                null_mut()
            } else {
                (*children).data.cast()
            },
            children_len: child_slice.len() as u64,
            restart,
            shutdown_kind,
            shutdown_ms,
            significant,
            strategy,
            intensity,
            period_seconds,
            auto_shutdown,
            ..ChildSpec::default()
        };
        finish_spec(exec, s, &spec)
    }
}

/// Erase supervisor role into the ordinary retained ProcessId authority.
/// # Safety
/// Handle is a rooted native wrapper from this invocation; Exec is owner-local.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_supervisor_id(
    exec: *mut Exec,
    handle: *const c_void,
) -> *mut c_void {
    unsafe {
        let Some(s) = context(exec) else {
            return null_mut();
        };
        let handle = handle.cast::<Handle>();
        if !valid_handle(s, handle) {
            fail(exec, 11);
            return null_mut();
        }
        process::identity(s, (*handle).identity.actor).cast()
    }
}

#[cfg(test)]
#[path = "value_tests.rs"]
mod tests;
