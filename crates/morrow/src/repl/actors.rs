//! Deterministic interactive actors share typed continuations, never native memory.
use super::*;
use std::collections::{BTreeMap, VecDeque};
mod cleanup;
mod operations;
mod scheduler;

const MAX_ACTORS: usize = 256;
const MAX_MESSAGES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pid {
    pub(super) id: u64,
    lineage: Option<u64>,
}
struct Actor {
    entry: Option<Value>,
    mailbox: VecDeque<Value>,
    waiting: Option<Waiting>,
    lineage: Option<u64>,
    queued: bool,
    scopes: Vec<Vec<Value>>,
}
struct Waiting {
    selector: Value,
    timeout: Option<Value>,
    deadline: Option<u64>,
}
struct Supervisor {
    initializer: Value,
    current: u64,
    remaining: u32,
}
/// Persistent state has bounded live storage and never recycles logical identities.
#[derive(Default)]
pub(super) struct Scheduler {
    actors: BTreeMap<u64, Actor>,
    supervisors: BTreeMap<u64, Supervisor>,
    ready: VecDeque<u64>,
    next: u64,
    time: u64,
    messages: usize,
    turns: u64,
    stopping: bool,
}
impl Scheduler {
    fn enqueue(&mut self, id: u64) {
        if let Some(actor) = self.actors.get_mut(&id)
            && !actor.queued
        {
            actor.queued = true;
            self.ready.push_back(id);
        }
    }
    fn storage(&self) -> Result<(), String> {
        let mut values = Vec::new();
        for actor in self.actors.values() {
            values.extend(actor.entry.iter());
            values.extend(actor.mailbox.iter());
            values.extend(actor.scopes.iter().flatten());
            if let Some(waiting) = &actor.waiting {
                values.push(&waiting.selector);
                values.extend(waiting.timeout.iter());
            }
        }
        values.extend(self.supervisors.values().map(|s| &s.initializer));
        graph_budget(values.into_iter())
    }
    fn spawn(&mut self, entry: Value, lineage: Option<u64>) -> Eval<Pid> {
        if self.stopping {
            return Err(fault("actor scheduler is stopping"));
        }
        if self.actors.len() >= MAX_ACTORS {
            return Err(fault("interactive actor limit exceeded"));
        }
        let id = self
            .next
            .checked_add(1)
            .ok_or_else(|| fault("interactive actor identity limit exceeded"))?;
        self.next = id;
        self.actors.insert(
            id,
            Actor {
                entry: Some(entry),
                mailbox: VecDeque::new(),
                waiting: None,
                lineage,
                queued: false,
                scopes: Vec::new(),
            },
        );
        if let Err(message) = self.storage() {
            self.actors.remove(&id);
            return Err(fault(message));
        }
        self.enqueue(id);
        Ok(Pid { id, lineage })
    }
    fn retire(&mut self, id: u64) -> Option<u64> {
        let actor = self.actors.remove(&id)?;
        self.messages -= actor.mailbox.len();
        self.ready.retain(|queued| *queued != id);
        actor.lineage
    }
}
fn success(value: Value) -> Value {
    Value::Sum(0, Rc::new(vec![value]))
}
fn error(code: i64) -> Value {
    Value::Sum(1, Rc::new(vec![Value::Int(code)]))
}

impl Machine {
    fn actor_entry(&mut self, value: Value) -> Eval<Value> {
        let Value::Closure(closure) = value else {
            return Err(fault("actor entry must be callable"));
        };
        if !closure.actor_entries.is_empty() {
            let function = closure
                .actor_entries
                .get(&closure.function.0)
                .copied()
                .unwrap_or(closure.function.0);
            return Ok(Value::Closure(Rc::new(ClosureValue {
                program: closure.program.clone(),
                function: ir::FunctionId(function),
                captures: closure.captures.clone(),
                actor_entries: closure.actor_entries.clone(),
            })));
        }
        let (program, entries) = crate::lowering::prepare_interactive_actors(&closure.program)
            .map_err(|e| fault(e.message))?;
        let function = entries
            .get(&closure.function.0)
            .copied()
            .unwrap_or(closure.function.0);
        Ok(Value::Closure(Rc::new(ClosureValue {
            program: Rc::new(program),
            function: ir::FunctionId(function),
            captures: closure.captures.clone(),
            actor_entries: Rc::new(entries),
        })))
    }
    pub(super) fn actor(&mut self, actor: &ir::ActorExpr) -> Eval<Value> {
        if self.comptime {
            return Err(fault("actors cannot execute during comptime"));
        }
        match actor {
            ir::ActorExpr::Spawn {
                entry,
                max_restarts,
                ..
            } => {
                let entry = self.expression(entry)?;
                let entry = self.actor_entry(entry)?;
                let restart = max_restarts
                    .as_ref()
                    .map(|n| self.expression(n))
                    .transpose()?;
                let restart = match restart {
                    None => None,
                    Some(Value::Int(n)) if (0..=32).contains(&n) => Some(n as u32),
                    _ => return Err(fault("actor restart budget must be between 0 and 32")),
                };
                let lineage = restart
                    .map(|_| {
                        self.actors
                            .next
                            .checked_add(1)
                            .ok_or_else(|| fault("interactive actor identity limit exceeded"))
                    })
                    .transpose()?;
                let pid = self.actors.spawn(entry.clone(), lineage)?;
                if let Some(remaining) = restart {
                    self.actors.supervisors.insert(
                        pid.id,
                        Supervisor {
                            initializer: entry,
                            current: pid.id,
                            remaining,
                        },
                    );
                }
                Ok(Value::Pid(pid))
            }
            ir::ActorExpr::Send { pid, message } => {
                let pid = self.expression(pid)?;
                let message = self.expression(message)?;
                let Value::Pid(pid) = pid else {
                    return Err(fault("invalid actor identity"));
                };
                if self.actors.stopping || !self.actors.actors.contains_key(&pid.id) {
                    return Ok(error(3));
                }
                if self.actors.messages >= MAX_MESSAGES {
                    return Ok(error(4));
                }
                self.actors
                    .actors
                    .get_mut(&pid.id)
                    .unwrap()
                    .mailbox
                    .push_back(message);
                if self.actors.storage().is_err() {
                    self.actors
                        .actors
                        .get_mut(&pid.id)
                        .unwrap()
                        .mailbox
                        .pop_back();
                    return Ok(error(4));
                }
                self.actors.messages += 1;
                self.actors.enqueue(pid.id);
                Ok(success(Value::Unit))
            }
            ir::ActorExpr::SupervisedCurrent { pid } => {
                let Value::Pid(pid) = self.expression(pid)? else {
                    return Err(fault("invalid actor identity"));
                };
                Ok(pid
                    .lineage
                    .and_then(|lineage| {
                        self.actors.supervisors.get(&lineage).map(|s| Pid {
                            id: s.current,
                            lineage: Some(lineage),
                        })
                    })
                    .filter(|p| self.actors.actors.contains_key(&p.id))
                    .map_or_else(|| error(3), |p| success(Value::Pid(p))))
            }
            ir::ActorExpr::Lowered(value) => self.actor_operation(&value.operation),
            _ => Err(fault("actor suspension requires a prepared continuation")),
        }
    }
}

/// Deterministic scheduler state suitable for replay comparisons and diagnostics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorReport {
    pub virtual_time_ms: u64,
    pub turns: u64,
    pub spawned: u64,
    pub live: usize,
    pub queued_messages: usize,
    pub pending_cleanups: usize,
    pub scope_frames: usize,
}
/// Every entry outcome is retained, including rejected entries that did not bind values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorReplay {
    pub outcomes: Vec<Result<String, String>>,
    pub actors: ActorReport,
}
impl Session {
    /// Inspect actor state without running callbacks or advancing virtual time.
    pub fn actor_report(&self) -> ActorReport {
        ActorReport {
            virtual_time_ms: self.actors.time,
            turns: self.actors.turns,
            spawned: self.actors.next,
            live: self.actors.actors.len(),
            queued_messages: self.actors.messages,
            pending_cleanups: self
                .actors
                .actors
                .values()
                .flat_map(|a| &a.scopes)
                .map(Vec::len)
                .sum(),
            scope_frames: self.actors.actors.values().map(|a| a.scopes.len()).sum(),
        }
    }
}
/// Replay bounded source entries with virtual actor time and no host I/O effects.
/// Output is captured; filesystem, network, foreign calls and real clocks are unavailable.
pub fn simulate_actors(entries: &[&str]) -> Result<ActorReplay, String> {
    if entries.len() > 4096
        || entries
            .iter()
            .try_fold(0usize, |n, s| n.checked_add(s.len()))
            .is_none_or(|n| n > 8 * 1024 * 1024)
    {
        return Err("actor simulation source limit exceeded".into());
    }
    let mut session = Session {
        simulation: true,
        ..Session::default()
    };
    let mut outcomes = Vec::new();
    let mut bytes = 0usize;
    for entry in entries {
        let outcome = if *entry == ":stop" {
            session.stop_actors()
        } else {
            session.evaluate(entry)
        };
        bytes += match &outcome {
            Ok(s) | Err(s) => s.len(),
        };
        if bytes > 8 * 1024 * 1024 {
            return Err("actor simulation transcript limit exceeded".into());
        }
        outcomes.push(outcome);
    }
    Ok(ActorReplay {
        outcomes,
        actors: session.actor_report(),
    })
}

/// Validate every authored actor body before an entry can perform external effects.
type Prepared = (Rc<ir::Program>, Rc<BTreeMap<usize, usize>>);
pub(super) fn prepare(program: Rc<ir::Program>) -> Result<Prepared, String> {
    let mut pending: Vec<_> = program.functions.iter().map(|f| &f.body).collect();
    let mut active = false;
    while let Some(expr) = pending.pop() {
        if matches!(expr.kind, ir::ExprKind::Actor(_)) {
            active = true;
            break;
        }
        pending.extend(ir::children(expr));
    }
    if !active {
        return Ok((program, Rc::default()));
    }
    let (program, entries) =
        crate::lowering::prepare_interactive_actors(&program).map_err(|e| e.message)?;
    Ok((Rc::new(program), Rc::new(entries)))
}
