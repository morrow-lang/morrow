//! Pattern paths acknowledge explicit Result tags while named catchalls retain pending values.
use super::*;
impl Engine<'_> {
    /// Select a variant with guarded payload views, including phi-selected sum aliases.
    pub(super) fn variant(
        &mut self,
        value: &Value,
        tag: usize,
        span: Span,
        depth: usize,
    ) -> Checked<(Predicate, Vec<Value>)> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation pattern depth limit exceeded",
            ));
        }
        match &value.node.kind {
            Region::Nominal { .. } => {
                let expanded = self.expand_nominal(value, span, depth + 1)?;
                self.variant(&expanded, tag, span, depth + 1)
            }
            Region::Sum {
                guards, variants, ..
            } => {
                let guard = guards.get(tag).copied().unwrap_or(Predicate::FALSE);
                let fields = variants.get(tag).map_or(&[][..], Vec::as_slice);
                self.charge(fields.len(), span)?;
                let fields = fields
                    .iter()
                    .map(|v| {
                        if value.complete {
                            v.clone()
                        } else {
                            v.partial()
                        }
                    })
                    .collect();
                Ok((guard, fields))
            }
            Region::Choice(choices) => {
                self.choice_variant(choices, value.complete, tag, span, depth + 1)
            }
            _ => self.unsupported(span),
        }
    }
    /// Guard each projected field by its selected alternative, never crediting all MAY aliases.
    fn choice_variant(
        &mut self,
        choices: &[(Predicate, Value)],
        complete: bool,
        tag: usize,
        span: Span,
        depth: usize,
    ) -> Checked<(Predicate, Vec<Value>)> {
        let mut condition = Predicate::FALSE;
        let mut fields: Vec<Vec<(Predicate, Value)>> = Vec::new();
        self.charge(choices.len(), span)?;
        for (guard, value) in choices {
            let (selected, values) = self.variant(value, tag, span, depth)?;
            let selected = self
                .predicates
                .and(*guard, selected, &mut self.work, span)?;
            condition = self
                .predicates
                .or(condition, selected, &mut self.work, span)?;
            if selected == Predicate::FALSE {
                continue;
            }
            self.charge(values.len(), span)?;
            if fields.is_empty() {
                fields.resize_with(values.len(), Vec::new);
            }
            if fields.len() != values.len() {
                return self.unsupported(span);
            }
            for (field, value) in fields.iter_mut().zip(values) {
                field.push((selected, if complete { value } else { value.partial() }));
            }
        }
        let values = fields
            .into_iter()
            .map(|field| self.node(Region::Choice(field), span))
            .collect::<Checked<_>>()?;
        Ok((condition, values))
    }
    /// Match arms inherit only predecessors for which earlier patterns or guards did not succeed.
    pub(super) fn matching(
        &mut self,
        value: &ir::Expr,
        arms: &[ir::MatchArm],
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let value = self.expression(value, depth)?;
        self.matching_region(value, arms, None, false, span, depth)
    }
    /// A receive may remain suspended on unmatched messages; timeout adds an independent exit path.
    pub(super) fn matching_region(
        &mut self,
        value: Value,
        arms: &[ir::MatchArm],
        timeout: Option<&ir::Expr>,
        partial: bool,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let entry = self.path;
        let mut timeout_path = Predicate::FALSE;
        if timeout.is_some() {
            let ready = self.predicates.variable(&mut self.work, span)?;
            let inverse = self.predicates.not(ready, &mut self.work, span)?;
            self.path = self.predicates.and(entry, ready, &mut self.work, span)?;
            timeout_path = self.predicates.and(entry, inverse, &mut self.work, span)?;
        }
        let mut pending = self.path;
        let mut output = Vec::new();
        self.charge(arms.len(), span)?;
        for arm in arms {
            if pending == Predicate::FALSE {
                break;
            }
            self.path = pending;
            let selected = self.pattern(&arm.pattern, &value, arm.span, depth)?;
            let inverse = self.predicates.not(selected, &mut self.work, span)?;
            let unmatched = self
                .predicates
                .and(pending, inverse, &mut self.work, span)?;
            self.path = self
                .predicates
                .and(pending, selected, &mut self.work, span)?;
            let guard = if let Some(guard) = &arm.guard {
                let guard = self.expression(guard, depth)?;
                self.condition(&guard, span)?
            } else {
                Predicate::TRUE
            };
            let inverse = self.predicates.not(guard, &mut self.work, span)?;
            let failed_guard = self
                .predicates
                .and(self.path, inverse, &mut self.work, span)?;
            pending = self
                .predicates
                .or(unmatched, failed_guard, &mut self.work, span)?;
            self.path = self
                .predicates
                .and(self.path, guard, &mut self.work, span)?;
            let body = self.expression(&arm.body, depth)?;
            output.push((self.path, body));
        }
        if let Some(body) = timeout {
            self.path = timeout_path;
            let value = self.expression(body, depth)?;
            output.push((self.path, value));
        }
        self.path = Predicate::FALSE;
        for (path, _) in &output {
            self.path = self.predicates.or(self.path, *path, &mut self.work, span)?;
        }
        if !partial && pending != Predicate::FALSE {
            return self.unsupported(span);
        }
        self.node(Region::Choice(output), span)
    }
    /// Wildcards acknowledge nothing; only explicit Result constructor tests inspect the outer tag.
    pub(super) fn pattern(
        &mut self,
        pattern: &ir::Pattern,
        value: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation pattern depth limit exceeded",
            ));
        }
        use ir::Pattern as P;
        match pattern {
            P::UnionSelect { narrowed, binding } => {
                let (guard, value) = self.union_narrow(value, narrowed, span, depth + 1)?;
                if let Some(binding) = binding {
                    self.locals.insert(binding.id.0, value);
                }
                Ok(guard)
            }
            P::List { .. } | P::TupleRest { .. } => {
                self.sequence_pattern(pattern, value, span, depth + 1)
            }
            P::Wildcard => Ok(Predicate::TRUE),
            P::Bind(id) => {
                self.locals.insert(id.0, value.clone());
                Ok(Predicate::TRUE)
            }
            P::Newtype(inner) => self.pattern(inner, value, span, depth + 1),
            P::Bool(expected) => {
                let actual = self.condition(value, span)?;
                if *expected {
                    Ok(actual)
                } else {
                    self.predicates.not(actual, &mut self.work, span)
                }
            }
            P::Int(expected) => self.scalar_pattern(value, &Key::Int(*expected), span),
            P::String(expected) => self.scalar_pattern(value, &Key::String(expected.clone()), span),
            P::Constructor {
                constructor,
                binding,
            } => {
                self.dispose(value, true, false, span)?;
                let tag = usize::from(matches!(constructor, Constructor::None | Constructor::Err));
                let (guard, fields) = self.variant(value, tag, span, depth + 1)?;
                if let (Some(id), Some(field)) = (binding, fields.first()) {
                    self.locals.insert(id.0, field.clone());
                }
                Ok(guard)
            }
            P::Variant { tag, fields } => {
                self.dispose(value, true, false, span)?;
                let (guard, values) = self.variant(value, *tag, span, depth + 1)?;
                self.pattern_fields(fields, &values, guard, span, depth + 1)
            }
            P::Tuple(fields) => {
                let mut values = Vec::new();
                self.charge(fields.len(), span)?;
                for i in 0..fields.len() {
                    values.push(self.field(value, i, span)?);
                }
                self.pattern_fields(fields, &values, Predicate::TRUE, span, depth + 1)
            }
        }
    }
    /// Inner patterns run only after the enclosing variant has matched.
    pub(super) fn pattern_fields(
        &mut self,
        patterns: &[ir::Pattern],
        values: &[Value],
        mut guard: Predicate,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        if guard == Predicate::FALSE {
            return Ok(guard);
        }
        if patterns.len() != values.len() {
            return self.unsupported(span);
        }
        let parent = self.path;
        self.charge(patterns.len(), span)?;
        for (pattern, value) in patterns.iter().zip(values) {
            self.path = self.predicates.and(parent, guard, &mut self.work, span)?;
            let selected = self.pattern(pattern, value, span, depth)?;
            guard = self.predicates.and(guard, selected, &mut self.work, span)?;
        }
        self.path = parent;
        Ok(guard)
    }
    /// Known literal equality is exact; unknown scalar tests are conservative independent choices.
    fn scalar_pattern(&mut self, value: &Value, expected: &Key, span: Span) -> Checked<Predicate> {
        match &value.node.kind {
            Region::Scalar(actual) => Ok(if actual == expected {
                Predicate::TRUE
            } else {
                Predicate::FALSE
            }),
            _ => self.predicates.variable(&mut self.work, span),
        }
    }
}
