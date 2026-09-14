//! Actor cancellation and logical scope cleanup preserve the first failure.
use super::*;
impl Machine {
    pub(in crate::repl::actors) fn drain_actor_scopes(&mut self, id: u64) -> Eval<()> {
        let mut failure = None;
        while self
            .actors
            .actors
            .get(&id)
            .is_some_and(|a| !a.scopes.is_empty())
        {
            if let Err(error) = self.leave_actor_scope(id)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
    pub(in crate::repl::actors) fn leave_actor_scope(&mut self, id: u64) -> Eval<()> {
        if self
            .actors
            .actors
            .get(&id)
            .is_none_or(|a| a.scopes.is_empty())
        {
            return Err(fault("actor scope underflow"));
        }
        let mut failure = None;
        self.cleanup_depth += 1;
        loop {
            let cleanup = self
                .actors
                .actors
                .get_mut(&id)
                .and_then(|a| a.scopes.last_mut())
                .and_then(Vec::pop);
            let Some(cleanup) = cleanup else {
                break;
            };
            let result = self.invoke(&cleanup, vec![]);
            let error = match result {
                Ok(Value::Int(2) | Value::Unit) => None,
                Ok(_) => Some(fault("invalid actor cleanup status")),
                Err(error) => Some(error),
            };
            if failure.is_none() {
                failure = error;
            }
        }
        self.cleanup_depth -= 1;
        self.actors.actors.get_mut(&id).unwrap().scopes.pop();
        failure.map_or(Ok(()), Err)
    }
}
impl Session {
    /// Cancel all actors, draining pending cleanup and preserving issued Pid identities.
    /// Cleanup faults do not prevent other callbacks from running; the first fault is returned.
    pub fn stop_actors(&mut self) -> Result<String, String> {
        let mut machine = Machine::new(Rc::default(), HashMap::new());
        machine.simulation = self.simulation;
        machine.actors = std::mem::take(&mut self.actors);
        machine.actors.stopping = true;
        let mut failure = None;
        while let Some(id) = machine.actors.actors.keys().next().copied() {
            machine.current_actor = Some(id);
            if let Err(error) = machine.drain_actor_scopes(id)
                && failure.is_none()
            {
                failure = Some(error);
            }
            machine.actors.retire(id);
        }
        machine.current_actor = None;
        machine.actors.supervisors.clear();
        machine.actors.stopping = false;
        self.actors = machine.actors;
        match failure {
            None => Ok(machine.output),
            Some(Failure::Message(message)) => Err(message),
            Some(Failure::JsonLimit) => Err("JSON resource limit exceeded".into()),
            Some(_) => Err("invalid control flow during actor cancellation".into()),
        }
    }
}
