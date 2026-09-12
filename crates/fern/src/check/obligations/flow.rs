//! Reachable exits retain correlated origins and execute only registered mandatory cleanup.
use super::*;
impl Engine<'_> {
    /// Preserve each exit's output identity and predicates for subsequent caller substitution.
    pub(super) fn output(&mut self, span: Span) -> Checked<Value> {
        let values = std::mem::take(&mut self.returned_values);
        if values.len() == 1 && values[0].0 == Predicate::TRUE {
            return Ok(values.into_iter().next().unwrap().1);
        }
        self.node(Region::Choice(values), span)
    }
    /// Propagation produces an outer Err for its caller while retaining the original error payload.
    fn error_value(&mut self, value: &Value, span: Span) -> Checked<Value> {
        let origin = Some(self.origin(None, span)?);
        self.node(
            Region::Sum {
                origin,
                tag: Some(1),
                guards: vec![Predicate::FALSE, Predicate::TRUE],
                variants: vec![vec![], vec![value.clone()]],
            },
            span,
        )
    }

    /// Convert a proved Boolean value to its predicate; partial views never share family truth.
    pub(super) fn condition(&mut self, value: &Value, span: Span) -> Checked<Predicate> {
        if !value.complete {
            return self.predicates.variable(&mut self.work, span);
        }
        match &value.node.kind {
            Region::EditorBorrow => self.unsupported(span),
            Region::Boolean(path) => Ok(*path),
            Region::Scalar(Key::Bool(value)) => Ok(if *value {
                Predicate::TRUE
            } else {
                Predicate::FALSE
            }),
            Region::Choice(choices) => {
                let mut result = Predicate::FALSE;
                for (guard, value) in choices {
                    let condition = self.condition(value, span)?;
                    let selected = self
                        .predicates
                        .and(*guard, condition, &mut self.work, span)?;
                    result = self.predicates.or(result, selected, &mut self.work, span)?;
                }
                Ok(result)
            }
            _ => self.predicates.variable(&mut self.work, span),
        }
    }
    /// Acknowledgement and transfer use conditional provenance, not a union of possible aliases.
    pub(super) fn dispose(
        &mut self,
        value: &Value,
        outer: bool,
        returned: bool,
        span: Span,
    ) -> Checked<()> {
        for (id, guard) in self.origins_of(value, outer, span)? {
            let guard = self
                .predicates
                .and(self.path, guard, &mut self.work, span)?;
            let target = if returned {
                &mut self.origins[id].returned
            } else {
                &mut self.origins[id].handled
            };
            *target = self.predicates.or(*target, guard, &mut self.work, span)?;
        }
        Ok(())
    }
    /// Record one ordinary return before removing its normal successor.
    pub(super) fn exit(&mut self, value: &Value, span: Span, depth: usize) -> Checked<()> {
        if self.path == Predicate::FALSE {
            return Ok(());
        }
        self.dispose(value, false, true, span)?;
        self.charge(1, span)?;
        self.returned_values.push((self.path, value.clone()));
        self.exits = self
            .predicates
            .or(self.exits, self.path, &mut self.work, span)?;
        self.run_cleanups(span, depth + 1)?;
        self.path = Predicate::FALSE;
        Ok(())
    }
    /// A function return inside iteration remains separate from ordinary next-iteration exits.
    pub(super) fn function_exit(&mut self, value: &Value, span: Span, depth: usize) -> Checked<()> {
        if let Some(previous) = self.iteration_breaks {
            self.charge(1, span)?;
            self.iteration_returns.push((self.path, value.clone()));
            self.iteration_breaks =
                Some(
                    self.predicates
                        .or(previous, self.path, &mut self.work, span)?,
                );
        }
        self.exit(value, span, depth)
    }
    /// Join values with the exact surviving branch guards; returned paths do not continue.
    pub(super) fn conditional(
        &mut self,
        condition: &ir::Expr,
        yes: &ir::Expr,
        no: Option<&ir::Expr>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let value = self.expression(condition, depth)?;
        let condition = self.condition(&value, span)?;
        let parent = self.path;
        self.path = self
            .predicates
            .and(parent, condition, &mut self.work, span)?;
        let yes = self.expression(yes, depth)?;
        let yes_path = self.path;
        let inverse = self.predicates.not(condition, &mut self.work, span)?;
        self.path = self.predicates.and(parent, inverse, &mut self.work, span)?;
        let no = match no {
            Some(no) => self.expression(no, depth)?,
            None => self.node(Region::Empty, span)?,
        };
        let no_path = self.path;
        self.path = self
            .predicates
            .or(yes_path, no_path, &mut self.work, span)?;
        self.node(Region::Choice(vec![(yes_path, yes), (no_path, no)]), span)
    }
    /// Lazy Boolean operands run only where needed and preserve early-return successors.
    pub(super) fn logical(
        &mut self,
        op: crate::ast::BinaryOp,
        left: &ir::Expr,
        right: &ir::Expr,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let left = self.expression(left, depth)?;
        let condition = self.condition(&left, span)?;
        let parent = self.path;
        let is_and = op == crate::ast::BinaryOp::And;
        let inverse = self.predicates.not(condition, &mut self.work, span)?;
        let (run, skip) = if is_and {
            (condition, inverse)
        } else {
            (inverse, condition)
        };
        self.path = self.predicates.and(parent, run, &mut self.work, span)?;
        let right = self.expression(right, depth)?;
        let right = self.condition(&right, span)?;
        let skipped = self.predicates.and(parent, skip, &mut self.work, span)?;
        self.path = self
            .predicates
            .or(self.path, skipped, &mut self.work, span)?;
        let value = if is_and {
            self.predicates
                .and(condition, right, &mut self.work, span)?
        } else {
            self.predicates.or(condition, right, &mut self.work, span)?
        };
        self.node(Region::Boolean(value), span)
    }
    /// Propagation acknowledges the outer Result while preserving earlier duties on the Err exit.
    pub(super) fn propagate(
        &mut self,
        expr: &ir::Expr,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let value = self.expression(expr, depth)?;
        let parent = self.path;
        self.dispose(&value, true, false, span)?;
        let (ok, values) = self.variant(&value, 0, span, depth)?;
        let (err, errors) = self.variant(&value, 1, span, depth)?;
        self.path = self.predicates.and(parent, err, &mut self.work, span)?;
        if self.path != Predicate::FALSE {
            let error = errors
                .first()
                .ok_or_else(|| Diagnostic::new(span, "missing propagated Result payload"))?;
            let error = self.error_value(error, span)?;
            self.function_exit(&error, span, depth)?;
        }
        self.path = self.predicates.and(parent, ok, &mut self.work, span)?;
        values
            .first()
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.node(Region::Empty, span))
    }
    /// Registration captures lexical snapshots; only actual exit paths run their effects.
    pub(super) fn defer(&mut self, value: &ir::Expr, span: Span, depth: usize) -> Checked<Value> {
        let ir::ExprKind::Closure { function, captures } = &value.kind else {
            return self.unsupported(span);
        };
        let captures = self.arguments(captures, depth, span)?;
        self.charge(1, span)?;
        self.cleanups.push(Cleanup {
            guard: self.path,
            function: function.0,
            captures,
            span,
        });
        self.node(Region::Empty, span)
    }
    /// Execute registered cleanup in reverse order without consuming another exit's registrations.
    fn run_cleanups(&mut self, span: Span, depth: usize) -> Checked<()> {
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation cleanup depth limit exceeded",
            ));
        }
        let count = self
            .cleanups
            .iter()
            .try_fold(self.cleanups.len(), |n, c| n.checked_add(c.captures.len()))
            .ok_or_else(|| {
                Diagnostic::new(span, "Result obligation cleanup size limit exceeded")
            })?;
        self.charge(count, span)?;
        let cleanups = self.cleanups.clone();
        let parent = self.path;
        for cleanup in cleanups.iter().rev() {
            self.path = self
                .predicates
                .and(parent, cleanup.guard, &mut self.work, span)?;
            if self.path != Predicate::FALSE {
                self.cleanup(cleanup, depth)?;
            }
        }
        self.path = parent;
        Ok(())
    }
    /// Cleanup has its own local namespace and nested defer stack, sharing only captured regions.
    fn cleanup(&mut self, cleanup: &Cleanup, depth: usize) -> Checked<()> {
        self.charge(self.program.functions.len(), cleanup.span)?;
        let function = self
            .program
            .functions
            .iter()
            .find(|f| f.id.0 == cleanup.function)
            .ok_or_else(|| Diagnostic::new(cleanup.span, "missing cleanup obligation function"))?;
        if !function.params.is_empty() || function.captures.len() != cleanup.captures.len() {
            return self.unsupported(cleanup.span);
        }
        let locals = std::mem::take(&mut self.locals);
        let params = std::mem::take(&mut self.parameters);
        let cleanups = std::mem::take(&mut self.cleanups);
        self.charge(function.captures.len(), cleanup.span)?;
        for (param, value) in function.captures.iter().zip(&cleanup.captures) {
            self.locals.insert(param.id.0, value.clone());
        }
        self.expression(&function.body, depth + 1)?;
        self.run_cleanups(cleanup.span, depth + 1)?;
        self.locals = locals;
        self.parameters = params;
        self.cleanups = cleanups;
        Ok(())
    }
    /// Let-else keeps only successful pattern paths; the existing type checker proves divergence.
    pub(super) fn let_else(
        &mut self,
        pattern: &ir::Pattern,
        value: &ir::Expr,
        otherwise: &ir::Expr,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        let value = self.expression(value, depth)?;
        let parent = self.path;
        let selected = self.pattern(pattern, &value, span, depth)?;
        let inverse = self.predicates.not(selected, &mut self.work, span)?;
        self.path = self.predicates.and(parent, inverse, &mut self.work, span)?;
        self.expression(otherwise, depth)?;
        if self.path != Predicate::FALSE {
            return self.unsupported(span);
        }
        self.path = self
            .predicates
            .and(parent, selected, &mut self.work, span)?;
        Ok(())
    }
    /// With accumulates handled failure outputs while later steps see only successful paths.
    pub(super) fn with(
        &mut self,
        steps: &[ir::WithStep],
        body: &ir::Expr,
        handlers: &[ir::WithHandler],
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(steps.len().saturating_add(handlers.len()), span)?;
        let mut output = Vec::new();
        for step in steps {
            if self.path == Predicate::FALSE {
                break;
            }
            let value = self.expression(&step.value, depth)?;
            let parent = self.path;
            self.dispose(&value, true, false, span)?;
            let (ok, values) = self.variant(&value, 0, span, depth)?;
            let (err, errors) = self.variant(&value, 1, span, depth)?;
            self.path = self.predicates.and(parent, err, &mut self.work, span)?;
            if self.path != Predicate::FALSE {
                let error = errors
                    .first()
                    .ok_or_else(|| Diagnostic::new(span, "missing with error payload"))?;
                if let Some(index) = step.error_handler {
                    let handler = handlers
                        .get(index)
                        .ok_or_else(|| Diagnostic::new(span, "invalid with handler"))?;
                    self.locals.insert(handler.error.id.0, error.clone());
                    let value = self.expression(&handler.body, depth)?;
                    output.push((self.path, value));
                } else {
                    let error = self.error_value(error, span)?;
                    self.function_exit(&error, span, depth)?;
                }
            }
            self.path = self.predicates.and(parent, ok, &mut self.work, span)?;
            if self.path != Predicate::FALSE {
                let value = values
                    .first()
                    .ok_or_else(|| Diagnostic::new(span, "missing with success payload"))?;
                let matched = self.pattern(&step.pattern, value, span, depth)?;
                self.path = self
                    .predicates
                    .and(self.path, matched, &mut self.work, span)?;
            }
        }
        let body = self.expression(body, depth)?;
        output.push((self.path, body));
        self.path = Predicate::FALSE;
        for (path, _) in &output {
            self.path = self.predicates.or(self.path, *path, &mut self.work, span)?;
        }
        self.node(Region::Choice(output), span)
    }
}
