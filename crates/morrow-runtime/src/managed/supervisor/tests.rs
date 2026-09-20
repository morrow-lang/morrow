//! Deterministic engine tests replaying the pinned OTP supervisor references.
//!
//! The `World` driver mimics the Elixir fixtures: scripted start outcomes per
//! child name, a comma-joined event log and automatic child replies. No real
//! clock or sleeps are involved; window tests inject whole-second readings.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug)]
enum Outcome {
    Ready,
    Fail(ExitReason),
    Ignore,
    Hold,
}

fn broken() -> ExitReason {
    ExitReason::Failure("broken".to_string())
}

fn flags(strategy: Strategy, intensity: u32) -> Flags {
    Flags {
        strategy,
        intensity,
        period_seconds: 5,
        auto_shutdown: AutoShutdown::Never,
    }
}

fn worker(name: &str) -> ChildSpec<u64> {
    ChildSpec::worker(name, 0)
}

struct World {
    engine: Engine<u64>,
    scripts: BTreeMap<String, VecDeque<Outcome>>,
    log: Vec<String>,
    next_id: u64,
    next_request: u64,
    live: BTreeMap<String, (u64, u64)>,
    holding: BTreeSet<(String, u64)>,
    waiting: Vec<(String, u64)>,
    armed: Vec<(String, u64, u32)>,
    retries: Vec<String>,
    replies: Vec<(RequestId, Result<Response, Error>)>,
    startup: Option<Result<(), Error>>,
    retired: Option<ExitReason>,
    auto_retry: bool,
}

impl World {
    fn start(
        flags: Flags,
        children: Vec<ChildSpec<u64>>,
        scripts: &[(&str, &[Outcome])],
    ) -> Result<World, Error> {
        let (engine, actions) = Engine::start(flags, children)?;
        let mut world = World {
            engine,
            scripts: scripts
                .iter()
                .map(|(name, script)| (name.to_string(), script.iter().cloned().collect()))
                .collect(),
            log: Vec::new(),
            next_id: 100,
            next_request: 1,
            live: BTreeMap::new(),
            holding: BTreeSet::new(),
            waiting: Vec::new(),
            armed: Vec::new(),
            retries: Vec::new(),
            replies: Vec::new(),
            startup: None,
            retired: None,
            auto_retry: true,
        };
        world.drive(actions);
        Ok(world)
    }

    fn outcome(&mut self, name: &str) -> Outcome {
        match self.scripts.get_mut(name) {
            Some(script) if script.len() > 1 => script.pop_front().unwrap_or(Outcome::Ready),
            Some(script) => script.front().cloned().unwrap_or(Outcome::Ready),
            None => Outcome::Ready,
        }
    }

    fn drive(&mut self, actions: Vec<Action<u64>>) {
        let mut queue: VecDeque<Action<u64>> = actions.into();
        let mut budget = 10_000;
        while let Some(action) = queue.pop_front() {
            budget -= 1;
            assert!(budget > 0, "driver loop exceeded its work budget");
            let inputs = self.perform(action);
            for input in inputs {
                queue.extend(self.engine.handle(input));
            }
        }
    }

    fn perform(&mut self, action: Action<u64>) -> Vec<Input<u64>> {
        match action {
            Action::Start {
                name, generation, ..
            } => match self.outcome(&name) {
                Outcome::Ready | Outcome::Hold => {
                    let hold = matches!(self.outcome_peek(&name), Outcome::Hold);
                    if hold {
                        self.holding.insert((name.clone(), generation));
                    }
                    self.log.push(format!("start:{name}"));
                    let id = self.next_id;
                    self.next_id += 1;
                    self.live.insert(name.clone(), (generation, id));
                    vec![Input::Started {
                        name,
                        generation,
                        id,
                    }]
                }
                Outcome::Fail(reason) => {
                    self.log.push(format!("fail:{name}"));
                    vec![Input::StartFailed {
                        name,
                        generation,
                        reason,
                    }]
                }
                Outcome::Ignore => {
                    self.log.push(format!("ignore:{name}"));
                    vec![Input::Ignored { name, generation }]
                }
            },
            Action::SendShutdown {
                name, generation, ..
            } => {
                self.log.push(format!("stop:{name}"));
                if self.holding.contains(&(name.clone(), generation)) {
                    self.waiting.push((name, generation));
                    Vec::new()
                } else {
                    self.live.remove(&name);
                    vec![Input::Exited {
                        name,
                        generation,
                        reason: ExitReason::Shutdown,
                    }]
                }
            }
            Action::ArmDeadline {
                name,
                generation,
                milliseconds,
            } => {
                if milliseconds == 0 {
                    vec![Input::DeadlineElapsed { name, generation }]
                } else {
                    self.armed.push((name, generation, milliseconds));
                    Vec::new()
                }
            }
            Action::Kill {
                name, generation, ..
            } => {
                self.live.remove(&name);
                self.waiting
                    .retain(|(n, g)| !(n == &name && *g == generation));
                vec![Input::Exited {
                    name,
                    generation,
                    reason: ExitReason::Killed,
                }]
            }
            Action::ScheduleRetry { name } => {
                if self.auto_retry {
                    vec![Input::Retry { name }]
                } else {
                    self.retries.push(name);
                    Vec::new()
                }
            }
            Action::Reply { request, reply } => {
                self.replies.push((request, reply));
                Vec::new()
            }
            Action::StartupComplete(result) => {
                self.startup = Some(result);
                Vec::new()
            }
            Action::Retire { reason } => {
                self.retired = Some(reason);
                Vec::new()
            }
        }
    }

    /// The most recently consumed outcome decides holding; scripts with a single
    /// entry repeat it, matching the fixture's `[head] = all` clause.
    fn outcome_peek(&self, name: &str) -> Outcome {
        self.scripts
            .get(name)
            .and_then(|script| script.front().cloned())
            .unwrap_or(Outcome::Ready)
    }

    fn feed(&mut self, input: Input<u64>) {
        let actions = self.engine.handle(input);
        self.drive(actions);
    }

    fn exit(&mut self, name: &str, reason: ExitReason) {
        let (generation, _) = self.live.remove(name).expect("child is live");
        self.feed(Input::Exited {
            name: name.to_string(),
            generation,
            reason,
        });
    }

    fn request(&mut self, request: Request<u64>) -> Result<Response, Error> {
        let id = RequestId(self.next_request);
        self.next_request += 1;
        self.feed(Input::Request { id, request });
        let position = self
            .replies
            .iter()
            .position(|(reply_id, _)| *reply_id == id)
            .expect("request was answered");
        self.replies.remove(position).1
    }

    /// Enqueue a request without expecting an immediate reply.
    fn submit(&mut self, request: Request<u64>) -> RequestId {
        let id = RequestId(self.next_request);
        self.next_request += 1;
        self.feed(Input::Request { id, request });
        id
    }

    fn reply(&mut self, id: RequestId) -> Option<Result<Response, Error>> {
        let position = self
            .replies
            .iter()
            .position(|(reply_id, _)| *reply_id == id)?;
        Some(self.replies.remove(position).1)
    }

    fn take_log(&mut self) -> String {
        let joined = self.log.join(",");
        self.log.clear();
        joined
    }

    fn pid(&self, name: &str) -> Option<u64> {
        self.engine.children().into_iter().find_map(|child| {
            if child.name == name {
                match child.state {
                    ChildState::Running(id) => Some(id),
                    _ => None,
                }
            } else {
                None
            }
        })
    }

    fn state(&self, name: &str) -> Option<ChildState> {
        self.engine
            .children()
            .into_iter()
            .find(|child| child.name == name)
            .map(|child| child.state)
    }

    fn release(&mut self, name: &str) {
        let position = self
            .waiting
            .iter()
            .position(|(n, _)| n == name)
            .expect("child is holding a shutdown");
        let (name, generation) = self.waiting.remove(position);
        self.live.remove(&name);
        self.feed(Input::Exited {
            name,
            generation,
            reason: ExitReason::Shutdown,
        });
    }

    fn elapse(&mut self, name: &str) {
        let position = self
            .armed
            .iter()
            .position(|(n, _, _)| n == name)
            .expect("deadline is armed");
        let (name, generation, _) = self.armed.remove(position);
        self.feed(Input::DeadlineElapsed { name, generation });
    }

    fn stop(&mut self) -> String {
        assert_eq!(self.request(Request::Stop), Ok(Response::Unit));
        assert_eq!(self.retired, Some(ExitReason::Normal));
        self.take_log()
    }
}

fn four_workers() -> Vec<ChildSpec<u64>> {
    ["a", "b", "c", "d"].into_iter().map(worker).collect()
}

fn strategy_reference(strategy: Strategy, restart: &str, restarted: &[&str]) {
    let mut world = World::start(flags(strategy, 10), four_workers(), &[]).unwrap();
    assert_eq!(world.startup, Some(Ok(())));
    assert_eq!(world.take_log(), "start:a,start:b,start:c,start:d");
    let pids: BTreeMap<&str, u64> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|name| (name, world.pid(name).unwrap()))
        .collect();
    world.exit("b", ExitReason::Shutdown);
    assert_eq!(world.take_log(), restart);
    let fresh = restarted
        .iter()
        .all(|name| world.pid(name).unwrap() != pids[name]);
    assert!(fresh, "restarted children must have fresh identities");
    for name in ["a", "b", "c", "d"] {
        if !restarted.contains(&name) {
            assert_eq!(world.pid(name), Some(pids[name]));
        }
    }
    assert_eq!(world.stop(), "stop:d,stop:c,stop:b,stop:a");
    assert!(world.engine.is_retired());
}

#[test]
fn one_for_one_reference_trace() {
    strategy_reference(Strategy::OneForOne, "start:b", &["b"]);
}

#[test]
fn one_for_all_reference_trace() {
    strategy_reference(
        Strategy::OneForAll,
        "stop:d,stop:c,stop:a,start:a,start:b,start:c,start:d",
        &["a", "b", "c", "d"],
    );
}

#[test]
fn rest_for_one_reference_trace() {
    strategy_reference(
        Strategy::RestForOne,
        "stop:d,stop:c,start:b,start:c,start:d",
        &["b", "c", "d"],
    );
}

#[test]
fn manual_stop_restart_delete_and_temporary_errors() {
    let children = vec![
        worker("permanent"),
        worker("transient").with_restart(Restart::Transient),
        worker("temporary").with_restart(Restart::Temporary),
    ];
    let mut world = World::start(flags(Strategy::OneForOne, 10), children, &[]).unwrap();
    let original = world.pid("permanent").unwrap();
    world.take_log();
    assert_eq!(
        world.request(Request::TerminateChild("permanent".into())),
        Ok(Response::Unit)
    );
    assert_eq!(world.take_log(), "stop:permanent");
    assert_eq!(world.state("permanent"), Some(ChildState::Stopped));
    assert_eq!(
        world.request(Request::RestartChild("permanent".into())),
        Ok(Response::Unit)
    );
    let replacement = world.pid("permanent").unwrap();
    assert_ne!(replacement, original, "manual_restart_fresh");
    assert_eq!(
        world.request(Request::Current("permanent".into())),
        Ok(Response::Current(replacement))
    );
    world.take_log();
    assert_eq!(
        world.request(Request::TerminateChild("transient".into())),
        Ok(Response::Unit)
    );
    assert_eq!(
        world.request(Request::DeleteChild("transient".into())),
        Ok(Response::Unit)
    );
    assert_eq!(
        world.request(Request::RestartChild("transient".into())),
        Err(Error::Removed)
    );
    assert_eq!(
        world.request(Request::TerminateChild("temporary".into())),
        Ok(Response::Unit)
    );
    assert_eq!(
        world.request(Request::RestartChild("temporary".into())),
        Err(Error::Removed)
    );
    assert_eq!(world.state("temporary"), None);
    assert_eq!(world.stop(), "stop:transient,stop:temporary,stop:permanent");
}

#[test]
fn policy_matrix_matches_reference() {
    let reasons = [
        ("normal", ExitReason::Normal),
        ("shutdown", ExitReason::Shutdown),
        (
            "shutdown_detail",
            ExitReason::ShutdownDetail("detail".to_string()),
        ),
        ("broken", broken()),
    ];
    let policies = [
        ("permanent", Restart::Permanent),
        ("transient", Restart::Transient),
        ("temporary", Restart::Temporary),
    ];
    let mut observed = Vec::new();
    for (policy_label, restart) in policies {
        for (reason_label, reason) in reasons.clone() {
            let mut world = World::start(
                flags(Strategy::OneForOne, 10),
                vec![worker("a").with_restart(restart)],
                &[],
            )
            .unwrap();
            let old = world.pid("a").unwrap();
            world.exit("a", reason);
            let state = match world.state("a") {
                Some(ChildState::Running(new)) if new != old => "fresh",
                Some(ChildState::Stopped) => "stopped",
                None => "removed",
                other => panic!("unexpected state {other:?}"),
            };
            observed.push(format!("{policy_label}_{reason_label}={state}"));
            assert!(!world.engine.is_retired());
        }
    }
    let expected = [
        "permanent_normal=fresh",
        "permanent_shutdown=fresh",
        "permanent_shutdown_detail=fresh",
        "permanent_broken=fresh",
        "transient_normal=stopped",
        "transient_shutdown=stopped",
        "transient_shutdown_detail=stopped",
        "transient_broken=fresh",
        "temporary_normal=removed",
        "temporary_shutdown=removed",
        "temporary_shutdown_detail=removed",
        "temporary_broken=removed",
    ];
    assert_eq!(observed, expected);
}

#[test]
fn startup_failure_rolls_back_in_reverse() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        four_workers(),
        &[("c", &[Outcome::Fail(broken())])],
    )
    .unwrap();
    assert_eq!(world.take_log(), "start:a,start:b,fail:c,stop:b,stop:a");
    assert_eq!(
        world.startup,
        Some(Err(Error::StartFailed("c".to_string(), broken())))
    );
    assert_eq!(world.retired, Some(ExitReason::Shutdown));
    assert!(world.engine.is_retired());
}

#[test]
fn ignored_specs_keep_non_temporary_children_stopped() {
    let children = vec![
        worker("p"),
        worker("t").with_restart(Restart::Transient),
        worker("x").with_restart(Restart::Temporary),
    ];
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        children,
        &[
            ("p", &[Outcome::Ignore]),
            ("t", &[Outcome::Ignore]),
            ("x", &[Outcome::Ignore]),
        ],
    )
    .unwrap();
    assert_eq!(world.startup, Some(Ok(())));
    assert_eq!(world.take_log(), "ignore:p,ignore:t,ignore:x");
    let snapshot = world.request(Request::WhichChildren);
    assert_eq!(
        snapshot,
        Ok(Response::Children(vec![
            ChildInfo {
                name: "p".to_string(),
                kind: ChildKind::Worker,
                state: ChildState::Stopped
            },
            ChildInfo {
                name: "t".to_string(),
                kind: ChildKind::Worker,
                state: ChildState::Stopped
            },
        ]))
    );
    assert_eq!(world.stop(), "");
}

#[test]
fn failed_restart_attempts_exhaust_intensity() {
    let mut world = World::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 2,
            period_seconds: 3600,
            auto_shutdown: AutoShutdown::Never,
        },
        vec![worker("a")],
        &[("a", &[Outcome::Ready, Outcome::Fail(broken())])],
    )
    .unwrap();
    world.take_log();
    world.exit("a", broken());
    assert_eq!(world.take_log(), "fail:a,fail:a");
    assert_eq!(world.retired, Some(ExitReason::Shutdown));
    assert!(world.engine.is_retired());
}

#[test]
fn mixed_group_removes_temporary_and_restarts_transient() {
    let children = vec![
        worker("a"),
        worker("b").with_restart(Restart::Temporary),
        worker("c").with_restart(Restart::Transient),
    ];
    let mut world = World::start(flags(Strategy::OneForAll, 10), children, &[]).unwrap();
    let old_a = world.pid("a").unwrap();
    let old_c = world.pid("c").unwrap();
    world.take_log();
    world.exit("a", broken());
    assert_eq!(world.take_log(), "stop:c,stop:b,start:a,start:c");
    assert_ne!(world.pid("a").unwrap(), old_a);
    assert_ne!(world.pid("c").unwrap(), old_c);
    assert_eq!(world.state("b"), None, "mixed_temporary_removed");
}

fn group_retry(strategy: Strategy, expected: &str) {
    let mut world = World::start(
        Flags {
            strategy,
            intensity: 10,
            period_seconds: 3600,
            auto_shutdown: AutoShutdown::Never,
        },
        four_workers(),
        &[(
            "c",
            &[Outcome::Ready, Outcome::Fail(broken()), Outcome::Ready],
        )],
    )
    .unwrap();
    let old_d = world.pid("d").unwrap();
    world.take_log();
    world.exit("b", broken());
    assert_eq!(world.take_log(), expected);
    assert_ne!(world.pid("d").unwrap(), old_d);
    for name in ["a", "b", "c", "d"] {
        assert!(matches!(world.state(name), Some(ChildState::Running(_))));
    }
    assert!(!world.engine.is_retired());
}

#[test]
fn one_for_all_failed_initializer_retries_from_failed_child() {
    group_retry(
        Strategy::OneForAll,
        "stop:d,stop:c,stop:a,start:a,start:b,fail:c,stop:b,stop:a,start:a,start:b,start:c,start:d",
    );
}

#[test]
fn rest_for_one_failed_initializer_retries_from_failed_child() {
    group_retry(
        Strategy::RestForOne,
        "stop:d,stop:c,start:b,fail:c,start:c,start:d",
    );
}

#[test]
fn manual_restart_at_intensity_zero_is_free_and_automatic_restart_retires() {
    let mut world = World::start(flags(Strategy::OneForOne, 0), vec![worker("a")], &[]).unwrap();
    let first = world.pid("a").unwrap();
    let mut previous = first;
    for _ in 0..3 {
        assert_eq!(
            world.request(Request::TerminateChild("a".into())),
            Ok(Response::Unit)
        );
        assert_eq!(
            world.request(Request::RestartChild("a".into())),
            Ok(Response::Unit)
        );
        let next = world.pid("a").unwrap();
        assert_ne!(next, previous, "manual restart reused identity");
        previous = next;
    }
    assert_ne!(previous, first, "manual_zero_fresh");
    assert!(!world.engine.is_retired());
    world.exit("a", broken());
    assert_eq!(
        world.retired,
        Some(ExitReason::Shutdown),
        "manual_zero_exit"
    );
}

#[test]
fn graceful_zero_deadline_kills_holding_child() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        vec![worker("a").with_shutdown(Shutdown::Graceful(0))],
        &[("a", &[Outcome::Hold])],
    )
    .unwrap();
    world.take_log();
    assert_eq!(
        world.request(Request::TerminateChild("a".into())),
        Ok(Response::Unit)
    );
    assert_eq!(world.take_log(), "stop:a");
    assert!(world.waiting.is_empty(), "killed child no longer waits");
    assert_eq!(world.state("a"), Some(ChildState::Stopped));
    assert_eq!(world.stop(), "");
}

#[test]
fn graceful_deadline_escalates_to_kill_once() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        vec![worker("a").with_shutdown(Shutdown::Graceful(250))],
        &[("a", &[Outcome::Hold])],
    )
    .unwrap();
    world.take_log();
    let request = world.submit(Request::TerminateChild("a".into()));
    assert_eq!(
        world.reply(request),
        None,
        "termination waits for the child"
    );
    assert_eq!(world.armed.len(), 1);
    assert_eq!(world.armed[0].2, 250);
    let (name, generation, _) = world.armed[0].clone();
    world.elapse("a");
    assert_eq!(world.reply(request), Some(Ok(Response::Unit)));
    assert_eq!(world.take_log(), "stop:a");
    let stale = world
        .engine
        .handle(Input::DeadlineElapsed { name, generation });
    assert!(stale.is_empty(), "a second deadline produces nothing");
    assert_eq!(world.stop(), "");
}

#[test]
fn any_significant_natural_exit_shuts_down() {
    let children = vec![
        worker("a")
            .with_restart(Restart::Temporary)
            .significant(true),
        worker("other"),
    ];
    let mut world = World::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::AnySignificant,
        },
        children,
        &[],
    )
    .unwrap();
    world.take_log();
    world.exit("a", broken());
    assert_eq!(world.retired, Some(ExitReason::Shutdown));
    assert_eq!(world.take_log(), "stop:other");
}

#[test]
fn all_significant_manual_termination_keeps_alive_and_last_natural_stops() {
    let children = vec![
        worker("a")
            .with_restart(Restart::Transient)
            .significant(true),
        worker("b")
            .with_restart(Restart::Transient)
            .significant(true),
    ];
    let mut world = World::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::AllSignificant,
        },
        children,
        &[],
    )
    .unwrap();
    assert_eq!(
        world.request(Request::TerminateChild("a".into())),
        Ok(Response::Unit)
    );
    assert!(!world.engine.is_retired(), "all_manual_keeps_alive");
    world.exit("b", ExitReason::Normal);
    assert_eq!(
        world.retired,
        Some(ExitReason::Shutdown),
        "all_last_natural"
    );
}

#[test]
fn all_significant_waits_for_every_significant_child() {
    let children = vec![
        worker("a")
            .with_restart(Restart::Transient)
            .significant(true),
        worker("b")
            .with_restart(Restart::Transient)
            .significant(true),
        worker("plain"),
    ];
    let mut world = World::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::AllSignificant,
        },
        children,
        &[],
    )
    .unwrap();
    world.exit("a", ExitReason::Normal);
    assert!(!world.engine.is_retired());
    world.take_log();
    world.exit("b", ExitReason::Normal);
    assert_eq!(world.retired, Some(ExitReason::Shutdown));
    assert_eq!(world.take_log(), "stop:plain");
}

#[test]
fn significant_abnormal_transient_restarts() {
    let mut world = World::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 10,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::AnySignificant,
        },
        vec![
            worker("a")
                .with_restart(Restart::Transient)
                .significant(true),
        ],
        &[],
    )
    .unwrap();
    let old = world.pid("a").unwrap();
    world.exit("a", broken());
    assert_ne!(world.pid("a").unwrap(), old);
    assert!(!world.engine.is_retired());
}

#[test]
fn empty_all_significant_keeps_alive() {
    let mut world = World::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::AllSignificant,
        },
        Vec::new(),
        &[],
    )
    .unwrap();
    assert_eq!(world.startup, Some(Ok(())));
    assert!(!world.engine.is_retired());
    assert_eq!(world.stop(), "");
}

#[test]
fn invalid_significant_configurations_start_nothing() {
    let permanent = Engine::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::AnySignificant,
        },
        vec![worker("a").significant(true)],
    );
    assert!(matches!(permanent, Err(Error::InvalidOptions)));
    let never = Engine::start(
        Flags {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::Never,
        },
        vec![
            worker("a")
                .with_restart(Restart::Temporary)
                .significant(true),
        ],
    );
    assert!(matches!(never, Err(Error::InvalidOptions)));
}

#[test]
fn supervisor_induced_exits_never_restart_or_auto_shutdown() {
    let children = vec![
        worker("a"),
        worker("b")
            .with_restart(Restart::Temporary)
            .significant(true),
    ];
    let mut world = World::start(
        Flags {
            strategy: Strategy::OneForAll,
            intensity: 10,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::AnySignificant,
        },
        children,
        &[],
    )
    .unwrap();
    world.take_log();
    world.exit("a", broken());
    assert_eq!(world.take_log(), "stop:b,start:a");
    assert!(!world.engine.is_retired());
    assert_eq!(world.state("b"), None);
}

#[test]
fn infinity_waits_until_release_then_stops() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        vec![worker("a").with_shutdown(Shutdown::Infinity)],
        &[("a", &[Outcome::Hold])],
    )
    .unwrap();
    world.take_log();
    let stop = world.submit(Request::Stop);
    assert_eq!(world.take_log(), "stop:a");
    assert_eq!(world.reply(stop), None);
    assert!(!world.engine.is_retired(), "infinity_waits_after_unlink");
    assert!(world.armed.is_empty(), "infinity arms no deadline");
    world.release("a");
    assert_eq!(world.reply(stop), Some(Ok(Response::Unit)));
    assert_eq!(world.retired, Some(ExitReason::Normal));
}

#[test]
fn nested_branch_kill_leaves_inner_engine_waiting() {
    let (mut inner, inner_actions) = Engine::start(
        flags(Strategy::OneForOne, 1),
        vec![worker("leaf").with_shutdown(Shutdown::Infinity)],
    )
    .unwrap();
    let leaf_generation = match inner_actions.as_slice() {
        [Action::Start { generation, .. }] => *generation,
        other => panic!("unexpected inner startup {other:?}"),
    };
    let inner_started = inner.handle(Input::Started {
        name: "leaf".into(),
        generation: leaf_generation,
        id: 900,
    });
    assert_eq!(inner_started, vec![Action::StartupComplete(Ok(()))]);

    let (mut outer, outer_actions) = Engine::start(
        flags(Strategy::OneForOne, 1),
        vec![ChildSpec::branch("branch", 7).with_shutdown(Shutdown::Graceful(0))],
    )
    .unwrap();
    let branch_generation = match outer_actions.as_slice() {
        [
            Action::Start {
                kind: ChildKind::Branch,
                generation,
                template: 7,
                ..
            },
        ] => *generation,
        other => panic!("unexpected outer startup {other:?}"),
    };
    let outer_started = outer.handle(Input::Started {
        name: "branch".into(),
        generation: branch_generation,
        id: 800,
    });
    assert_eq!(outer_started, vec![Action::StartupComplete(Ok(()))]);

    let stop = outer.handle(Input::Request {
        id: RequestId(1),
        request: Request::Stop,
    });
    assert_eq!(
        stop,
        vec![
            Action::SendShutdown {
                name: "branch".into(),
                generation: branch_generation,
                id: 800
            },
            Action::ArmDeadline {
                name: "branch".into(),
                generation: branch_generation,
                milliseconds: 0
            },
        ]
    );
    let inner_shutdown = inner.handle(Input::ParentExit {
        reason: ExitReason::Shutdown,
    });
    assert_eq!(
        inner_shutdown,
        vec![Action::SendShutdown {
            name: "leaf".into(),
            generation: leaf_generation,
            id: 900
        }]
    );
    let kill = outer.handle(Input::DeadlineElapsed {
        name: "branch".into(),
        generation: branch_generation,
    });
    assert_eq!(
        kill,
        vec![Action::Kill {
            name: "branch".into(),
            generation: branch_generation,
            id: 800
        }]
    );
    let retired = outer.handle(Input::Exited {
        name: "branch".into(),
        generation: branch_generation,
        reason: ExitReason::Killed,
    });
    assert_eq!(
        retired,
        vec![
            Action::Reply {
                request: RequestId(1),
                reply: Ok(Response::Unit)
            },
            Action::Retire {
                reason: ExitReason::Normal
            },
        ],
        "nested_finite_branch=killed"
    );
    assert!(outer.is_retired());
    assert!(!inner.is_retired(), "nested_descendant_still_waiting");
    let released = inner.handle(Input::Exited {
        name: "leaf".into(),
        generation: leaf_generation,
        reason: ExitReason::Shutdown,
    });
    assert_eq!(
        released,
        vec![Action::Retire {
            reason: ExitReason::Shutdown
        }],
        "nested_descendant_release=shutdown"
    );
    assert!(inner.is_retired());
}

fn start_one(flags: Flags) -> (Engine<u64>, u64) {
    let (mut engine, actions) = Engine::start(flags, vec![worker("a")]).unwrap();
    let generation = match actions.as_slice() {
        [Action::Start { generation, .. }] => *generation,
        other => panic!("unexpected startup {other:?}"),
    };
    let started = engine.handle(Input::Started {
        name: "a".into(),
        generation,
        id: 1,
    });
    assert_eq!(started, vec![Action::StartupComplete(Ok(()))]);
    (engine, generation)
}

fn exit_at(engine: &mut Engine<u64>, generation: u64, seconds: u64) -> Vec<Action<u64>> {
    assert!(engine.handle(Input::Clock { seconds }).is_empty());
    engine.handle(Input::Exited {
        name: "a".into(),
        generation,
        reason: broken(),
    })
}

fn restarted_generation(actions: &[Action<u64>]) -> u64 {
    match actions {
        [Action::Start { generation, .. }] => *generation,
        other => panic!("expected a single restart, got {other:?}"),
    }
}

#[test]
fn window_inclusive_boundary_exceeds_intensity() {
    let (mut engine, generation) = start_one(flags(Strategy::OneForOne, 1));
    let second = restarted_generation(&exit_at(&mut engine, generation, 0));
    assert!(
        engine
            .handle(Input::Started {
                name: "a".into(),
                generation: second,
                id: 2
            })
            .is_empty()
    );
    let actions = exit_at(&mut engine, second, 5);
    assert_eq!(
        actions,
        vec![Action::Retire {
            reason: ExitReason::Shutdown
        }]
    );
    assert!(engine.is_retired());
}

#[test]
fn window_outside_period_allows_restart() {
    let (mut engine, generation) = start_one(flags(Strategy::OneForOne, 1));
    let second = restarted_generation(&exit_at(&mut engine, generation, 0));
    assert!(
        engine
            .handle(Input::Started {
                name: "a".into(),
                generation: second,
                id: 2
            })
            .is_empty()
    );
    let third = restarted_generation(&exit_at(&mut engine, second, 6));
    assert!(third > second);
    assert!(!engine.is_retired());
}

#[test]
fn intensity_zero_retires_on_first_automatic_restart() {
    let (mut engine, generation) = start_one(flags(Strategy::OneForOne, 0));
    let actions = exit_at(&mut engine, generation, 0);
    assert_eq!(
        actions,
        vec![Action::Retire {
            reason: ExitReason::Shutdown
        }]
    );
}

#[test]
fn exit_during_startup_is_handled_after_commit() {
    let (mut engine, actions) = Engine::start(
        flags(Strategy::OneForOne, 10),
        vec![worker("a"), worker("b")],
    )
    .unwrap();
    let first = restarted_generation(&actions);
    let next = engine.handle(Input::Started {
        name: "a".into(),
        generation: first,
        id: 1,
    });
    let second = match next.as_slice() {
        [
            Action::Start {
                name, generation, ..
            },
        ] if name == "b" => *generation,
        other => panic!("expected start of b, got {other:?}"),
    };
    let deferred = engine.handle(Input::Exited {
        name: "a".into(),
        generation: first,
        reason: broken(),
    });
    assert!(deferred.is_empty(), "exit waits for the transaction");
    let committed = engine.handle(Input::Started {
        name: "b".into(),
        generation: second,
        id: 2,
    });
    match committed.as_slice() {
        [
            Action::StartupComplete(Ok(())),
            Action::Start {
                name, generation, ..
            },
        ] => {
            assert_eq!(name, "a");
            assert!(*generation > second);
        }
        other => panic!("expected commit then restart, got {other:?}"),
    }
}

#[test]
fn stale_events_are_ignored() {
    let (mut engine, generation) = start_one(flags(Strategy::OneForOne, 10));
    assert!(
        engine
            .handle(Input::Exited {
                name: "a".into(),
                generation: generation + 1000,
                reason: broken()
            })
            .is_empty()
    );
    assert!(
        engine
            .handle(Input::Exited {
                name: "zz".into(),
                generation,
                reason: broken()
            })
            .is_empty()
    );
    assert!(
        engine
            .handle(Input::DeadlineElapsed {
                name: "a".into(),
                generation
            })
            .is_empty()
    );
    assert!(engine.handle(Input::Retry { name: "a".into() }).is_empty());
    assert!(
        engine
            .handle(Input::Started {
                name: "a".into(),
                generation,
                id: 5
            })
            .is_empty()
    );
    assert_eq!(engine.children()[0].state, ChildState::Running(1));
}

#[test]
fn parent_exit_shuts_down_with_parent_reason() {
    let mut world = World::start(flags(Strategy::OneForOne, 1), four_workers(), &[]).unwrap();
    world.take_log();
    world.feed(Input::ParentExit {
        reason: ExitReason::Normal,
    });
    assert_eq!(world.take_log(), "stop:d,stop:c,stop:b,stop:a");
    assert_eq!(world.retired, Some(ExitReason::Normal));
}

#[test]
fn which_children_and_current_report_states() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 10),
        vec![worker("a"), ChildSpec::branch("b", 1), worker("c")],
        &[("c", &[Outcome::Ready, Outcome::Fail(broken())])],
    )
    .unwrap();
    world.auto_retry = false;
    let a = world.pid("a").unwrap();
    assert_eq!(
        world.request(Request::TerminateChild("a".into())),
        Ok(Response::Unit)
    );
    world.exit("c", broken());
    assert_eq!(world.retries, vec!["c".to_string()]);
    let b = world.pid("b").unwrap();
    assert_eq!(
        world.request(Request::WhichChildren),
        Ok(Response::Children(vec![
            ChildInfo {
                name: "a".into(),
                kind: ChildKind::Worker,
                state: ChildState::Stopped
            },
            ChildInfo {
                name: "b".into(),
                kind: ChildKind::Branch,
                state: ChildState::Running(b)
            },
            ChildInfo {
                name: "c".into(),
                kind: ChildKind::Worker,
                state: ChildState::Restarting
            },
        ]))
    );
    assert_ne!(a, b);
    assert_eq!(
        world.request(Request::Current("a".into())),
        Err(Error::Stopped)
    );
    assert_eq!(
        world.request(Request::Current("b".into())),
        Ok(Response::Current(b))
    );
    assert_eq!(
        world.request(Request::Current("c".into())),
        Err(Error::Restarting)
    );
    assert_eq!(
        world.request(Request::Current("zz".into())),
        Err(Error::Removed)
    );
    assert_eq!(
        world.request(Request::DeleteChild("b".into())),
        Err(Error::AlreadyRunning)
    );
    assert_eq!(
        world.request(Request::DeleteChild("c".into())),
        Err(Error::Restarting)
    );
    assert_eq!(
        world.request(Request::RestartChild("c".into())),
        Err(Error::Restarting)
    );
    assert_eq!(
        world.request(Request::RestartChild("b".into())),
        Err(Error::AlreadyRunning)
    );
    assert_eq!(
        world.request(Request::DeleteChild("zz".into())),
        Err(Error::Removed)
    );
    assert_eq!(
        world.request(Request::TerminateChild("zz".into())),
        Err(Error::Removed)
    );
}

#[test]
fn terminate_child_cancels_pending_retry() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 10),
        vec![worker("a")],
        &[(
            "a",
            &[Outcome::Ready, Outcome::Fail(broken()), Outcome::Ready],
        )],
    )
    .unwrap();
    world.auto_retry = false;
    world.exit("a", broken());
    assert_eq!(world.state("a"), Some(ChildState::Restarting));
    assert_eq!(
        world.request(Request::TerminateChild("a".into())),
        Ok(Response::Unit)
    );
    assert_eq!(world.state("a"), Some(ChildState::Stopped));
    world.take_log();
    world.feed(Input::Retry { name: "a".into() });
    assert_eq!(world.take_log(), "", "a stale retry starts nothing");
    assert_eq!(world.state("a"), Some(ChildState::Stopped));
}

#[test]
fn start_child_dynamic_semantics() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 10),
        vec![worker("a")],
        &[
            ("ignored", &[Outcome::Ignore]),
            ("failing", &[Outcome::Fail(broken())]),
        ],
    )
    .unwrap();
    world.take_log();
    assert_eq!(
        world.request(Request::StartChild(worker("a"))),
        Err(Error::AlreadyRunning)
    );
    assert_eq!(
        world.request(Request::StartChild(worker("b"))),
        Ok(Response::Unit)
    );
    assert_eq!(world.take_log(), "start:b");
    assert!(matches!(world.state("b"), Some(ChildState::Running(_))));
    assert_eq!(
        world.request(Request::TerminateChild("b".into())),
        Ok(Response::Unit)
    );
    assert_eq!(
        world.request(Request::StartChild(worker("b"))),
        Err(Error::AlreadyPresent)
    );
    assert_eq!(
        world.request(Request::StartChild(
            worker("ignored").with_restart(Restart::Temporary)
        )),
        Ok(Response::Unit)
    );
    assert_eq!(world.state("ignored"), None);
    assert_eq!(
        world.request(Request::StartChild(worker("ignored"))),
        Ok(Response::Unit)
    );
    assert_eq!(world.state("ignored"), Some(ChildState::Stopped));
    assert_eq!(
        world.request(Request::StartChild(worker("failing"))),
        Err(Error::StartFailed("failing".into(), broken()))
    );
    assert_eq!(world.state("failing"), None);
    assert_eq!(
        world.request(Request::StartChild(worker("a").significant(true))),
        Err(Error::InvalidOptions)
    );
    assert_eq!(
        world.take_log(),
        "stop:b,ignore:ignored,ignore:ignored,fail:failing"
    );
    let order: Vec<String> = world
        .engine
        .children()
        .into_iter()
        .map(|child| child.name)
        .collect();
    assert_eq!(order, vec!["a", "b", "ignored"]);
}

#[test]
fn restart_child_reports_failure_and_keeps_spec() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 0),
        vec![worker("a")],
        &[(
            "a",
            &[Outcome::Ready, Outcome::Fail(broken()), Outcome::Ignore],
        )],
    )
    .unwrap();
    assert_eq!(
        world.request(Request::TerminateChild("a".into())),
        Ok(Response::Unit)
    );
    assert_eq!(
        world.request(Request::RestartChild("a".into())),
        Err(Error::StartFailed("a".into(), broken()))
    );
    assert_eq!(world.state("a"), Some(ChildState::Stopped));
    assert_eq!(
        world.request(Request::RestartChild("a".into())),
        Ok(Response::Unit)
    );
    assert_eq!(world.state("a"), Some(ChildState::Stopped));
    assert!(!world.engine.is_retired(), "manual restarts never charge");
}

#[test]
fn requests_after_retirement_return_supervisor_stopped() {
    let mut world = World::start(flags(Strategy::OneForOne, 1), vec![worker("a")], &[]).unwrap();
    world.stop();
    assert_eq!(
        world.request(Request::WhichChildren),
        Err(Error::SupervisorStopped)
    );
    assert_eq!(
        world.request(Request::StartChild(worker("b"))),
        Err(Error::SupervisorStopped)
    );
    assert_eq!(
        world.request(Request::Current("a".into())),
        Err(Error::SupervisorStopped)
    );
    assert_eq!(world.request(Request::Stop), Err(Error::SupervisorStopped));
    assert!(
        world
            .engine
            .handle(Input::ParentExit {
                reason: ExitReason::Kill
            })
            .is_empty()
    );
}

#[test]
fn queued_requests_after_retirement_are_answered_with_supervisor_stopped() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        vec![worker("a").with_shutdown(Shutdown::Infinity)],
        &[("a", &[Outcome::Hold])],
    )
    .unwrap();
    let stop = world.submit(Request::Stop);
    let later = world.submit(Request::WhichChildren);
    assert_eq!(world.reply(later), None);
    world.release("a");
    assert_eq!(world.reply(stop), Some(Ok(Response::Unit)));
    assert_eq!(world.reply(later), Some(Err(Error::SupervisorStopped)));
}

#[test]
fn request_queue_is_bounded() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        vec![worker("a").with_shutdown(Shutdown::Infinity)],
        &[("a", &[Outcome::Hold])],
    )
    .unwrap();
    let _stop = world.submit(Request::Stop);
    let mut queued = Vec::new();
    for _ in 0..MAX_QUEUED_REQUESTS {
        queued.push(world.submit(Request::WhichChildren));
    }
    let overflow = world.submit(Request::WhichChildren);
    assert_eq!(world.reply(overflow), Some(Err(Error::ResourceLimit)));
    for id in &queued {
        assert_eq!(world.reply(*id), None);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tree {
    Leaf,
    Branch(Flags, Vec<ChildSpec<Tree>>),
}

impl Template for Tree {
    fn nested(&self) -> Option<(&Flags, &[ChildSpec<Tree>])> {
        match self {
            Tree::Leaf => None,
            Tree::Branch(flags, children) => Some((flags, children)),
        }
    }
}

fn nested_tree(depth: usize) -> Vec<ChildSpec<Tree>> {
    let mut children = vec![ChildSpec::worker("leaf", Tree::Leaf)];
    for _ in 1..depth {
        children = vec![ChildSpec::branch(
            "branch",
            Tree::Branch(flags(Strategy::OneForOne, 1), children),
        )];
    }
    children
}

#[test]
fn validation_rejects_each_limit() {
    let ok = flags(Strategy::OneForOne, 1);
    assert_eq!(validate(&ok, &[worker("a")]), Ok(()));
    let mut intensity = ok.clone();
    intensity.intensity = MAX_INTENSITY;
    assert_eq!(validate(&intensity, &[worker("a")]), Ok(()));
    intensity.intensity = MAX_INTENSITY + 1;
    assert_eq!(
        validate(&intensity, &[worker("a")]),
        Err(Error::InvalidOptions)
    );
    let mut period = ok.clone();
    period.period_seconds = 0;
    assert_eq!(
        validate(&period, &[worker("a")]),
        Err(Error::InvalidOptions)
    );
    period.period_seconds = MAX_PERIOD_SECONDS;
    assert_eq!(validate(&period, &[worker("a")]), Ok(()));
    period.period_seconds = MAX_PERIOD_SECONDS + 1;
    assert_eq!(
        validate(&period, &[worker("a")]),
        Err(Error::InvalidOptions)
    );

    let many: Vec<ChildSpec<u64>> = (0..MAX_CHILDREN).map(|i| worker(&i.to_string())).collect();
    assert_eq!(validate(&ok, &many), Ok(()));
    let too_many: Vec<ChildSpec<u64>> =
        (0..=MAX_CHILDREN).map(|i| worker(&i.to_string())).collect();
    assert_eq!(validate(&ok, &too_many), Err(Error::InvalidOptions));

    assert_eq!(validate(&ok, &nested_tree(MAX_DEPTH)), Ok(()));
    assert_eq!(
        validate(&ok, &nested_tree(MAX_DEPTH + 1)),
        Err(Error::InvalidOptions)
    );
    let nested_duplicate = vec![ChildSpec::branch(
        "b",
        Tree::Branch(
            ok.clone(),
            vec![
                ChildSpec::worker("x", Tree::Leaf),
                ChildSpec::worker("x", Tree::Leaf),
            ],
        ),
    )];
    assert_eq!(validate(&ok, &nested_duplicate), Err(Error::DuplicateName));

    let long = "n".repeat(MAX_NAME_BYTES);
    assert_eq!(validate(&ok, &[worker(&long)]), Ok(()));
    let longer = "n".repeat(MAX_NAME_BYTES + 1);
    assert_eq!(
        validate(&ok, &[worker(&longer)]),
        Err(Error::InvalidOptions)
    );

    assert_eq!(
        validate(
            &ok,
            &[worker("a").with_shutdown(Shutdown::Graceful(MAX_GRACEFUL_MILLISECONDS))]
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &ok,
            &[worker("a").with_shutdown(Shutdown::Graceful(MAX_GRACEFUL_MILLISECONDS + 1))]
        ),
        Err(Error::InvalidOptions)
    );
    assert_eq!(
        validate(&ok, &[worker("a"), worker("a")]),
        Err(Error::DuplicateName)
    );
    assert!(matches!(
        Engine::start(ok, vec![worker("a"), worker("a")]),
        Err(Error::DuplicateName)
    ));
}

#[test]
fn dynamic_start_child_is_bounded() {
    let many: Vec<ChildSpec<u64>> = (0..MAX_CHILDREN).map(|i| worker(&i.to_string())).collect();
    let mut world = World::start(flags(Strategy::OneForOne, 1), many, &[]).unwrap();
    assert_eq!(
        world.request(Request::StartChild(worker("extra"))),
        Err(Error::ResourceLimit)
    );
    let long = "n".repeat(MAX_NAME_BYTES + 1);
    assert_eq!(
        world.request(Request::StartChild(worker(&long))),
        Err(Error::InvalidOptions)
    );
    assert_eq!(
        world.request(Request::StartChild(
            worker("extra").with_shutdown(Shutdown::Graceful(MAX_GRACEFUL_MILLISECONDS + 1))
        )),
        Err(Error::InvalidOptions)
    );
    assert_eq!(world.engine.children().len(), MAX_CHILDREN);
}

#[test]
fn default_policies_follow_the_specification() {
    let w = worker("w");
    assert_eq!(w.policy.restart, Restart::Permanent);
    assert_eq!(w.policy.shutdown, Shutdown::Graceful(5000));
    assert!(!w.policy.significant);
    assert_eq!(w.kind, ChildKind::Worker);
    let b = ChildSpec::branch("b", 0u64);
    assert_eq!(b.policy.restart, Restart::Permanent);
    assert_eq!(b.policy.shutdown, Shutdown::Infinity);
    assert_eq!(b.kind, ChildKind::Branch);
    let defaults = Flags::default();
    assert_eq!(defaults.intensity, 1);
    assert_eq!(defaults.period_seconds, 5);
    assert_eq!(defaults.strategy, Strategy::OneForOne);
    assert_eq!(defaults.auto_shutdown, AutoShutdown::Never);
}

#[test]
fn immediate_shutdown_kills_without_deadline() {
    let mut world = World::start(
        flags(Strategy::OneForOne, 1),
        vec![worker("a").with_shutdown(Shutdown::Immediate)],
        &[("a", &[Outcome::Hold])],
    )
    .unwrap();
    world.take_log();
    assert_eq!(
        world.request(Request::TerminateChild("a".into())),
        Ok(Response::Unit)
    );
    assert_eq!(world.take_log(), "", "a killed child logs no stop");
    assert!(world.armed.is_empty());
    assert_eq!(world.state("a"), Some(ChildState::Stopped));
}

#[test]
fn tag_order_is_frozen() {
    let reasons = [
        ExitReason::Normal,
        ExitReason::Shutdown,
        ExitReason::ShutdownDetail(String::new()),
        ExitReason::Fault(0),
        ExitReason::Failure(String::new()),
        ExitReason::Kill,
        ExitReason::Killed,
        ExitReason::NoProcess,
    ];
    for (index, reason) in reasons.iter().enumerate() {
        assert_eq!(reason.tag(), index as i64);
    }
    assert!(!ExitReason::Normal.is_abnormal());
    assert!(!ExitReason::Shutdown.is_abnormal());
    assert!(!ExitReason::ShutdownDetail("x".into()).is_abnormal());
    assert!(ExitReason::Fault(1).is_abnormal());
    assert!(ExitReason::Killed.is_abnormal());
    let errors = [
        Error::InvalidOptions,
        Error::ResourceLimit,
        Error::ForeignInvocation,
        Error::UnsupportedContext,
        Error::WrongChildKey,
        Error::DuplicateName,
        Error::AlreadyRunning,
        Error::AlreadyPresent,
        Error::Restarting,
        Error::Stopped,
        Error::Removed,
        Error::StartFailed(String::new(), ExitReason::Normal),
        Error::SupervisorStopped,
    ];
    for (index, error) in errors.iter().enumerate() {
        assert_eq!(error.tag(), index as i64);
    }
    assert_eq!(ChildState::Running(1).tag(), 0);
    assert_eq!(ChildState::Stopped.tag(), 1);
    assert_eq!(ChildState::Restarting.tag(), 2);
    assert_eq!(ChildKind::Worker.tag(), 0);
    assert_eq!(ChildKind::Branch.tag(), 1);
}
