//! Seeded external-event scenarios against the production managed scheduler.
use super::*;

pub const VERSION: u32 = 1;
pub const MAX_STEPS: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub seed: u64,
    pub steps: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    pub version: u32,
    pub seed: u64,
    pub steps: u32,
    pub virtual_ms: u64,
    pub callbacks: u64,
    pub delivered: u64,
    pub timeouts: u64,
    pub restarts: u64,
    pub churn: u64,
    pub trace_hash: u64,
    pub final_live: usize,
    pub final_messages: usize,
    pub final_heap_bytes: usize,
    pub final_heap_objects: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Failure {
    pub config: Config,
    pub step: u32,
    pub message: String,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "actor simulation v{VERSION}, seed={}, step={}/{}: {}",
            self.config.seed, self.step, self.config.steps, self.message
        )
    }
}
impl std::error::Error for Failure {}

/// Run a bounded deterministic scenario on an exclusively owned runtime thread.
/// No pointers, clocks or roots from the calling thread enter the scenario.
pub fn run(config: Config) -> Result<Report, Failure> {
    run_with_oracle(config, |output| output)
}

fn run_with_oracle(config: Config, expected_timer: fn(i64) -> i64) -> Result<Report, Failure> {
    if !(1..=MAX_STEPS).contains(&config.steps) {
        return Err(Failure {
            config,
            step: 0,
            message: format!("steps must be in 1..={MAX_STEPS}"),
        });
    }
    std::thread::Builder::new()
        .name("morrow-actor-simulation".into())
        .spawn(move || {
            // Every managed pointer and root is constructed and destroyed on this
            // thread; no caller heap can be affected by the precise-GC oracle.
            let outcome = std::panic::catch_unwind(|| unsafe { drive(config, expected_timer) });
            // drive's roots and invocation guard have retired on both success
            // and ordinary failure/unwind. Release remaining heap-0 controls even
            // when the deliberately corrupted oracle stops the run early.
            unsafe {
                memory::shutdown();
            }
            outcome.unwrap_or_else(|_| {
                Err(Failure {
                    config,
                    step: 0,
                    message: "scenario thread panicked".into(),
                })
            })
        })
        .map_err(|e| Failure {
            config,
            step: 0,
            message: e.to_string(),
        })?
        .join()
        .map_err(|_| Failure {
            config,
            step: 0,
            message: "scenario thread panicked".into(),
        })?
}

// SplitMix64 and FNV-1a are fixed integer-only algorithms. VERSION changes if
// event generation, hashing or the expected scheduling contract changes.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
}
fn hash(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}
fn verify(
    config: Config,
    step: u32,
    condition: bool,
    message: impl Into<String>,
) -> Result<(), Failure> {
    if condition {
        Ok(())
    } else {
        Err(Failure {
            config,
            step,
            message: message.into(),
        })
    }
}
fn expected_outputs(
    config: Config,
    step: u32,
    actual: &[i64],
    expected: &[i64],
) -> Result<(), Failure> {
    verify(
        config,
        step,
        actual == expected,
        format!("reply oracle expected {expected:?}, received {actual:?}"),
    )
}

// All callback frames use the native descriptor ABI: identity followed by
// full-width captures. Stack frames are copied by the real publication APIs.
unsafe extern "C" fn timer(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let f = frame.cast::<i64>();
        let mut select = [
            matching as *const () as i64,
            *f.add(1),
            *f.add(2),
            *f.add(3),
        ];
        let mut after = [emit as *const () as i64, *f.add(1), -*f.add(2)];
        morrow_managed_receive(
            exec,
            select.as_mut_ptr().cast(),
            after.as_mut_ptr().cast(),
            *f.add(4),
        )
    }
}
unsafe extern "C" fn matching(exec: *mut Exec, frame: *mut c_void, message: i64) -> *mut c_void {
    unsafe {
        let f = frame.cast::<i64>();
        if message != *f.add(3) {
            return null_mut();
        }
        let mut next = [emit as *const () as i64, *f.add(1), *f.add(2)];
        // Selector callbacks return owned actor-heap storage to the scheduler.
        let copy = copy::frame((*exec).session, next.as_mut_ptr().cast());
        // Copy owns explicit temporary roots until this return. The scheduler's
        // same-heap replace path publishes the result without a GC safepoint.
        memory::morrow_gc_collect_precise();
        copy.value as *mut c_void
    }
}
unsafe extern "C" fn emit(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let f = frame.cast::<i64>();
        let port = *f.add(1) as *mut Pid;
        let text = Box::new(abi::string(&(*f.add(2)).to_string()) as usize);
        let _text_root = memory::root_range(&*text, 1);
        // Force collection in the sender heap before transport changes scope.
        memory::morrow_gc_collect_precise();
        let result = morrow_managed_send(exec, port.cast(), *text as i64, (*port).mailbox);
        if (*(result as *const abi::ResultValue)).tag != 0 {
            fail(exec, 9);
            return 3;
        }
        2
    }
}
unsafe extern "C" fn yielding(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let f = frame.cast::<i64>();
        let remaining = *f.add(3);
        if remaining == 0 {
            return emit(exec, frame);
        }
        let mut next = [
            yielding as *const () as i64,
            *f.add(1),
            *f.add(2),
            remaining - 1,
        ];
        morrow_managed_continue(exec, next.as_mut_ptr().cast())
    }
}
unsafe extern "C" fn broken(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        emit(exec, frame);
        // A restart must copy the retained initializer, not the failed child's
        // mutated graph. The independent reply oracle expects the original token.
        *frame.cast::<i64>().add(2) += 100;
        fail(exec, 1);
    }
    3
}
unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
    2
}
struct Invocation(*mut Exec);
impl Drop for Invocation {
    fn drop(&mut self) {
        unsafe {
            morrow_managed_close(self.0);
        }
    }
}
unsafe fn drain(
    exec: *mut Exec,
    port: *mut c_void,
    config: Config,
    step: u32,
) -> Result<Vec<i64>, Failure> {
    unsafe {
        let mut replies = Vec::new();
        loop {
            let mut bytes = [0; 32];
            let length = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if length == -1 {
                return Ok(replies);
            }
            verify(
                config,
                step,
                (0..=32).contains(&length) && replies.len() < 16,
                "invalid or unbounded reply",
            )?;
            let token = std::str::from_utf8(&bytes[..length as usize])
                .ok()
                .and_then(|s| s.parse().ok());
            verify(
                config,
                step,
                token.is_some(),
                "reply is not an integer token",
            )?;
            replies.push(token.unwrap());
        }
    }
}
unsafe fn settle(
    exec: *mut Exec,
    random: &mut Random,
    config: Config,
    step: u32,
) -> Result<(), Failure> {
    unsafe {
        for _ in 0..32 {
            let status = morrow_managed_poll(exec, 1 + (random.next() % 4) as i64);
            verify(
                config,
                step,
                status != 3,
                format!("invocation fault {}", *(*exec).fault),
            )?;
            if status == 1 {
                return Ok(());
            }
        }
        verify(
            config,
            step,
            false,
            "sibling failed to progress within 32 bounded polls",
        )
    }
}
unsafe fn drive(config: Config, expected_timer: fn(i64) -> i64) -> Result<Report, Failure> {
    // Descriptor addresses never move while the invocation borrows them.
    let scalar = Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    };
    let string = Type {
        kind: 1,
        count: 0,
        children: null(),
        arities: null(),
    };
    let children = [&string as *const Type];
    let pid_type = Type {
        kind: 6,
        count: 1,
        children: children.as_ptr(),
        arities: null(),
    };
    let captures = [&pid_type as *const Type, &scalar, &scalar, &scalar];
    let function = |identity, step, select, count| Function {
        identity,
        step,
        select,
        capture_count: count,
        captures: captures.as_ptr(),
        mailbox: &scalar,
    };
    let descriptors = [
        function(timer as *const c_void, Some(timer), None, 4),
        function(matching as *const c_void, None, Some(matching), 3),
        function(emit as *const c_void, Some(emit), None, 2),
        function(yielding as *const c_void, Some(yielding), None, 3),
        function(broken as *const c_void, Some(broken), None, 2),
        function(done as *const c_void, Some(done), None, 0),
    ];
    let functions = descriptors.each_ref().map(|f| f as *const Function);
    let mut fault = 0;
    let mut report = Report {
        version: VERSION,
        seed: config.seed,
        steps: config.steps,
        virtual_ms: 0,
        callbacks: 0,
        delivered: 0,
        timeouts: 0,
        restarts: 0,
        churn: 0,
        trace_hash: 0xcbf29ce484222325,
        final_live: 0,
        final_messages: 0,
        final_heap_bytes: 0,
        final_heap_objects: 0,
    };
    let mut random = Random(config.seed);
    unsafe {
        let invocation = Invocation(morrow_managed_open(
            &mut fault,
            functions.as_ptr(),
            functions.len() as i64,
        ));
        let exec = invocation.0;
        verify(
            config,
            0,
            !exec.is_null() && fault == 0,
            "cannot open managed invocation",
        )?;
        enable_clock(exec, 0).map_err(|e| Failure {
            config,
            step: 0,
            message: format!("clock: {e:?}"),
        })?;
        let invocation_bytes = snapshot(exec).unwrap().retained;
        let port = Box::new(morrow_managed_port(exec, &string) as usize);
        verify(config, 0, *port != 0, "cannot create reply port")?;
        let port_root = memory::root_range(&*port, 1);
        let baseline = snapshot(exec).unwrap().retained;
        for step in 0..config.steps {
            let token = i64::from(step) + 1;
            let duration = if step % 8 == 0 {
                600_000
            } else {
                1 + random.next() % 600_000
            };
            let deadline = report.virtual_ms + duration;
            let mode = random.next() % 4;
            let remaining = random.next() % 8;
            let mut initializer = [
                timer as *const () as i64,
                *port as i64,
                token,
                7,
                duration as i64,
            ];
            let actor = morrow_managed_spawn(exec, initializer.as_mut_ptr().cast(), &scalar);
            verify(config, step, !actor.is_null(), "timer admission failed")?;
            let actor_slot = Box::new(actor as usize);
            let actor_root = memory::root_range(&*actor_slot, 1);
            verify(
                config,
                step,
                morrow_managed_poll(exec, 1) == 1,
                "timer failed to suspend",
            )?;
            verify(
                config,
                step,
                snapshot(exec).unwrap().next_deadline == Some(deadline),
                "timer deadline differs from independent arithmetic",
            )?;
            expected_outputs(
                config,
                step,
                &drain(exec, *port as *mut _, config, step)?,
                &[],
            )?;
            let mut sibling = [
                yielding as *const () as i64,
                *port as i64,
                0,
                remaining as i64,
            ];
            morrow_managed_spawn(exec, sibling.as_mut_ptr().cast(), &scalar);
            // Independent precise-root oracle before dereferencing the timer PID.
            memory::morrow_gc_collect_precise();
            verify(
                config,
                step,
                memory::heap_owns(0, actor),
                "timer PID was reclaimed before send",
            )?;
            advance_clock(exec, deadline - 1).unwrap();
            if mode == 0 || mode == 2 {
                let result =
                    morrow_managed_send(exec, actor, if mode == 0 { 7 } else { 8 }, &scalar);
                verify(
                    config,
                    step,
                    (*(result as *const abi::ResultValue)).tag == 0,
                    "pre-deadline send failed",
                )?;
            }
            advance_clock(exec, deadline).unwrap();
            if mode == 1 || mode == 2 {
                let result = morrow_managed_send(exec, actor, 7, &scalar);
                verify(
                    config,
                    step,
                    (*(result as *const abi::ResultValue)).tag == 0,
                    "deadline send failed",
                )?;
            }
            settle(exec, &mut random, config, step)?;
            let output = if mode == 0 {
                report.delivered += 1;
                token
            } else {
                report.timeouts += 1;
                -token
            };
            let output = expected_timer(output);
            let expected = if remaining == 0 {
                [0, output]
            } else {
                [output, 0]
            };
            let actual = drain(exec, *port as *mut _, config, step)?;
            expected_outputs(config, step, &actual, &expected)?;
            // No subsequent operation in this round accesses the timer PID.
            drop(actor_root);
            drop(actor_slot);
            for value in [deadline, mode, remaining] {
                hash(&mut report.trace_hash, value);
            }
            for value in actual {
                hash(&mut report.trace_hash, value as u64);
            }
            report.virtual_ms = deadline;
            // One third of rounds includes repeated child faults. Lifetime restart
            // budget exhaustion must leave unrelated actors and the root healthy.
            if step % 3 == 0 {
                let budget = random.next() % 4;
                let mut failed = [broken as *const () as i64, *port as i64, token];
                let original = morrow_managed_supervise(
                    exec,
                    failed.as_mut_ptr().cast(),
                    &scalar,
                    budget as i64,
                );
                verify(
                    config,
                    step,
                    !original.is_null(),
                    "supervised admission failed",
                )?;
                let mut sibling = [emit as *const () as i64, *port as i64, 0];
                morrow_managed_spawn(exec, sibling.as_mut_ptr().cast(), &scalar);
                settle(exec, &mut random, config, step)?;
                let actual = drain(exec, *port as *mut _, config, step)?;
                let mut expected = vec![token, 0];
                expected.extend(std::iter::repeat_n(token, budget as usize));
                expected_outputs(config, step, &actual, &expected)?;
                report.restarts += budget;
                hash(&mut report.trace_hash, budget);
            }
            let churn = 1 + random.next() % 3;
            for _ in 0..churn {
                let mut frame = [done as *const () as i64];
                verify(
                    config,
                    step,
                    !morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).is_null(),
                    "churn admission failed",
                )?;
            }
            settle(exec, &mut random, config, step)?;
            report.churn += churn;
            let state = snapshot(exec).unwrap();
            verify(
                config,
                step,
                state.live == 1
                    && state.messages == 0
                    && state.retained == baseline
                    && state.next_deadline.is_none()
                    && fault == 0,
                format!("resource oracle: {state:?}; baseline retained={baseline}"),
            )?;
            report.callbacks = state.callbacks;
            hash(&mut report.trace_hash, state.identities);
            // No temporary actor PID is used after this precise collection.
            if step % 64 == 0 {
                memory::morrow_gc_collect_precise();
            }
        }
        morrow_managed_stop(exec);
        let state = snapshot(exec).unwrap();
        report.final_live = state.live;
        report.final_messages = state.messages;
        drop(port_root);
        drop(invocation);
        memory::morrow_gc_collect_precise();
        let stats = memory::stats();
        report.final_heap_bytes = stats.bytes;
        report.final_heap_objects = stats.objects;
        verify(
            config,
            config.steps,
            stats.bytes == 0
                && stats.objects == 0
                && state.live == 0
                && state.messages == 0
                && state.retained == invocation_bytes,
            format!("final reclamation oracle: {stats:?}, {state:?}"),
        )?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_expected_timer_is_rejected_with_replay_configuration() {
        let config = Config { seed: 17, steps: 8 };
        let failure = run_with_oracle(config, |output| -output).unwrap_err();
        assert_eq!((failure.config, failure.step), (config, 0));
        assert!(failure.message.contains("reply oracle expected"));
    }
    #[test]
    fn invalid_work_bounds_are_rejected_before_starting_runtime() {
        for steps in [0, MAX_STEPS + 1, u32::MAX] {
            let config = Config {
                seed: u64::MAX,
                steps,
            };
            assert_eq!(run(config).unwrap_err().config, config);
        }
    }
    #[test]
    fn seeded_corpus_preserves_deadlines_and_sibling_progress() {
        for seed in [0, 1, 2, 3, 17, 42, 0xdead_beef, u64::MAX] {
            let report = run(Config { seed, steps: 256 }).unwrap();
            assert_eq!(report.delivered + report.timeouts, 256);
            assert!(report.virtual_ms >= 32 * 600_000);
        }
    }
    #[test]
    fn replay_survives_more_than_the_old_lifetime_identity_limit() {
        let report = run(Config {
            seed: 42,
            steps: 16_384,
        })
        .unwrap();
        assert!(
            report.churn
                + u64::from(report.steps) * 2
                + u64::from(report.steps.div_ceil(3)) * 2
                + report.restarts
                > 65_536
        );
        assert_eq!((report.final_heap_bytes, report.final_heap_objects), (0, 0));
    }
    #[test]
    fn seeded_actor_scenarios_replay_real_outcomes_and_release_all_heaps() {
        let config = Config {
            seed: 0x0046_524e,
            steps: 64,
        };
        let first = run(config).unwrap();
        assert_eq!(first, run(config).unwrap());
        assert_eq!(first.delivered + first.timeouts, 64);
        assert!(first.callbacks > 64 && first.restarts > 0 && first.churn > 0);
        assert_eq!(
            (
                first.final_live,
                first.final_messages,
                first.final_heap_bytes,
                first.final_heap_objects
            ),
            (0, 0, 0, 0)
        );
        assert_ne!(
            first.trace_hash,
            run(Config {
                seed: config.seed + 1,
                ..config
            })
            .unwrap()
            .trace_hash
        );
    }
}
