//! Native compatibility entry points; Rust-owned state contains no managed pointers.
use super::*;
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
fn result(value: Result<i64, i64>) -> i64 {
    match value {
        Ok(v) => abi::result_ok(v),
        Err(e) => abi::result_err(e),
    }
}
fn with(operation: impl FnOnce(&mut State) -> Result<i64, i64>) -> i64 {
    result(STATE.with(|s| operation(&mut s.borrow_mut())))
}

/// Spawn a named compatibility actor.
/// # Safety
/// Nonnull name must be readable NUL-terminated UTF-8 on the invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_actor_spawn(name: *const c_char) -> i64 {
    if name.is_null() {
        return 0;
    }
    let name = unsafe { abi::text(name) }.to_owned();
    STATE.with(|s| s.borrow_mut().spawn(name, 0))
}
/// Spawn linked to the explicitly selected live current actor.
/// # Safety
/// Nonnull name must be a readable native string on the invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_actor_spawn_link(name: *const c_char) -> i64 {
    if name.is_null() {
        return 0;
    }
    let name = unsafe { abi::text(name) }.to_owned();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let parent = s.current;
        if s.live(parent).is_none() {
            0
        } else {
            s.spawn(name, parent)
        }
    })
}
/// Select the current compatibility actor; zero clears the context.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_set_current(id: i64) -> i64 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if id == 0 || s.live(id).is_some() {
            s.current = id;
            0
        } else {
            -1
        }
    })
}
/// Read the explicit current actor context.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_self() -> i64 {
    STATE.with(|s| s.borrow().current)
}
/// Set a deterministic nonnegative supervision clock.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_clock_set(now: i64) -> i64 {
    if now < 0 {
        -1
    } else {
        STATE.with(|s| s.borrow_mut().clock = Some(now));
        0
    }
}
/// Advance deterministic supervision time without wrapping.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_clock_advance(delta: i64) -> i64 {
    if delta < 0 {
        return -1;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let now = s.now();
        s.clock = Some(now);
        if let Some(next) = now.checked_add(delta) {
            s.clock = Some(next);
            0
        } else {
            -1
        }
    })
}
/// Read deterministic or host supervision seconds.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_clock_now() -> i64 {
    STATE.with(|s| s.borrow().now())
}
/// Monitor a live worker; repeated registration is idempotent.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_monitor(parent: i64, child: i64) -> i64 {
    with(|s| s.monitor(parent, child))
}
/// Remove a monitor even after worker death.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_demonitor(parent: i64, child: i64) -> i64 {
    with(|s| {
        s.live(parent).ok_or(3i64)?;
        let i = s.index(child).ok_or(3i64)?;
        s.actors[i].monitors.retain(|id| *id != parent);
        Ok(0)
    })
}
/// Register one-for-one restart policy.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_supervise(parent: i64, child: i64, max: i64, period: i64) -> i64 {
    with(|s| s.supervise(parent, child, max, period, 1))
}
/// Register one-for-all policy among siblings with that strategy.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_supervise_one_for_all(
    parent: i64,
    child: i64,
    max: i64,
    period: i64,
) -> i64 {
    with(|s| s.supervise(parent, child, max, period, 2))
}
/// Register rest-for-one policy in sibling registration order.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_supervise_rest_for_one(
    parent: i64,
    child: i64,
    max: i64,
    period: i64,
) -> i64 {
    with(|s| s.supervise(parent, child, max, period, 3))
}
/// Copy a message into a live mailbox.
/// # Safety
/// Nonnull message must be a readable native string on the invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_actor_send(id: i64, message: *const c_char) -> i64 {
    if message.is_null() {
        return abi::result_err(3);
    }
    let message = unsafe { abi::text(message) }.to_owned();
    with(|s| s.send(id, message))
}
/// Pop a mailbox value and return a fresh managed native string.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_receive(id: i64) -> i64 {
    match STATE.with(|s| s.borrow_mut().receive(id)) {
        Ok(text) => abi::result_ok(abi::string(&text) as i64),
        Err(e) => abi::result_err(e),
    }
}
/// Stop a subtree, notify survivors, then apply the failed root's restart policy.
/// # Safety
/// Nonnull reason must be a readable native string on the invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_actor_exit(id: i64, reason: *const c_char) -> i64 {
    let reason = if reason.is_null() {
        ""
    } else {
        unsafe { abi::text(reason) }
    };
    with(|s| s.exit(id, reason))
}
/// Restart an exited identity once, preserving monitors and its direct-owner policy.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_restart(id: i64) -> i64 {
    with(|s| s.restart(id))
}
/// Number of messages in a live actor, or minus one for a dead/unknown identity.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_mailbox_len(id: i64) -> i64 {
    STATE.with(|s| {
        let s = s.borrow();
        s.live(id).map_or(-1, |i| s.actors[i].messages.len() as i64)
    })
}
/// Consume one scheduler ticket in round-robin order.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_scheduler_next() -> i64 {
    STATE.with(|s| s.borrow_mut().next())
}
/// Compatibility spelling for spawn.
/// # Safety
/// Same native string contract as fern_actor_spawn.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_actor_start(name: *const c_char) -> i64 {
    unsafe { fern_actor_spawn(name) }
}
/// Compatibility spelling for send.
/// # Safety
/// Same native string contract as fern_actor_send.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_actor_post(id: i64, message: *const c_char) -> i64 {
    unsafe { fern_actor_send(id, message) }
}
/// Compatibility spelling for receive.
#[unsafe(no_mangle)]
pub extern "C" fn fern_actor_next(id: i64) -> i64 {
    fern_actor_receive(id)
}
