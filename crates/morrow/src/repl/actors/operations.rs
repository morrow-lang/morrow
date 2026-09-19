//! Interpret sealed scheduling instructions as immutable values.
use super::*;
use crate::actors::Operation;
impl Machine {
    pub(super) fn actor_operation(&mut self, operation: &Operation) -> Eval<Value> {
        match operation {
            Operation::ListBuilder { capacity, .. } => {
                let Value::Int(capacity) = self.expression(capacity)? else {
                    return Err(fault("actor list capacity must be Int"));
                };
                if capacity < 0 {
                    return Err(fault("actor list capacity cannot be negative"));
                }
                Ok(Value::List(Rc::new(Vec::new())))
            }
            Operation::ListAppend { list, value } => {
                let Value::List(items) = self.expression(list)? else {
                    return Err(fault("actor list append requires List"));
                };
                let value = self.expression(value)?;
                let mut items = (*items).clone();
                items.push(value);
                Ok(Value::List(Rc::new(items)))
            }
            Operation::ClosureIdentity { value, function } => {
                let Value::Closure(closure) = self.expression(value)? else {
                    return Err(fault("actor dynamic call requires closure"));
                };
                // Numeric identities are scoped to the closure's originating program.
                // Old REPL values must use their original callable fallback after later entries.
                Ok(Value::Bool(
                    Rc::ptr_eq(&closure.program, &self.program) && closure.function == *function,
                ))
            }
            Operation::ClosureCapture { value, index, .. } => {
                let Value::Closure(closure) = self.expression(value)? else {
                    return Err(fault("actor dynamic capture requires closure"));
                };
                closure
                    .captures
                    .get(*index)
                    .cloned()
                    .ok_or_else(|| fault("actor closure capture outside bounds"))
            }
            Operation::ScopeEnter => {
                let actor = self
                    .current_actor
                    .and_then(|id| self.actors.actors.get_mut(&id))
                    .ok_or_else(|| fault("actor scope outside actor"))?;
                if actor.scopes.len() + actor.scopes.iter().map(Vec::len).sum::<usize>() >= 4096 {
                    return Err(fault("actor scope/cleanup limit exceeded"));
                }
                actor.scopes.push(Vec::new());
                Ok(Value::Int(0))
            }
            Operation::ScopeDefer(closure) => {
                let closure = self.expression(closure)?;
                if !matches!(closure, Value::Closure(_)) {
                    return Err(fault("invalid actor cleanup closure"));
                }
                let actor = self
                    .current_actor
                    .and_then(|id| self.actors.actors.get_mut(&id))
                    .ok_or_else(|| fault("actor cleanup outside actor"))?;
                if actor.scopes.len() + actor.scopes.iter().map(Vec::len).sum::<usize>() >= 4096 {
                    return Err(fault("actor scope/cleanup limit exceeded"));
                }
                actor
                    .scopes
                    .last_mut()
                    .ok_or_else(|| fault("actor cleanup without scope"))?
                    .push(closure);
                Ok(Value::Int(0))
            }
            Operation::ScopeLeave => {
                let id = self
                    .current_actor
                    .ok_or_else(|| fault("actor scope outside actor"))?;
                self.leave_actor_scope(id)?;
                Ok(Value::Int(0))
            }
            Operation::CleanupInvoke { function, closure } => {
                let closure = self.expression(closure)?;
                let Value::Closure(value) = &closure else {
                    return Err(fault("invalid actor cleanup callback"));
                };
                if value.function != *function {
                    return Err(fault("actor cleanup identity mismatch"));
                }
                if self.invoke(&closure, vec![])? != Value::Unit {
                    return Err(fault("actor cleanup callback must return Unit"));
                }
                Ok(Value::Int(2))
            }

            Operation::Pointer(value) => self.expression(value),
            Operation::ProcessExit(_) => {
                Err(fault("typed Process operations require the native backend"))
            }
            Operation::Continue(value) | Operation::ContinueReusable(value) => {
                let value = self.expression(value)?;
                let actor = self
                    .current_actor
                    .and_then(|id| self.actors.actors.get_mut(&id))
                    .ok_or_else(|| fault("continuation outside actor"))?;
                actor.entry = Some(value);
                Ok(Value::Int(0))
            }
            Operation::Register {
                selector,
                timeout,
                duration,
                ..
            } => {
                let selector = self.expression(selector)?;
                let timeout = timeout
                    .as_ref()
                    .map(|value| self.expression(value))
                    .transpose()?;
                let Value::Int(duration) = self.expression(duration)? else {
                    return Err(fault("invalid receive duration"));
                };
                if duration < -1 || duration < 0 && timeout.is_some() {
                    return Err(fault("receive duration cannot be negative"));
                }
                let deadline = if duration == -1 {
                    None
                } else {
                    Some(
                        self.actors
                            .time
                            .checked_add(duration as u64)
                            .ok_or_else(|| fault("virtual actor clock overflow"))?,
                    )
                };
                let actor = self
                    .current_actor
                    .and_then(|id| self.actors.actors.get_mut(&id))
                    .ok_or_else(|| fault("receive outside actor"))?;
                actor.waiting = Some(Waiting {
                    selector,
                    timeout,
                    deadline,
                });
                Ok(Value::Int(1))
            }
            Operation::Select { value, arms } => {
                let value = self.expression(value)?;
                for arm in arms {
                    let previous = self.locals.clone();
                    let selected = self.pattern(&arm.pattern, &value)?;
                    let selected = if selected {
                        arm.guard
                            .as_ref()
                            .map(|g| self.expression(g))
                            .transpose()?
                            .unwrap_or(Value::Bool(true))
                    } else {
                        Value::Bool(false)
                    };
                    let result = if selected == Value::Bool(true) {
                        Some(self.expression(&arm.body))
                    } else {
                        None
                    };
                    self.locals = previous;
                    if let Some(result) = result {
                        return result;
                    }
                }
                Ok(Value::Int(0))
            }
            Operation::IterateField { value, field } => {
                let value = self.expression(value)?;
                let number = match (value, field) {
                    (Value::Range(start, _, _), 0) => start,
                    (Value::Range(_, end, _), 1) => end,
                    (Value::Range(_, _, inclusive), 2) => i64::from(inclusive),
                    (Value::List(values), 1) => values.len() as i64,
                    (Value::Map(values), 1) => values.len() as i64,
                    (Value::List(_) | Value::Map(_), 0 | 2) => 0,
                    _ => return Err(fault("invalid actor iteration field")),
                };
                Ok(Value::Int(number))
            }
            Operation::IterateItem { value, index, .. } => {
                let value = self.expression(value)?;
                let Value::Int(index) = self.expression(index)? else {
                    return Err(fault("invalid actor iteration index"));
                };
                match value {
                    Value::Range(..) => Ok(Value::Int(index)),
                    Value::List(values) => usize::try_from(index)
                        .ok()
                        .and_then(|i| values.get(i))
                        .cloned()
                        .ok_or_else(|| fault("actor list index outside bounds")),
                    Value::Map(values) => usize::try_from(index)
                        .ok()
                        .and_then(|i| values.get(i))
                        .map(|(k, v)| Value::Sum(0, Rc::new(vec![k.clone(), v.clone()])))
                        .ok_or_else(|| fault("actor map index outside bounds")),
                    _ => Err(fault("invalid actor iterable")),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dynamic_identity_cannot_reinterpret_a_different_program_with_the_same_id() {
        let syntax = parse::parse("fn main(): ()").unwrap();
        let program = Rc::new(check::check(&syntax).unwrap());
        let original = Rc::new((*program).clone());
        let function = program.functions[0].id;
        let operation = Operation::ClosureIdentity {
            function,
            value: Box::new(ir::Expr {
                kind: ir::ExprKind::Local(ir::LocalId(0)),
                ty: Type::Function(vec![], Box::new(Type::Unit)),
                span: crate::Span::default(),
            }),
        };
        let mut machine = Machine::new(program.clone(), HashMap::new());
        for (origin, expected) in [(original, false), (program, true)] {
            machine.locals.insert(
                0,
                Value::Closure(Rc::new(ClosureValue {
                    program: origin,
                    function,
                    captures: vec![],
                    actor_entries: Rc::default(),
                })),
            );
            assert_eq!(
                machine.actor_operation(&operation).unwrap(),
                Value::Bool(expected)
            );
        }
    }
}
