//! String-mailbox actor compatibility API with deterministic supervision forests.
use crate::abi;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_char;
#[cfg(test)]
#[path = "actors/tests.rs"]
mod tests;
#[derive(Clone, Default)]
struct Actor {
    name: String,
    alive: bool,
    replacement: i64,
    linked: i64,
    owner: i64,
    strategy: i64,
    order: i64,
    max_restarts: i64,
    period: i64,
    window: Option<i64>,
    restarts: i64,
    children: Vec<(i64, i64, i64)>,
    next_order: i64,
    monitors: Vec<i64>,
    messages: VecDeque<String>,
    tokens: i64,
    queued: bool,
}
#[derive(Default)]
struct State {
    actors: Vec<Actor>,
    ready: VecDeque<i64>,
    current: i64,
    clock: Option<i64>,
}
impl State {
    fn index(&self, id: i64) -> Option<usize> {
        usize::try_from(id)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .filter(|i| *i < self.actors.len())
    }
    fn live(&self, id: i64) -> Option<usize> {
        self.index(id).filter(|&i| self.actors[i].alive)
    }
    fn now(&self) -> i64 {
        self.clock.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs().min(i64::MAX as u64) as i64)
        })
    }
    fn spawn(&mut self, name: String, linked: i64) -> i64 {
        if self.actors.len() >= i64::MAX as usize || self.actors.try_reserve(1).is_err() {
            return 0;
        }
        self.actors.push(Actor {
            name,
            linked,
            alive: true,
            ..Actor::default()
        });
        self.actors.len() as i64
    }
    fn send(&mut self, id: i64, message: String) -> Result<i64, i64> {
        let index = self.live(id).ok_or(3i64)?;
        let a = &mut self.actors[index];
        if a.tokens == i64::MAX {
            return Err(3);
        }
        a.messages.try_reserve(1).map_err(|_| 4i64)?;
        if !a.queued {
            self.ready.try_reserve(1).map_err(|_| 4i64)?;
        }
        a.messages.push_back(message);
        a.tokens += 1;
        if !a.queued {
            self.ready.push_back(id);
            a.queued = true;
        }
        Ok(0)
    }
    fn receive(&mut self, id: i64) -> Result<String, i64> {
        let index = self.live(id).ok_or(3i64)?;
        let a = &mut self.actors[index];
        let message = a.messages.pop_front().ok_or(3i64)?;
        a.tokens = a.tokens.min(a.messages.len() as i64);
        Ok(message)
    }
    fn next(&mut self) -> i64 {
        while let Some(id) = self.ready.pop_front() {
            let Some(index) = self.index(id) else {
                continue;
            };
            let a = &mut self.actors[index];
            a.tokens = a.tokens.min(a.messages.len() as i64);
            if !a.alive || a.tokens <= 0 || a.messages.is_empty() {
                a.queued = false;
                continue;
            }
            a.tokens -= 1;
            if a.tokens == 0 {
                a.queued = false;
            } else {
                self.ready.push_back(id);
            }
            return id;
        }
        0
    }
    fn monitor(&mut self, observer: i64, worker: i64) -> Result<i64, i64> {
        self.live(observer).ok_or(3i64)?;
        let child = self.live(worker).ok_or(3i64)?;
        if !self.actors[child].monitors.contains(&observer) {
            self.actors[child]
                .monitors
                .try_reserve(1)
                .map_err(|_| 4i64)?;
            self.actors[child].monitors.push(observer);
        }
        Ok(0)
    }
    fn supervise(
        &mut self,
        parent: i64,
        child: i64,
        max: i64,
        period: i64,
        strategy: i64,
    ) -> Result<i64, i64> {
        if max <= 0 || period <= 0 || !(1..=3).contains(&strategy) {
            return Err(3);
        }
        let p = self.live(parent).ok_or(3i64)?;
        let c = self.live(child).ok_or(3i64)?;
        if self.actors[c].owner != 0 && self.actors[c].owner != parent {
            return Err(3);
        }
        let mut ancestor = parent;
        for _ in 0..self.actors.len() {
            if ancestor == child {
                return Err(3);
            }
            let i = self.index(ancestor).ok_or(3i64)?;
            ancestor = self.actors[i].owner;
            if ancestor == 0 {
                break;
            }
        }
        if ancestor != 0 {
            return Err(3);
        }
        self.actors[c].monitors.try_reserve(1).map_err(|_| 4i64)?;
        let existing = self.actors[p]
            .children
            .iter()
            .position(|&(id, _, _)| id == child);
        let order = if let Some(index) = existing {
            self.actors[p].children[index].1 = strategy;
            self.actors[p].children[index].2
        } else {
            self.actors[p].children.try_reserve(1).map_err(|_| 4i64)?;
            let order = self.actors[p].next_order;
            self.actors[p].next_order = order.checked_add(1).ok_or(4i64)?;
            self.actors[p].children.push((child, strategy, order));
            order
        };
        let a = &mut self.actors[c];
        a.owner = parent;
        a.strategy = strategy;
        a.order = order;
        a.max_restarts = max;
        a.period = period;
        a.window = None;
        a.restarts = 0;
        self.monitor(parent, child)
    }
    fn stop_subtree(&mut self, root: i64) -> Vec<i64> {
        let mut pending = vec![root];
        let mut stopped = Vec::new();
        while let Some(id) = pending.pop() {
            let index = self.index(id).expect("validated supervision forest");
            let a = &mut self.actors[index];
            for &(child, _, _) in a.children.iter().rev() {
                pending.push(child);
            }
            if a.alive {
                a.alive = false;
                a.tokens = 0;
                a.queued = false;
                a.messages.clear();
                if self.current == id {
                    self.current = 0;
                }
                stopped.push(id);
            }
        }
        stopped
    }
    fn signal(&mut self, observer: i64, id: i64, kind: &str, reason: &str) -> Result<i64, i64> {
        if self.live(observer).is_none() {
            return Ok(0);
        }
        self.send(
            observer,
            format!(
                "{kind}({id},{})",
                if reason.is_empty() { "unknown" } else { reason }
            ),
        )
    }
    fn exit(&mut self, id: i64, reason: &str) -> Result<i64, i64> {
        let index = self.live(id).ok_or(3i64)?;
        let stopped = self.stop_subtree(id);
        for stopped_id in stopped {
            let i = self.index(stopped_id).unwrap();
            let a = &self.actors[i];
            let linked = a.linked;
            let monitors = a.monitors.clone();
            let stopped_reason = if stopped_id == id { reason } else { "shutdown" };
            self.signal(linked, stopped_id, "Exit", stopped_reason)?;
            for observer in monitors {
                self.signal(observer, stopped_id, "DOWN", stopped_reason)?;
            }
        }
        if matches!(reason, "" | "normal" | "shutdown") {
            return Ok(0);
        }
        let parent = self.actors[index].owner;
        let Some(p) = self.live(parent) else {
            return Ok(0);
        };
        let now = self.now();
        let a = &mut self.actors[index];
        if a.window
            .is_none_or(|start| now.saturating_sub(start) >= a.period)
        {
            a.window = Some(now);
            a.restarts = 0;
        }
        if a.max_restarts <= 0 || a.period <= 0 || a.restarts >= a.max_restarts {
            self.signal(parent, id, "ESCALATE", reason)?;
            return Err(3);
        }
        a.restarts += 1;
        let strategy = a.strategy;
        let order = a.order;
        let mut targets = Vec::new();
        if strategy != 1 {
            for &(child, child_strategy, child_order) in &self.actors[p].children {
                if child_strategy == strategy
                    && (child == id || self.live(child).is_some())
                    && (strategy == 2 || child_order >= order)
                    && !targets.contains(&child)
                {
                    targets.push(child);
                }
            }
        }
        if !targets.contains(&id) {
            targets.push(id);
        }
        for &target in &targets {
            if target != id && self.live(target).is_some() {
                self.exit(target, "shutdown")?;
            }
        }
        let mut primary = 0;
        for target in targets {
            let replacement = self.restart(target)?;
            if target == id {
                primary = replacement;
            }
            self.send(parent, format!("RESTART({target},{replacement})"))?;
        }
        if primary > 0 { Ok(primary) } else { Err(3) }
    }
    fn restart(&mut self, id: i64) -> Result<i64, i64> {
        let index = self.index(id).ok_or(3i64)?;
        let old = &self.actors[index];
        if old.alive || old.replacement != 0 || (old.owner != 0 && self.live(old.owner).is_none()) {
            return Err(3);
        }
        let next = Actor {
            name: old.name.clone(),
            alive: true,
            linked: old.linked,
            owner: old.owner,
            monitors: old.monitors.clone(),
            strategy: old.strategy,
            order: old.order,
            max_restarts: old.max_restarts,
            period: old.period,
            window: old.window,
            restarts: old.restarts,
            ..Actor::default()
        };
        if self.actors.len() >= i64::MAX as usize {
            return Err(4);
        }
        self.actors.try_reserve(1).map_err(|_| 4i64)?;
        let parent = next.owner;
        self.actors.push(next);
        let replacement = self.actors.len() as i64;
        if let Some(p) = self.index(parent) {
            for child in &mut self.actors[p].children {
                if child.0 == id {
                    child.0 = replacement;
                }
            }
        }
        self.actors[index].replacement = replacement;
        Ok(replacement)
    }
}

#[path = "actors/api.rs"]
mod api;
pub use api::*;
