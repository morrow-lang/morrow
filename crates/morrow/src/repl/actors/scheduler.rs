//! FIFO dispatch promotes virtual deadlines only after runnable work is exhausted.
use super::*;
impl Machine {
    pub(in crate::repl) fn run_actors(&mut self) -> Eval<()> {
        loop {
            self.charge_step()?;
            let mut due: Vec<_> = self
                .actors
                .actors
                .iter()
                .filter_map(|(id, a)| {
                    a.waiting
                        .as_ref()?
                        .deadline
                        .filter(|d| *d <= self.actors.time)
                        .map(|d| (d, *id))
                })
                .collect();
            due.sort_unstable();
            for (_, id) in due {
                self.actors.enqueue(id);
            }
            if let Some(id) = self.actors.ready.pop_front() {
                let Some(actor) = self.actors.actors.get_mut(&id) else {
                    continue;
                };
                actor.queued = false;
                self.current_actor = Some(id);
                let result = self.actor_turn(id);
                self.current_actor = None;
                self.actors.time = self
                    .actors
                    .time
                    .checked_add(1)
                    .ok_or_else(|| fault("virtual actor clock overflow"))?;
                let result = result.and_then(|()| self.actors.storage().map_err(fault));
                if let Err(error) = result {
                    self.current_actor = Some(id);
                    let _ = self.drain_actor_scopes(id);
                    self.current_actor = None;
                    let lineage = self.actors.retire(id);
                    if let Some(lineage) = lineage {
                        if let Some(supervisor) = self.actors.supervisors.get_mut(&lineage)
                            && supervisor.remaining > 0
                        {
                            supervisor.remaining -= 1;
                            let initializer = supervisor.initializer.clone();
                            let pid = self.actors.spawn(initializer, Some(lineage))?;
                            self.actors.supervisors.get_mut(&lineage).unwrap().current = pid.id;
                        } else {
                            self.actors.supervisors.remove(&lineage);
                        }
                    } else {
                        return Err(error);
                    }
                }
                continue;
            }
            let next = self
                .actors
                .actors
                .values()
                .filter_map(|a| a.waiting.as_ref()?.deadline)
                .min();
            let Some(deadline) = next else {
                break;
            };
            self.actors.time = deadline;
            let due: Vec<_> = self
                .actors
                .actors
                .iter()
                .filter_map(|(id, a)| {
                    a.waiting
                        .as_ref()?
                        .deadline
                        .filter(|d| *d <= deadline)
                        .map(|d| (d, *id))
                })
                .collect();
            for (_, id) in due {
                self.actors.enqueue(id);
            }
        }
        self.actors.storage().map_err(fault)
    }
    fn actor_turn(&mut self, id: u64) -> Eval<()> {
        self.actors.turns = self
            .actors
            .turns
            .checked_add(1)
            .ok_or_else(|| fault("actor turn counter overflow"))?;
        let entry = self.actors.actors.get_mut(&id).unwrap().entry.take();
        if let Some(entry) = entry {
            let status = self.invoke(&entry, vec![])?;
            match status {
                Value::Int(0) => self.actors.enqueue(id),
                Value::Int(1) => self.poll_actor(id)?,
                Value::Int(2) | Value::Unit => {
                    self.drain_actor_scopes(id)?;
                    if let Some(lineage) = self.actors.retire(id) {
                        self.actors.supervisors.remove(&lineage);
                    }
                }
                _ => return Err(fault("invalid actor continuation status")),
            }
        } else {
            self.poll_actor(id)?;
        }
        Ok(())
    }
    fn poll_actor(&mut self, id: u64) -> Eval<()> {
        let Some(waiting) = self.actors.actors.get(&id).and_then(|a| a.waiting.as_ref()) else {
            return Ok(());
        };
        let selector = waiting.selector.clone();
        let messages: Vec<_> = self.actors.actors[&id].mailbox.iter().cloned().collect();
        for (index, message) in messages.into_iter().enumerate() {
            let selected = self.invoke(&selector, vec![message])?;
            if matches!(selected, Value::Closure(_)) {
                let actor = self.actors.actors.get_mut(&id).unwrap();
                actor.mailbox.remove(index);
                self.actors.messages -= 1;
                actor.waiting = None;
                actor.entry = Some(selected);
                self.actors.enqueue(id);
                return Ok(());
            }
            if selected != Value::Int(0) {
                return Err(fault("invalid actor selector result"));
            }
        }
        let actor = self.actors.actors.get_mut(&id).unwrap();
        if actor
            .waiting
            .as_ref()
            .and_then(|w| w.deadline)
            .is_some_and(|d| d <= self.actors.time)
        {
            actor.entry = actor.waiting.take().and_then(|w| w.timeout);
            if actor.entry.is_none() {
                return Err(fault("missing actor timeout continuation"));
            }
            self.actors.enqueue(id);
        }
        Ok(())
    }
}
