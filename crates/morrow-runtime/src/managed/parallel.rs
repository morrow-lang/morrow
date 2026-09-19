//! Scheduler-set drivers over one transport and one invocation-wide budget.
use super::*;

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(test)]
type RetirementHook = (usize, fn(usize));

#[cfg(test)]
thread_local! {
    // Inject exactly the interleaving between importing a fault and observing stop.
    static BEFORE_STOP_OBSERVATION: std::cell::Cell<Option<unsafe fn(*mut Session)>> = const { std::cell::Cell::new(None) };
    static FAIL_WORKER_START: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static AFTER_SUPERVISED_RETIREMENT: std::cell::Cell<Option<RetirementHook>> = const { std::cell::Cell::new(None) };
    static RUN_WAITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn after_supervised_retirement() {
    if let Some((context, hook)) = AFTER_SUPERVISED_RETIREMENT.with(|hook| hook.take()) {
        hook(context);
    }
}

pub(super) struct Driver {
    shared: Arc<transport::Shared>,
    next: usize,
    workers: Vec<JoinHandle<()>>,
    idle: Arc<Vec<AtomicBool>>,
    #[cfg(test)]
    worker_exits: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(any(test, feature = "simulation"))]
    simulation: Option<Simulation>,
}

#[cfg(any(test, feature = "simulation"))]
struct SimWorker {
    domain: Box<memory::Domain>,
    exec: *mut Exec,
    _fault: Box<i64>,
}

#[cfg(any(test, feature = "simulation"))]
struct Simulation {
    workers: Vec<SimWorker>,
    random: u64,
    milliseconds: u64,
    choices: Vec<usize>,
    replay: Option<Vec<usize>>,
    cursor: usize,
}

unsafe fn driver(s: *mut Session) -> Option<*mut Driver> {
    unsafe { (*s)._parallel.as_ref().map(control::Owned::as_ptr) }
}

unsafe fn attach(s: *mut Session, group: Arc<transport::Shared>, scheduler: usize) {
    unsafe {
        (*s).session_key = Arc::as_ptr(&group) as usize;
        (*s).scheduler = scheduler;
        (*s)._shared = Some(control::Owned::new(group));
    }
}

fn start_worker(
    scheduler: usize,
    callback: impl FnOnce() + Send + 'static,
) -> std::io::Result<JoinHandle<()>> {
    #[cfg(test)]
    if FAIL_WORKER_START.with(|failure| {
        if failure.get() == Some(scheduler) {
            failure.set(None);
            true
        } else {
            false
        }
    }) {
        return Err(std::io::Error::other("injected worker startup failure"));
    }
    std::thread::Builder::new()
        .name(format!("morrow-scheduler-{scheduler}"))
        .spawn(callback)
}

/// Configure an invocation before any actor is admitted. Count includes the
/// calling scheduler; callbacks remain cooperative within each scheduler.
/// # Safety
/// Exec is rooted on this thread. Its immutable descriptor graph and callbacks
/// remain valid until stop/close has joined all workers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_parallel(exec: *mut Exec, count: i64) -> i64 {
    unsafe { configure(exec, count, None) }
}

unsafe fn configure(exec: *mut Exec, count: i64, simulated: Option<u64>) -> i64 {
    unsafe {
        if exec.is_null()
            || (*exec).session.is_null()
            || !(*exec).actor.is_null()
            || !(1..=64).contains(&count)
        {
            return 3;
        }
        let s = (*exec).session;
        if (*s).next_id != 0 || (*s).stopped || *(*exec).fault != 0 || (*s)._shared.is_some() {
            return 3;
        }
        let stealing = if simulated.is_some() {
            false
        } else {
            match migration::configured() {
                Ok(enabled) => enabled,
                Err(()) => {
                    fail(exec, 9);
                    return 3;
                }
            }
        };
        let group = transport::Shared::new(count as usize, (*s).retained);
        group.stealing.store(stealing, Ordering::Release);
        let idle = Arc::new(
            (0..count)
                .map(|_| AtomicBool::new(true))
                .collect::<Vec<_>>(),
        );
        let owner = control::Owned::new(Driver {
            shared: Arc::clone(&group),
            next: 0,
            workers: Vec::new(),
            idle: Arc::clone(&idle),
            #[cfg(test)]
            worker_exits: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            #[cfg(any(test, feature = "simulation"))]
            simulation: simulated.map(|seed| Simulation {
                workers: Vec::new(),
                random: seed.max(1),
                milliseconds: 0,
                choices: Vec::new(),
                replay: None,
                cursor: 0,
            }),
        });
        let d = owner.as_ptr();
        attach(s, Arc::clone(&group), 0);
        (*s)._parallel = Some(owner);
        #[cfg(any(test, feature = "simulation"))]
        if simulated.is_some() {
            (*s).simulation = simulation::State {
                enabled: true,
                ..Default::default()
            };
            for scheduler in 1..count as usize {
                let mut domain = Box::new(memory::Domain::new());
                let mut fault = Box::new(0);
                let child = {
                    let _active = domain.activate();
                    let child =
                        host::open_local(&mut *fault, (*s).functions, (*s).function_count as i64);
                    if !child.is_null() {
                        attach((*child).session, Arc::clone(&group), scheduler);
                        (*(*child).session).reduction_budget = (*s).reduction_budget;
                        (*(*child).session).simulation = simulation::State {
                            enabled: true,
                            ..Default::default()
                        };
                    }
                    child
                };
                if child.is_null() {
                    fail(exec, 9);
                    stop(s);
                    return 3;
                }
                (*d).simulation.as_mut().unwrap().workers.push(SimWorker {
                    domain,
                    exec: child,
                    _fault: fault,
                });
            }
            return 0;
        }
        #[cfg(not(any(test, feature = "simulation")))]
        debug_assert!(simulated.is_none());
        let reduction_budget = (*s).reduction_budget;
        let functions = (*s).functions as usize;
        let function_count = (*s).function_count;
        for scheduler in 1..count as usize {
            let group = Arc::clone(&group);
            let idle = Arc::clone(&idle);
            #[cfg(test)]
            let worker_exits = Arc::clone(&(*d).worker_exits);
            let (ready, started) = std::sync::mpsc::sync_channel(1);
            let worker = start_worker(scheduler, move || {
                let mut domain = memory::Domain::new();
                let _active = domain.activate();
                let mut fault = Box::new(0);
                let exec = host::open_local(
                    &mut *fault,
                    functions as *const *const Function,
                    function_count as i64,
                );
                if exec.is_null() {
                    let _ = ready.send(false);
                    return;
                }
                let local = (*exec).session;
                attach(local, Arc::clone(&group), scheduler);
                (*local).reduction_budget = reduction_budget;
                let _ = ready.send(true);
                while !group.stopped.load(Ordering::Acquire) {
                    set_idle(&group, &idle, scheduler, false);
                    let progressed = owner_turn(local);
                    publish_fault(local);
                    if !progressed {
                        set_idle(&group, &idle, scheduler, (*local).next_deadline == u64::MAX);
                        let wait = wait_duration(local);
                        group.endpoints[scheduler].wait(wait);
                    }
                }
                transport::drain(local);
                morrow_managed_close(exec);
                publish_fault(local);
                memory::morrow_gc_collect_precise();
                set_idle(&group, &idle, scheduler, true);
                #[cfg(test)]
                worker_exits.fetch_add(1, Ordering::AcqRel);
            });
            match worker {
                Ok(worker) => (*d).workers.push(worker),
                Err(_) => {
                    fail(exec, 9);
                    stop(s);
                    return 3;
                }
            }
            if started.recv() != Ok(true) {
                fail(exec, 9);
                stop(s);
                return 3;
            }
        }
        0
    }
}

/// Both drivers make the same bounded migration decisions at callback boundaries.
unsafe fn owner_turn(s: *mut Session) -> bool {
    unsafe {
        migration::publish_load(s);
        migration::request(s);
        let progressed = scheduler::turn(s);
        migration::publish_load(s);
        progressed
    }
}

pub(super) unsafe fn next_target(s: *mut Session) -> usize {
    unsafe {
        let Some(d) = driver(s) else {
            return (*s).scheduler;
        };
        let next = (*d).next;
        (*d).next = (next + 1) % Arc::as_ref(&(*d).shared).endpoints.len();
        next
    }
}

/// Pump only admission while a simulated root waits for a remote spawn reply.
/// No callback runs here, preserving the ordinary spawn scheduling boundary.
pub(super) unsafe fn service_spawn(s: *mut Session, target: usize) {
    #[cfg(any(test, feature = "simulation"))]
    unsafe {
        let Some(d) = driver(s) else {
            return;
        };
        let Some(simulation) = (*d).simulation.as_mut() else {
            return;
        };
        if target == 0 {
            transport::drain(s);
            return;
        }
        let worker = &mut simulation.workers[target - 1];
        let _active = worker.domain.activate();
        transport::drain((*worker.exec).session);
        set_idle(&(*d).shared, &(*d).idle, target, false);
    }
    #[cfg(not(any(test, feature = "simulation")))]
    let _ = (s, target);
}

pub(super) unsafe fn publish_fault(s: *mut Session) {
    unsafe {
        let Some(group) = shared(s) else {
            return;
        };
        let fault = *(*s).root.fault;
        if fault != 0 {
            let first =
                match group
                    .fault
                    .compare_exchange(0, fault, Ordering::AcqRel, Ordering::Acquire)
                {
                    Ok(_) => fault,
                    Err(first) => first,
                };
            // Generated main may have written a different local fault directly.
            // Every scheduler reports the invocation's first published winner;
            // actor-local fault cells still belong to their supervision policy.
            *(*s).root.fault = first;
            group.stopped.store(true, Ordering::Release);
            for endpoint in &group.endpoints {
                endpoint.notify();
            }
        } else {
            let fault = group.fault.load(Ordering::Acquire);
            if fault != 0 {
                fail(&raw mut (*s).root, fault);
            }
        }
    }
}

unsafe fn wait_duration(s: *mut Session) -> Duration {
    unsafe {
        let delay = if (*s).next_deadline == u64::MAX {
            10
        } else {
            (*s).next_deadline
                .saturating_sub(now(s).unwrap_or(0))
                .min(10)
        };
        Duration::from_millis(delay)
    }
}

/// Return None for the original single-scheduler host path.
pub(super) unsafe fn poll(exec: *mut Exec, max_steps: i64) -> Option<i64> {
    unsafe {
        let s = (*exec).session;
        let d = driver(s)?;
        if !(1..=65536).contains(&max_steps) {
            fail(exec, 9);
            return Some(3);
        }
        for _ in 0..max_steps {
            publish_fault(s);
            if *(*s).root.fault != 0 {
                morrow_managed_stop(exec);
                return Some(3);
            }
            #[cfg(test)]
            if let Some(hook) = BEFORE_STOP_OBSERVATION.with(|hook| hook.take()) {
                hook(s);
            }
            if Arc::as_ref(&(*d).shared).stopped.load(Ordering::Acquire) {
                // Stop publication follows the worker's first fault. Import it
                // again after the acquire: the earlier read may have seen zero.
                // Joining also includes every worker's cancellation cleanup.
                publish_fault(s);
                morrow_managed_stop(exec);
                return Some(if *(*s).root.fault == 0 { 0 } else { 3 });
            }
            if quiescent_status(s, d).is_some() {
                break;
            }
            #[cfg(any(test, feature = "simulation"))]
            if (*d).simulation.is_some() {
                simulation_turn(s, d);
                continue;
            }
            set_idle(&(*d).shared, &(*d).idle, 0, false);
            let progressed = owner_turn(s);
            set_idle(
                &(*d).shared,
                &(*d).idle,
                0,
                !progressed && (*s).next_deadline == u64::MAX,
            );
            if !progressed {
                break;
            }
        }
        publish_fault(s);
        if *(*s).root.fault != 0 {
            morrow_managed_stop(exec);
            Some(3)
        } else if let Some(status) = quiescent_status(s, d) {
            // Stopped workers can become idle during cancellation after the
            // fault read above. Import their result before reporting quiescence.
            if Arc::as_ref(&(*d).shared).stopped.load(Ordering::Acquire) {
                publish_fault(s);
                morrow_managed_stop(exec);
            }
            Some(if *(*s).root.fault == 0 { status } else { 3 })
        } else {
            Some(2)
        }
    }
}

fn set_idle(group: &transport::Shared, states: &[AtomicBool], scheduler: usize, idle: bool) {
    let _activity = group.activity.lock().unwrap();
    states[scheduler].store(idle, Ordering::Release);
}

/// A zero live count alone is not completion: supervised recovery retires its
/// old actor before admitting the replacement. Inspect the count only while all
/// schedulers are idle and all queues are empty under the activity lock.
unsafe fn quiescent_status(s: *mut Session, d: *mut Driver) -> Option<i64> {
    unsafe {
        let _activity = Arc::as_ref(&(*d).shared).activity.lock().unwrap();
        let idle = (*s).first.is_null()
            && (*d).idle.iter().all(|idle| idle.load(Ordering::Acquire))
            && Arc::as_ref(&(*d).shared)
                .endpoints
                .iter()
                .all(|endpoint| endpoint.is_empty());
        idle.then(|| i64::from(Arc::as_ref(&(*d).shared).budget.live() != 0))
    }
}

/// Execute until the invocation completes or has no possible internal progress.
pub(super) unsafe fn run(exec: *mut Exec) -> bool {
    unsafe {
        let s = (*exec).session;
        let Some(d) = driver(s) else {
            return false;
        };
        loop {
            match poll(exec, 64).unwrap() {
                0 => {
                    morrow_managed_stop(exec);
                    break;
                }
                3 => break,
                1 => {
                    fail(exec, 10);
                    morrow_managed_stop(exec);
                    break;
                }
                _ => {
                    #[cfg(any(test, feature = "simulation"))]
                    if (*d).simulation.is_some() {
                        continue;
                    }
                    // A bounded poll can yield with local continuations ready.
                    // Parking here adds 10ms per 64 turns to any actor on root.
                    if (*s).first.is_null() {
                        #[cfg(test)]
                        RUN_WAITS.with(|count| count.set(count.get() + 1));
                        Arc::as_ref(&(*d).shared).endpoints[0].wait(wait_duration(s));
                    }
                }
            }
        }
        true
    }
}

/// Root stop joins every thread before returning descriptor ownership to its host.
pub(super) unsafe fn stop(s: *mut Session) {
    unsafe {
        let Some(d) = driver(s) else {
            return;
        };
        Arc::as_ref(&(*d).shared)
            .stopped
            .store(true, Ordering::Release);
        for endpoint in &Arc::as_ref(&(*d).shared).endpoints {
            endpoint.notify();
        }
        for worker in std::mem::take(&mut (*d).workers) {
            if worker.join().is_err() {
                fail(&raw mut (*s).root, 11);
            }
        }
        #[cfg(any(test, feature = "simulation"))]
        if let Some(simulation) = (*d).simulation.as_mut() {
            for mut worker in std::mem::take(&mut simulation.workers) {
                let _active = worker.domain.activate();
                transport::drain((*worker.exec).session);
                morrow_managed_close(worker.exec);
                publish_fault((*worker.exec).session);
                memory::morrow_gc_collect_precise();
            }
        }
        transport::drain(s);
        publish_fault(s);
    }
}

/// Select the deterministic driver before admitting actors. Recorded choices can
/// be supplied to replay the same scheduler interleaving.
/// # Safety
/// The same owner-thread and descriptor lifetime contract as parallel applies.
#[cfg(any(test, feature = "simulation"))]
pub unsafe fn simulate(exec: *mut Exec, count: i64, seed: u64, replay: Option<Vec<usize>>) -> i64 {
    unsafe {
        if replay.as_ref().is_some_and(|choices| {
            choices.len() > WORK || choices.iter().any(|&choice| choice >= count as usize)
        }) {
            return 3;
        }
        let result = configure(exec, count, Some(seed));
        if result == 0 {
            let d = driver((*exec).session).unwrap();
            (*d).simulation.as_mut().unwrap().replay = replay;
        }
        result
    }
}

#[cfg(any(test, feature = "simulation"))]
unsafe fn simulation_turn(s: *mut Session, d: *mut Driver) {
    unsafe {
        let simulation = (*d).simulation.as_mut().unwrap();
        if simulation.choices.len() >= WORK {
            fail(&raw mut (*s).root, 9);
            return;
        }
        let choice = if let Some(replay) = &simulation.replay {
            let Some(&choice) = replay.get(simulation.cursor) else {
                fail(&raw mut (*s).root, 11);
                return;
            };
            choice
        } else {
            simulation.random ^= simulation.random << 13;
            simulation.random ^= simulation.random >> 7;
            simulation.random ^= simulation.random << 17;
            simulation.random as usize % Arc::as_ref(&(*d).shared).endpoints.len()
        };
        simulation.cursor += 1;
        simulation.choices.push(choice);
        simulation.milliseconds = simulation.milliseconds.saturating_add(1);
        let milliseconds = simulation.milliseconds;
        // One virtual clock belongs to the invocation, even when a scheduler has
        // not been selected recently. Control writes remain on this owner thread.
        (*s).simulation.milliseconds = milliseconds;
        for worker in &simulation.workers {
            (*(*worker.exec).session).simulation.milliseconds = milliseconds;
        }
        let local = if choice == 0 {
            s
        } else {
            (*simulation.workers[choice - 1].exec).session
        };
        if choice == 0 {
            (*local).simulation.milliseconds = milliseconds;
            let progressed = owner_turn(local);
            set_idle(
                &(*d).shared,
                &(*d).idle,
                choice,
                !progressed && (*local).next_deadline == u64::MAX,
            );
            assert!(memory::verify_heap_edges().is_ok());
        } else {
            let worker = &mut simulation.workers[choice - 1];
            let _active = worker.domain.activate();
            (*local).simulation.milliseconds = milliseconds;
            let progressed = owner_turn(local);
            set_idle(
                &(*d).shared,
                &(*d).idle,
                choice,
                !progressed && (*local).next_deadline == u64::MAX,
            );
            assert!(memory::verify_heap_edges().is_ok());
        }
        publish_fault(local);
    }
}

/// Enable owner-assisted work stealing in a deterministic invocation before
/// actors are admitted. Scheduler choices then fully determine migrations too.
/// # Safety
/// Exec is a live simulation invocation on its owner thread outside callbacks.
#[cfg(any(test, feature = "simulation"))]
pub unsafe fn simulated_work_stealing(exec: *mut Exec, enabled: bool) -> bool {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return false;
        }
        let s = (*exec).session;
        let Some(driver) = driver(s) else {
            return false;
        };
        if (*driver).simulation.is_none() || (*s).next_id != 0 {
            return false;
        }
        Arc::as_ref(&(*driver).shared)
            .stealing
            .store(enabled, Ordering::Release);
        true
    }
}

/// Copy the actual scheduler choices, including idle turns, for exact replay.
/// # Safety
/// Exec is a live simulation invocation on its owner thread outside callbacks.
#[cfg(any(test, feature = "simulation"))]
pub unsafe fn recorded(exec: *mut Exec) -> Option<Vec<usize>> {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return None;
        }
        let d = driver((*exec).session)?;
        Some((*d).simulation.as_ref()?.choices.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Barrier, Mutex, mpsc};
    use std::time::Duration;

    unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
        2
    }

    unsafe extern "C" fn root_countdown(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let left = *frame.cast::<i64>().add(1);
            if left == 0 {
                return 2;
            }
            let mut next = [root_countdown as *const () as i64, left - 1];
            morrow_managed_continue(exec, next.as_mut_ptr().cast())
        }
    }
    #[test]
    fn runnable_root_actor_never_enters_the_remote_work_wait() {
        let scalar = tests_scalar();
        let captures = [&scalar as *const Type];
        let function = Function {
            identity: root_countdown as *const c_void,
            step: Some(root_countdown),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(morrow_managed_parallel(exec, 1), 0);
            let mut frame = [root_countdown as *const () as i64, 128];
            assert!(
                !morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).is_null()
            );
            RUN_WAITS.with(|count| count.set(0));
            assert!(run(exec));
            let waits = RUN_WAITS.with(|count| count.get());
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
            assert_eq!(
                waits, 0,
                "budget exhaustion leaves ready local work and must not park the root scheduler"
            );
        }
    }

    #[test]
    fn published_invocation_fault_wins_over_later_direct_root_fault_writes() {
        let scalar = tests_scalar();
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        for (previously_published, expected) in [(4, 4), (0, 1)] {
            let mut fault = 0;
            unsafe {
                let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
                assert_eq!(morrow_managed_parallel(exec, 1), 0);
                let session = (*exec).session;
                let group = shared_arc(session);
                group.fault.store(previously_published, Ordering::Release);
                // Generated main writes its fault cell directly; it need not
                // call managed::fail and can race with a worker's publication.
                *(*exec).fault = 1;
                publish_fault(session);
                let first_observed = fault;
                *(*exec).fault = 7;
                publish_fault(session);
                let after_later_fault = fault;
                morrow_managed_close(exec);
                assert_eq!(first_observed, expected);
                assert_eq!(after_later_fault, expected);
                assert_eq!(fault, expected, "stop preserves the published winner");
                assert_eq!(group.fault.load(Ordering::Acquire), expected);
            }
        }
    }

    struct RestartProbe {
        gap: Barrier,
        resume: Barrier,
        callbacks: AtomicUsize,
    }

    fn pause_before_restart(context: usize) {
        // The native test owns this probe until stop joins the worker.
        let probe = unsafe { &*(context as *const RestartProbe) };
        probe.gap.wait();
        probe.resume.wait();
    }

    unsafe extern "C" fn restart_failure(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let context = *frame.cast::<usize>().add(1);
            let probe = &*(context as *const RestartProbe);
            if probe.callbacks.fetch_add(1, Ordering::AcqRel) == 0 {
                AFTER_SUPERVISED_RETIREMENT.with(|hook| {
                    hook.set(Some((context, pause_before_restart)));
                });
            }
            fail(exec, 1);
        }
        3
    }

    unsafe extern "C" fn start_supervised_worker(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let context = *frame.cast::<i64>().add(1);
            let mut child = [restart_failure as *const () as i64, context];
            morrow_managed_supervise(
                exec,
                child.as_mut_ptr().cast(),
                (*(*exec).actor).identity.mailbox,
                2,
            );
        }
        2
    }

    #[test]
    fn zero_live_during_supervision_restart_is_busy_until_the_worker_quiesces() {
        let scalar = tests_scalar();
        let captures = [&scalar as *const Type];
        let bootstrap = Function {
            identity: start_supervised_worker as *const c_void,
            step: Some(start_supervised_worker),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let failure = Function {
            identity: restart_failure as *const c_void,
            step: Some(restart_failure),
            ..bootstrap
        };
        let functions = [&bootstrap as *const Function, &failure];
        let probe = RestartProbe {
            gap: Barrier::new(2),
            resume: Barrier::new(2),
            callbacks: AtomicUsize::new(0),
        };
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), functions.len() as i64);
            assert_eq!(morrow_managed_parallel(exec, 2), 0);
            let group = shared_arc((*exec).session);
            let baseline = group.budget.retained();
            let mut frame = [
                start_supervised_worker as *const () as i64,
                &probe as *const _ as i64,
            ];
            assert!(
                !transport::spawn_remote(exec, frame.as_mut_ptr().cast(), &scalar, 1).is_null()
            );
            probe.gap.wait();
            let live_in_gap = group.budget.live();
            let result = poll(exec, 1);
            let stopped_in_gap = group.stopped.load(Ordering::Acquire);
            probe.resume.wait();
            run(exec);
            morrow_managed_close(exec);
            assert_eq!(
                live_in_gap, 0,
                "the test reaches the actual retire/admit gap"
            );
            assert_eq!(result, Some(2), "a running recovery is unfinished work");
            assert!(!stopped_in_gap);
            assert_eq!(probe.callbacks.load(Ordering::Acquire), 3);
            assert_eq!(fault, 0);
            assert_eq!(group.budget.live(), 0);
            assert_eq!(group.budget.retained(), baseline);
        }
    }

    unsafe fn publish_between_fault_import_and_stop(s: *mut Session) {
        let shared = unsafe { shared_arc(s) };
        std::thread::spawn(move || {
            assert_eq!(
                shared
                    .fault
                    .compare_exchange(0, 4, Ordering::AcqRel, Ordering::Acquire),
                Ok(0)
            );
            shared.stopped.store(true, Ordering::Release);
            shared.notify();
        })
        .join()
        .unwrap();
    }

    #[test]
    fn stop_observation_imports_racing_worker_fault_and_joins_before_return() {
        let scalar = tests_scalar();
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(morrow_managed_parallel(exec, 2), 0);
            let d = driver((*exec).session).unwrap();
            BEFORE_STOP_OBSERVATION
                .with(|hook| hook.set(Some(publish_between_fault_import_and_stop)));
            let result = poll(exec, 1);
            let observed_fault = fault;
            let joined = (*d).workers.is_empty();
            morrow_managed_close(exec);
            assert_eq!(result, Some(3), "a faulting stop must not report success");
            assert_eq!(
                observed_fault, 4,
                "the root imports the worker's first fault"
            );
            assert!(joined, "poll returns after every stopped worker has joined");
        }
    }

    struct CancellationProbe {
        installed: mpsc::Sender<()>,
        cleanups: AtomicUsize,
    }

    unsafe extern "C" fn cancellation_cleanup(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<i64>().add(1) as *const CancellationProbe);
            probe.cleanups.fetch_add(1, Ordering::AcqRel);
            fail(exec, 7);
        }
        2
    }

    unsafe extern "C" fn select_nothing(_: *mut Exec, _: *mut c_void, _: i64) -> *mut c_void {
        null_mut()
    }

    unsafe extern "C" fn suspended_with_cleanup(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let pointer = *frame.cast::<i64>().add(1);
            let probe = &*(pointer as *const CancellationProbe);
            let mut cleanup = [cancellation_cleanup as *const () as i64, pointer];
            let mut selector = [select_nothing as *const () as i64];
            if morrow_managed_scope_enter(exec) != 0
                || morrow_managed_scope_defer(exec, cleanup.as_mut_ptr().cast()) != 0
            {
                return 3;
            }
            let status = morrow_managed_receive(exec, selector.as_mut_ptr().cast(), null_mut(), -1);
            let _ = probe.installed.send(());
            status
        }
    }

    unsafe extern "C" fn ordinary_fault(exec: *mut Exec, _: *mut c_void) -> i64 {
        unsafe {
            fail(exec, 4);
        }
        3
    }

    #[test]
    fn worker_fault_cancels_other_scheduler_and_preserves_first_fault_through_cleanup() {
        let (installed, receiver) = mpsc::channel();
        let probe = CancellationProbe {
            installed,
            cleanups: AtomicUsize::new(0),
        };
        let scalar = tests_scalar();
        let captures = [&scalar as *const Type];
        let sleeper = Function {
            identity: suspended_with_cleanup as *const c_void,
            step: Some(suspended_with_cleanup),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let cleanup = Function {
            identity: cancellation_cleanup as *const c_void,
            step: Some(cancellation_cleanup),
            ..sleeper
        };
        let selector = Function {
            identity: select_nothing as *const c_void,
            step: None,
            select: Some(select_nothing),
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let failure = Function {
            identity: ordinary_fault as *const c_void,
            step: Some(ordinary_fault),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&sleeper as *const Function, &cleanup, &selector, &failure];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), functions.len() as i64);
            assert_eq!(morrow_managed_parallel(exec, 3), 0);
            let session = (*exec).session;
            let group = shared_arc(session);
            let baseline = group.budget.retained();
            let mut waiting = [
                suspended_with_cleanup as *const () as i64,
                &probe as *const _ as i64,
            ];
            let sibling = transport::spawn_remote(exec, waiting.as_mut_ptr().cast(), &scalar, 2)
                .cast::<Pid>();
            assert!(!sibling.is_null());
            assert!(receiver.recv_timeout(Duration::from_secs(5)).is_ok());
            let mut failing = [ordinary_fault as *const () as i64];
            assert!(
                !transport::spawn_remote(exec, failing.as_mut_ptr().cast(), &scalar, 1).is_null()
            );
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut result;
            loop {
                result = poll(exec, 1);
                if result == Some(3) || std::time::Instant::now() >= deadline {
                    break;
                }
                group.endpoints[0].wait(Duration::from_millis(1));
            }
            let joined = (*driver(session).unwrap()).workers.is_empty();
            morrow_managed_close(exec);
            assert_eq!(result, Some(3));
            assert!(joined, "fault polling joins worker cancellation cleanup");
            assert_eq!(fault, 4);
            assert_eq!(group.fault.load(Ordering::Acquire), 4);
            assert_eq!(probe.cleanups.load(Ordering::Acquire), 1);
            assert!(!(*(*sibling).actor).identity.alive.load(Ordering::Acquire));
            assert_eq!(group.budget.live(), 0);
            assert_eq!(group.budget.messages(), 0);
            assert_eq!(group.budget.retained(), baseline);
            assert!(group.endpoints.iter().all(|endpoint| endpoint.is_empty()));
        }
    }

    #[test]
    fn failed_second_worker_start_joins_the_already_started_worker() {
        let scalar = tests_scalar();
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            FAIL_WORKER_START.with(|failure| failure.set(Some(2)));
            let result = morrow_managed_parallel(exec, 3);
            FAIL_WORKER_START.with(|failure| failure.set(None));
            let d = driver((*exec).session).unwrap();
            let joined = (*d).workers.is_empty();
            let stopped = Arc::as_ref(&(*d).shared).stopped.load(Ordering::Acquire);
            let exited = Arc::as_ref(&(*d).worker_exits).load(Ordering::Acquire);
            morrow_managed_close(exec);
            assert_eq!(result, 3);
            assert_eq!(fault, 9);
            assert!(stopped);
            assert!(
                joined,
                "startup failure must join every successful prior start"
            );
            assert_eq!(
                exited, 1,
                "the first worker reaches the end of owner cleanup"
            );
        }
    }

    struct Probe {
        entered: Barrier,
        release: Barrier,
        completed: mpsc::Sender<()>,
    }

    unsafe extern "C" fn blocking(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let words = frame.cast::<i64>();
            let probe = &*(*words.add(1) as *const Probe);
            if *words.add(2) == 1 {
                probe.entered.wait();
                probe.release.wait();
            } else {
                probe.completed.send(()).unwrap();
            }
            assert_ne!((*(*exec).session).scheduler, 0);
        }
        2
    }

    #[test]
    fn a_blocked_worker_does_not_prevent_another_worker_callback() {
        let (completed, received) = mpsc::channel();
        let probe = Arc::new(Probe {
            entered: Barrier::new(2),
            release: Barrier::new(2),
            completed,
        });
        let scalar = tests_scalar();
        let captures = [&scalar as *const Type; 2];
        let function = Function {
            identity: blocking as *const c_void,
            step: Some(blocking),
            select: None,
            capture_count: 2,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
            assert_eq!(morrow_managed_parallel(exec, 3), 0);
            let mut first = [blocking as *const () as i64, Arc::as_ptr(&probe) as i64, 1];
            let mut second = [blocking as *const () as i64, Arc::as_ptr(&probe) as i64, 2];
            assert!(
                !transport::spawn_remote(exec, first.as_mut_ptr().cast(), &scalar, 1).is_null()
            );
            probe.entered.wait();
            assert!(
                !transport::spawn_remote(exec, second.as_mut_ptr().cast(), &scalar, 2).is_null()
            );
            let result = received.recv_timeout(Duration::from_secs(5));
            probe.release.wait();
            morrow_managed_close(exec);
            assert!(
                result.is_ok(),
                "worker B must finish while worker A remains blocked"
            );
            assert_eq!(fault, 0);
        }
    }

    unsafe extern "C" fn record(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let words = frame.cast::<i64>();
            let trace = &*(*words.add(1) as *const Mutex<Vec<(i64, usize)>>);
            trace
                .lock()
                .unwrap()
                .push((*words.add(2), (*(*exec).session).scheduler));
        }
        2
    }

    fn tests_scalar() -> Type {
        Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        }
    }

    unsafe fn scenario(replay: Option<Vec<usize>>) -> (Vec<usize>, Vec<(i64, usize)>) {
        let scalar = tests_scalar();
        let captures = [&scalar as *const Type; 2];
        let function = Function {
            identity: record as *const c_void,
            step: Some(record),
            select: None,
            capture_count: 2,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let trace = Mutex::new(Vec::<(i64, usize)>::new());
        let mut fault = 0;
        unsafe {
            let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
            assert_eq!(simulate(exec, 3, 0x46524e, replay), 0);
            for value in 0..12 {
                let mut frame = [record as *const () as i64, &trace as *const _ as i64, value];
                let target = value as usize % 3;
                let pid = if target == 0 {
                    lifecycle::spawn(exec, frame.as_mut_ptr().cast(), &scalar, null_mut())
                } else {
                    transport::spawn_remote(exec, frame.as_mut_ptr().cast(), &scalar, target)
                };
                assert!(!pid.is_null());
            }
            for _ in 0..100 {
                if poll(exec, 1) == Some(0) {
                    break;
                }
            }
            assert_eq!(shared((*exec).session).unwrap().budget.live(), 0);
            let choices = recorded(exec).unwrap();
            assert!(memory::verify_heap_edges().is_ok());
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
            (choices, trace.into_inner().unwrap())
        }
    }

    #[test]
    fn simulated_scheduler_choices_replay_across_isolated_domains() {
        unsafe {
            let (choices, trace) = scenario(None);
            assert_eq!(trace.len(), 12);
            assert!(trace.iter().any(|&(_, scheduler)| scheduler == 0));
            assert!(trace.iter().any(|&(_, scheduler)| scheduler == 1));
            assert!(trace.iter().any(|&(_, scheduler)| scheduler == 2));
            let (replayed, again) = scenario(Some(choices.clone()));
            assert_eq!(choices, replayed);
            assert_eq!(trace, again);
        }
    }
}
