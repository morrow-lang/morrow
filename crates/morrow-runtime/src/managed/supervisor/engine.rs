//! Owner-local supervisor state machine.
//!
//! The engine receives inputs from a driver (child acknowledgements and exits,
//! management requests, retry ticks, injected whole-second clock readings and
//! parent exits) and returns an ordered, bounded list of actions the driver
//! performs. It never schedules, allocates program heaps, reads a clock or
//! touches process identities beyond the opaque ids the driver reports.
//!
//! Restart and termination operations are serialized: while one operation
//! waits for a child acknowledgement or exit, later inputs are queued in
//! arrival order and handled after the operation completes. Exits of running
//! children observed meanwhile are recorded immediately so a later termination
//! step skips the dead child, and their policy handling is deferred.
use super::spec::{
    AutoShutdown, ChildInfo, ChildKind, ChildSpec, ChildState, Error, ExitReason, Flags,
    MAX_CHILDREN, ProcessId, Restart, Shutdown, Strategy, Template, validate, validate_child,
};
use super::window::Window;
use std::collections::VecDeque;

/// Most management requests that may wait behind an in-flight operation.
/// Further requests receive `ResourceLimit` immediately.
pub const MAX_QUEUED_REQUESTS: usize = 64;

/// Driver-chosen correlation token echoed in `Action::Reply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(pub u64);

/// Management commands. Each yields exactly one `Action::Reply`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request<T> {
    /// Append a dynamic child and start it.
    StartChild(ChildSpec<T>),
    /// Terminate a child; Permanent/Transient specs are retained as Stopped,
    /// Temporary specs are removed.
    TerminateChild(String),
    /// Start a stopped child with a fresh generation; never charges intensity.
    RestartChild(String),
    /// Remove a stopped child specification.
    DeleteChild(String),
    /// Report every child in declaration order.
    WhichChildren,
    /// Resolve a name to its running process id or an explicit state error.
    Current(String),
    /// Terminate all children in reverse order and retire with `Normal`.
    Stop,
}

/// Successful request payloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Unit,
    Children(Vec<ChildInfo>),
    Current(ProcessId),
}

/// Request outcome carried by `Action::Reply`.
pub type Reply = Result<Response, Error>;

/// Everything a driver can tell the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input<T> {
    /// The child generation acknowledged its startup with this identity.
    Started {
        name: String,
        generation: u64,
        id: ProcessId,
    },
    /// The child generation declined to start.
    Ignored { name: String, generation: u64 },
    /// The child generation failed to start.
    StartFailed {
        name: String,
        generation: u64,
        reason: ExitReason,
    },
    /// A running child generation exited. Acknowledgement must precede this.
    Exited {
        name: String,
        generation: u64,
        reason: ExitReason,
    },
    /// A graceful deadline armed for this generation elapsed.
    DeadlineElapsed { name: String, generation: u64 },
    /// The driver delivers a retry requested by `Action::ScheduleRetry`.
    Retry { name: String },
    /// Monotonic whole-second reading used for restart-intensity charging.
    Clock { seconds: u64 },
    /// The parent exited; a Normal parent exit also shuts the supervisor down.
    ParentExit { reason: ExitReason },
    /// A management request.
    Request { id: RequestId, request: Request<T> },
}

/// Everything the driver must perform, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action<T> {
    /// Start a fresh child generation from its template and acknowledge it.
    Start {
        name: String,
        kind: ChildKind,
        generation: u64,
        template: T,
    },
    /// Send a shutdown signal to the running generation.
    SendShutdown {
        name: String,
        generation: u64,
        id: ProcessId,
    },
    /// Report `Input::DeadlineElapsed` after this many milliseconds unless the
    /// generation exits first.
    ArmDeadline {
        name: String,
        generation: u64,
        milliseconds: u32,
    },
    /// Kill the running generation without a shutdown signal.
    Kill {
        name: String,
        generation: u64,
        id: ProcessId,
    },
    /// Deliver `Input::Retry` for this child after yielding to other inputs.
    ScheduleRetry { name: String },
    /// Answer a management request.
    Reply { request: RequestId, reply: Reply },
    /// The startup transaction finished. `Err` means every started child was
    /// terminated again; a `Retire` follows.
    StartupComplete(Result<(), Error>),
    /// The supervisor is finished; no further actions follow.
    Retire { reason: ExitReason },
}

/// Internal child occupancy. `Dead` is a natural exit whose policy handling
/// waits for the in-flight operation to complete.
#[derive(Clone, Debug)]
enum Slot {
    Running {
        generation: u64,
        id: ProcessId,
    },
    Dead {
        generation: u64,
        id: ProcessId,
        reason: ExitReason,
    },
    Stopped,
    Restarting,
}

impl Slot {
    const fn is_alive(&self) -> bool {
        matches!(self, Self::Running { .. } | Self::Dead { .. })
    }

    const fn state(&self) -> ChildState {
        match self {
            Self::Running { id, .. } | Self::Dead { id, .. } => ChildState::Running(*id),
            Self::Stopped => ChildState::Stopped,
            Self::Restarting => ChildState::Restarting,
        }
    }
}

struct Child<T> {
    /// Stable identity independent of declaration index and name reuse.
    key: u64,
    spec: ChildSpec<T>,
    slot: Slot,
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Terminate(u64),
    Start(u64),
}

#[derive(Clone, Copy, Debug)]
enum Wait {
    Start {
        key: u64,
        generation: u64,
    },
    Exit {
        key: u64,
        generation: u64,
        /// True once a kill was sent or when the policy never escalates.
        escalated: bool,
    },
}

enum Kind {
    Startup,
    Rollback {
        name: String,
        reason: ExitReason,
    },
    Restart,
    ManualStart {
        request: RequestId,
        key: u64,
    },
    ManualRestart {
        request: RequestId,
    },
    ManualTerminate {
        request: RequestId,
    },
    Shutdown {
        reason: ExitReason,
        request: Option<RequestId>,
    },
}

struct Operation {
    kind: Kind,
    steps: VecDeque<Step>,
    wait: Option<Wait>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Starting,
    Running,
    Retired,
}

enum Ack {
    Started(ProcessId),
    Ignored,
    Failed(ExitReason),
}

/// Deterministic supervisor engine for one supervisor instance.
pub struct Engine<T> {
    flags: Flags,
    children: Vec<Child<T>>,
    next_key: u64,
    next_generation: u64,
    clock: u64,
    window: Window,
    operation: Option<Operation>,
    queue: VecDeque<Input<T>>,
    queued_requests: usize,
    phase: Phase,
}

impl<T: Template> Engine<T> {
    /// Validate the tree and begin the startup transaction.
    ///
    /// Returns the engine together with its first actions: the first child's
    /// `Start`, or `StartupComplete(Ok(()))` for an empty child list. Nothing
    /// is emitted when validation fails.
    pub fn start(
        flags: Flags,
        children: Vec<ChildSpec<T>>,
    ) -> Result<(Self, Vec<Action<T>>), Error> {
        validate(&flags, &children)?;
        let window = Window::new(flags.intensity, flags.period_seconds);
        let children: Vec<Child<T>> = children
            .into_iter()
            .enumerate()
            .map(|(index, spec)| Child {
                key: index as u64,
                spec,
                slot: Slot::Stopped,
            })
            .collect();
        let steps = children
            .iter()
            .map(|child| Step::Start(child.key))
            .collect();
        let mut engine = Self {
            flags,
            next_key: children.len() as u64,
            children,
            next_generation: 1,
            clock: 0,
            window,
            operation: Some(Operation {
                kind: Kind::Startup,
                steps,
                wait: None,
            }),
            queue: VecDeque::new(),
            queued_requests: 0,
            phase: Phase::Starting,
        };
        let mut out = Vec::new();
        engine.run(&mut out);
        Ok((engine, out))
    }

    /// Feed one input and return the actions it produces.
    ///
    /// Work per call is bounded by the queued inputs and twice the child count;
    /// retries always go back through the driver.
    pub fn handle(&mut self, input: Input<T>) -> Vec<Action<T>> {
        let mut out = Vec::new();
        match input {
            Input::Clock { seconds } => self.clock = self.clock.max(seconds),
            Input::DeadlineElapsed { name, generation } => {
                self.deadline(&name, generation, &mut out);
            }
            Input::Started {
                name,
                generation,
                id,
            } => self.acknowledge(&name, generation, Ack::Started(id), &mut out),
            Input::Ignored { name, generation } => {
                self.acknowledge(&name, generation, Ack::Ignored, &mut out);
            }
            Input::StartFailed {
                name,
                generation,
                reason,
            } => self.acknowledge(&name, generation, Ack::Failed(reason), &mut out),
            Input::Exited {
                name,
                generation,
                reason,
            } => self.exited(name, generation, reason),
            Input::Request { id, request } => {
                if self.phase == Phase::Retired {
                    out.push(reply(id, Err(Error::SupervisorStopped)));
                } else if self.queued_requests >= MAX_QUEUED_REQUESTS {
                    out.push(reply(id, Err(Error::ResourceLimit)));
                } else {
                    self.queued_requests = self.queued_requests.saturating_add(1);
                    self.queue.push_back(Input::Request { id, request });
                }
            }
            Input::Retry { .. } | Input::ParentExit { .. } => {
                if self.phase != Phase::Retired {
                    self.queue.push_back(input);
                }
            }
        }
        self.run(&mut out);
        out
    }

    /// True once `Retire` was emitted; later requests get `SupervisorStopped`.
    pub const fn is_retired(&self) -> bool {
        matches!(self.phase, Phase::Retired)
    }

    /// Latest injected clock reading in whole seconds.
    pub const fn clock(&self) -> u64 {
        self.clock
    }

    pub const fn flags(&self) -> &Flags {
        &self.flags
    }

    /// Restart attempts inside the current intensity window.
    pub fn restarts_in_window(&self) -> usize {
        self.window.count()
    }

    /// Declaration-ordered snapshot, identical to `WhichChildren`.
    pub fn children(&self) -> Vec<ChildInfo> {
        self.children
            .iter()
            .map(|child| ChildInfo {
                name: child.spec.name.clone(),
                kind: child.spec.kind,
                state: child.slot.state(),
            })
            .collect()
    }

    fn index_of_name(&self, name: &str) -> Option<usize> {
        self.children
            .iter()
            .position(|child| child.spec.name == name)
    }

    fn index_of_key(&self, key: u64) -> Option<usize> {
        self.children.iter().position(|child| child.key == key)
    }

    /// Advance until an operation waits on the driver or nothing is queued.
    fn run(&mut self, out: &mut Vec<Action<T>>) {
        // Every iteration consumes a step, finishes an operation or consumes a
        // queued input; the budget makes that bound explicit.
        let per_operation = self.children.len().saturating_mul(2).saturating_add(2);
        let mut budget = self
            .queue
            .len()
            .saturating_add(1)
            .saturating_mul(per_operation)
            .saturating_add(per_operation);
        while budget > 0 {
            budget = budget.saturating_sub(1);
            if self.phase == Phase::Retired {
                self.drain_retired(out);
                return;
            }
            if let Some(operation) = &mut self.operation {
                if operation.wait.is_some() {
                    return;
                }
                match operation.steps.pop_front() {
                    Some(Step::Terminate(key)) => self.begin_terminate(key, out),
                    Some(Step::Start(key)) => self.begin_start(key, out),
                    None => self.finish(out),
                }
                continue;
            }
            match self.queue.pop_front() {
                Some(input) => self.process(input, out),
                None => return,
            }
        }
    }

    /// Answer queued requests after retirement and drop other queued inputs.
    fn drain_retired(&mut self, out: &mut Vec<Action<T>>) {
        while let Some(input) = self.queue.pop_front() {
            if let Input::Request { id, .. } = input {
                out.push(reply(id, Err(Error::SupervisorStopped)));
            }
        }
        self.queued_requests = 0;
    }

    fn process(&mut self, input: Input<T>, out: &mut Vec<Action<T>>) {
        match input {
            Input::Request { id, request } => {
                self.queued_requests = self.queued_requests.saturating_sub(1);
                self.request(id, request, out);
            }
            Input::Exited {
                name, generation, ..
            } => {
                let Some(index) = self.index_of_name(&name) else {
                    return;
                };
                let matches = self.children.get(index).is_some_and(|child| {
                    matches!(child.slot, Slot::Dead { generation: dead, .. } if dead == generation)
                });
                if matches {
                    self.child_exited(index);
                }
            }
            Input::Retry { name } => {
                let Some(index) = self.index_of_name(&name) else {
                    return;
                };
                if let Some(child) = self.children.get(index)
                    && matches!(child.slot, Slot::Restarting)
                {
                    let key = child.key;
                    self.restart(key);
                }
            }
            Input::ParentExit { reason } => self.begin_shutdown(reason, None),
            Input::Started { .. }
            | Input::Ignored { .. }
            | Input::StartFailed { .. }
            | Input::DeadlineElapsed { .. }
            | Input::Clock { .. } => {}
        }
    }

    fn request(&mut self, id: RequestId, request: Request<T>, out: &mut Vec<Action<T>>) {
        match request {
            Request::StartChild(spec) => self.start_child(id, spec, out),
            Request::TerminateChild(name) => match self.index_of_name(&name) {
                None => out.push(reply(id, Err(Error::Removed))),
                Some(index) => {
                    let key = self.children.get(index).map_or(0, |child| child.key);
                    self.operation = Some(Operation {
                        kind: Kind::ManualTerminate { request: id },
                        steps: VecDeque::from([Step::Terminate(key)]),
                        wait: None,
                    });
                }
            },
            Request::RestartChild(name) => match self.lookup(&name) {
                Err(error) => out.push(reply(id, Err(error))),
                Ok(key) => {
                    self.operation = Some(Operation {
                        kind: Kind::ManualRestart { request: id },
                        steps: VecDeque::from([Step::Start(key)]),
                        wait: None,
                    });
                }
            },
            Request::DeleteChild(name) => {
                let result = self.lookup(&name).map(|key| {
                    self.children.retain(|child| child.key != key);
                    Response::Unit
                });
                out.push(reply(id, result));
            }
            Request::WhichChildren => {
                out.push(reply(id, Ok(Response::Children(self.children()))));
            }
            Request::Current(name) => {
                let result = match self.index_of_name(&name).and_then(|i| self.children.get(i)) {
                    None => Err(Error::Removed),
                    Some(child) => match child.slot {
                        Slot::Running { id, .. } | Slot::Dead { id, .. } => {
                            Ok(Response::Current(id))
                        }
                        Slot::Stopped => Err(Error::Stopped),
                        Slot::Restarting => Err(Error::Restarting),
                    },
                };
                out.push(reply(id, result));
            }
            Request::Stop => self.begin_shutdown(ExitReason::Normal, Some(id)),
        }
    }

    /// Resolve a name to the key of a stopped child, or the error describing
    /// why it cannot be restarted or deleted.
    fn lookup(&self, name: &str) -> Result<u64, Error> {
        let child = self
            .index_of_name(name)
            .and_then(|index| self.children.get(index))
            .ok_or(Error::Removed)?;
        match child.slot {
            Slot::Running { .. } | Slot::Dead { .. } => Err(Error::AlreadyRunning),
            Slot::Restarting => Err(Error::Restarting),
            Slot::Stopped => Ok(child.key),
        }
    }

    fn start_child(&mut self, id: RequestId, spec: ChildSpec<T>, out: &mut Vec<Action<T>>) {
        if let Err(error) = validate_child(&self.flags, &spec, 1) {
            out.push(reply(id, Err(error)));
            return;
        }
        if let Some(existing) = self
            .index_of_name(&spec.name)
            .and_then(|index| self.children.get(index))
        {
            let error = if existing.slot.is_alive() {
                Error::AlreadyRunning
            } else {
                Error::AlreadyPresent
            };
            out.push(reply(id, Err(error)));
            return;
        }
        if self.children.len() >= MAX_CHILDREN {
            out.push(reply(id, Err(Error::ResourceLimit)));
            return;
        }
        let key = self.next_key;
        self.next_key = self.next_key.saturating_add(1);
        self.children.push(Child {
            key,
            spec,
            slot: Slot::Stopped,
        });
        self.operation = Some(Operation {
            kind: Kind::ManualStart { request: id, key },
            steps: VecDeque::from([Step::Start(key)]),
            wait: None,
        });
    }

    /// Record a running child's exit. An awaited exit completes the current
    /// termination step; any other running generation is marked dead and its
    /// policy handling is queued behind the in-flight operation.
    fn exited(&mut self, name: String, generation: u64, reason: ExitReason) {
        let Some(index) = self.index_of_name(&name) else {
            return;
        };
        let Some(child) = self.children.get_mut(index) else {
            return;
        };
        let Slot::Running {
            generation: running,
            id,
        } = child.slot
        else {
            return;
        };
        if running != generation {
            return;
        }
        let key = child.key;
        let awaited = matches!(
            self.operation.as_ref().and_then(|operation| operation.wait),
            Some(Wait::Exit { key: waited, generation: expected, .. })
                if waited == key && expected == generation
        );
        if awaited {
            if let Some(operation) = &mut self.operation {
                operation.wait = None;
            }
            self.stop_child(index);
        } else {
            child.slot = Slot::Dead {
                generation,
                id,
                reason: reason.clone(),
            };
            self.queue.push_back(Input::Exited {
                name,
                generation,
                reason,
            });
        }
    }

    /// Escalate a graceful termination that is still waiting to a kill.
    fn deadline(&mut self, name: &str, generation: u64, out: &mut Vec<Action<T>>) {
        let Some(Wait::Exit {
            key,
            generation: expected,
            escalated: false,
        }) = self.operation.as_ref().and_then(|operation| operation.wait)
        else {
            return;
        };
        if expected != generation {
            return;
        }
        let Some(child) = self.index_of_key(key).and_then(|i| self.children.get(i)) else {
            return;
        };
        if child.spec.name != name {
            return;
        }
        let Slot::Running { id, .. } = child.slot else {
            return;
        };
        out.push(Action::Kill {
            name: name.to_string(),
            generation,
            id,
        });
        if let Some(operation) = &mut self.operation {
            operation.wait = Some(Wait::Exit {
                key,
                generation,
                escalated: true,
            });
        }
    }

    /// Complete the awaited start step with the driver's outcome.
    fn acknowledge(&mut self, name: &str, generation: u64, ack: Ack, out: &mut Vec<Action<T>>) {
        let Some(Wait::Start {
            key,
            generation: expected,
        }) = self.operation.as_ref().and_then(|operation| operation.wait)
        else {
            return;
        };
        if expected != generation {
            return;
        }
        let Some(index) = self.index_of_key(key) else {
            return;
        };
        if self
            .children
            .get(index)
            .is_none_or(|child| child.spec.name != name)
        {
            return;
        }
        let Some(mut operation) = self.operation.take() else {
            return;
        };
        operation.wait = None;
        match ack {
            Ack::Started(id) => {
                if let Some(child) = self.children.get_mut(index) {
                    child.slot = Slot::Running { generation, id };
                }
                self.operation = Some(operation);
            }
            Ack::Ignored => {
                self.stop_child(index);
                self.operation = Some(operation);
            }
            Ack::Failed(reason) => self.start_failed(operation, index, reason, out),
        }
    }

    fn start_failed(
        &mut self,
        mut operation: Operation,
        index: usize,
        reason: ExitReason,
        out: &mut Vec<Action<T>>,
    ) {
        let name = self
            .children
            .get(index)
            .map(|child| child.spec.name.clone())
            .unwrap_or_default();
        match operation.kind {
            Kind::Startup => {
                operation.kind = Kind::Rollback { name, reason };
                operation.steps = self.terminate_steps(|_| true);
                self.operation = Some(operation);
            }
            Kind::Restart => {
                if let Some(child) = self.children.get_mut(index) {
                    child.slot = Slot::Restarting;
                }
                operation.steps.clear();
                out.push(Action::ScheduleRetry { name });
                self.operation = Some(operation);
            }
            Kind::ManualStart { request, key } => {
                self.children.retain(|child| child.key != key);
                out.push(reply(request, Err(Error::StartFailed(name, reason))));
            }
            Kind::ManualRestart { request } => {
                out.push(reply(request, Err(Error::StartFailed(name, reason))));
            }
            Kind::Rollback { .. } | Kind::ManualTerminate { .. } | Kind::Shutdown { .. } => {
                // These operations never start children; keep them running.
                self.operation = Some(operation);
            }
        }
    }

    /// Terminate steps in reverse declaration order for children matching the
    /// predicate on their declaration index.
    fn terminate_steps(&self, include: impl Fn(usize) -> bool) -> VecDeque<Step> {
        self.children
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, _)| include(*index))
            .map(|(_, child)| Step::Terminate(child.key))
            .collect()
    }

    fn begin_terminate(&mut self, key: u64, out: &mut Vec<Action<T>>) {
        let Some(index) = self.index_of_key(key) else {
            return;
        };
        let Some(child) = self.children.get_mut(index) else {
            return;
        };
        match child.slot {
            Slot::Running { generation, id } => {
                let name = child.spec.name.clone();
                let escalated = match child.spec.policy.shutdown {
                    Shutdown::Immediate => {
                        out.push(Action::Kill {
                            name,
                            generation,
                            id,
                        });
                        true
                    }
                    Shutdown::Graceful(milliseconds) => {
                        out.push(Action::SendShutdown {
                            name: name.clone(),
                            generation,
                            id,
                        });
                        out.push(Action::ArmDeadline {
                            name,
                            generation,
                            milliseconds,
                        });
                        false
                    }
                    Shutdown::Infinity => {
                        out.push(Action::SendShutdown {
                            name,
                            generation,
                            id,
                        });
                        true
                    }
                };
                if let Some(operation) = &mut self.operation {
                    operation.wait = Some(Wait::Exit {
                        key,
                        generation,
                        escalated,
                    });
                }
            }
            Slot::Dead { .. } => self.stop_child(index),
            Slot::Stopped => {}
            Slot::Restarting => child.slot = Slot::Stopped,
        }
    }

    fn begin_start(&mut self, key: u64, out: &mut Vec<Action<T>>) {
        let Some(child) = self.index_of_key(key).and_then(|i| self.children.get(i)) else {
            return;
        };
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        out.push(Action::Start {
            name: child.spec.name.clone(),
            kind: child.spec.kind,
            generation,
            template: child.spec.template.clone(),
        });
        if let Some(operation) = &mut self.operation {
            operation.wait = Some(Wait::Start { key, generation });
        }
    }

    /// A child stopped: temporary specs are removed, others become Stopped.
    fn stop_child(&mut self, index: usize) {
        let Some(child) = self.children.get_mut(index) else {
            return;
        };
        if child.spec.policy.restart == Restart::Temporary {
            self.children.remove(index);
        } else {
            child.slot = Slot::Stopped;
        }
    }

    /// Apply restart policy and significant-child auto shutdown to a natural
    /// exit recorded in a `Dead` slot.
    fn child_exited(&mut self, index: usize) {
        let Some(child) = self.children.get_mut(index) else {
            return;
        };
        let Slot::Dead { reason, .. } = &child.slot else {
            return;
        };
        let restart = match child.spec.policy.restart {
            Restart::Permanent => true,
            Restart::Transient => reason.is_abnormal(),
            Restart::Temporary => false,
        };
        if restart {
            child.slot = Slot::Restarting;
            let key = child.key;
            self.restart(key);
            return;
        }
        let significant = child.spec.policy.significant;
        self.stop_child(index);
        if !significant {
            return;
        }
        let shutdown = match self.flags.auto_shutdown {
            AutoShutdown::Never => false,
            AutoShutdown::AnySignificant => true,
            AutoShutdown::AllSignificant => !self
                .children
                .iter()
                .any(|child| child.spec.policy.significant && child.slot.is_alive()),
        };
        if shutdown {
            self.begin_shutdown(ExitReason::Shutdown, None);
        }
    }

    /// Charge one restart attempt, then start the strategy's operation for the
    /// child identified by `key`, or retire when intensity is exceeded.
    fn restart(&mut self, key: u64) {
        if self.window.charge(self.clock) {
            self.begin_shutdown(ExitReason::Shutdown, None);
            return;
        }
        let Some(index) = self.index_of_key(key) else {
            return;
        };
        let mut steps = match self.flags.strategy {
            Strategy::OneForOne => VecDeque::new(),
            Strategy::OneForAll => self.terminate_steps(|_| true),
            Strategy::RestForOne => self.terminate_steps(|later| later > index),
        };
        let first = match self.flags.strategy {
            Strategy::OneForOne => index,
            Strategy::OneForAll => 0,
            Strategy::RestForOne => index,
        };
        let last = match self.flags.strategy {
            Strategy::OneForOne => index.saturating_add(1),
            Strategy::OneForAll | Strategy::RestForOne => self.children.len(),
        };
        steps.extend(
            self.children
                .iter()
                .skip(first)
                .take(last.saturating_sub(first))
                .map(|child| Step::Start(child.key)),
        );
        self.operation = Some(Operation {
            kind: Kind::Restart,
            steps,
            wait: None,
        });
    }

    fn begin_shutdown(&mut self, reason: ExitReason, request: Option<RequestId>) {
        self.operation = Some(Operation {
            kind: Kind::Shutdown { reason, request },
            steps: self.terminate_steps(|_| true),
            wait: None,
        });
    }

    fn finish(&mut self, out: &mut Vec<Action<T>>) {
        let Some(operation) = self.operation.take() else {
            return;
        };
        match operation.kind {
            Kind::Startup => {
                self.phase = Phase::Running;
                out.push(Action::StartupComplete(Ok(())));
            }
            Kind::Rollback { name, reason } => {
                out.push(Action::StartupComplete(Err(Error::StartFailed(
                    name, reason,
                ))));
                self.retire(ExitReason::Shutdown, out);
            }
            Kind::Restart => {}
            Kind::ManualStart { request, .. }
            | Kind::ManualRestart { request }
            | Kind::ManualTerminate { request } => {
                out.push(reply(request, Ok(Response::Unit)));
            }
            Kind::Shutdown { reason, request } => {
                if let Some(request) = request {
                    out.push(reply(request, Ok(Response::Unit)));
                }
                self.retire(reason, out);
            }
        }
    }

    fn retire(&mut self, reason: ExitReason, out: &mut Vec<Action<T>>) {
        self.phase = Phase::Retired;
        out.push(Action::Retire { reason });
        self.drain_retired(out);
    }
}

fn reply<T>(request: RequestId, reply: Reply) -> Action<T> {
    Action::Reply { request, reply }
}
