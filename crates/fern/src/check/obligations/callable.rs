//! Known callable identities preserve their environments; structural Fn alone proves no handling.
use super::*;
impl Engine<'_> {
    /// Capture values in source order without treating creation of a callable as executing its body.
    pub(super) fn closure(
        &mut self,
        function: usize,
        captures: &[ir::Expr],
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let captures = self.arguments(captures, depth, span)?;
        self.node(Region::Callable { function, captures }, span)
    }
    /// Invoke a known target or each guarded target without merging their handling guarantees.
    pub(super) fn invoke(
        &mut self,
        callee: &Value,
        args: &[Value],
        result: &Type,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        match &callee.node.kind {
            Region::Callable { function, captures } => {
                self.charge(args.len().saturating_add(captures.len()), span)?;
                let mut values = args.to_vec();
                values.extend(captures.iter().cloned());
                if self.relevance.is_some_and(|set| !set.contains(function)) {
                    self.fresh(result, None, span, 0)
                } else {
                    self.source_call(*function, &values, span)
                }
            }
            Region::UnknownCallable => {
                for value in args {
                    self.require_callback(value, span)?;
                }
                self.fresh(result, None, span, 0)
            }
            Region::Choice(choices) => self.invoke_choices(choices, args, result, span, depth + 1),
            _ => self.unsupported(span),
        }
    }

    /// A callback requirement records a possible future proof, never acknowledged ownership.
    pub(super) fn require_callback(&mut self, value: &Value, span: Span) -> Checked<()> {
        for (id, guard) in self.origins_of(value, false, span)? {
            let required = self
                .predicates
                .and(self.path, guard, &mut self.work, span)?;
            let previous = self
                .pending_callbacks
                .get(&id)
                .copied()
                .unwrap_or(Predicate::FALSE);
            let required = self
                .predicates
                .or(previous, required, &mut self.work, span)?;
            self.pending_callbacks.insert(id, required);
        }
        Ok(())
    }
    /// Each possible target retains its actual branch guard and independent normal successor.
    fn invoke_choices(
        &mut self,
        choices: &[(Predicate, Value)],
        args: &[Value],
        result: &Type,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        if choices.len() > 128 {
            return Err(Diagnostic::new(
                span,
                "Result obligation callable target limit exceeded",
            ));
        }
        self.charge(choices.len(), span)?;
        let parent = self.path;
        let mut outputs = Vec::new();
        for (guard, callee) in choices {
            self.path = self.predicates.and(parent, *guard, &mut self.work, span)?;
            if self.path != Predicate::FALSE {
                let output = self.invoke(callee, args, result, span, depth)?;
                outputs.push((self.path, output));
            }
        }
        self.path = Predicate::FALSE;
        for (guard, _) in &outputs {
            self.path = self
                .predicates
                .or(self.path, *guard, &mut self.work, span)?;
        }
        self.node(Region::Choice(outputs), span)
    }
}
