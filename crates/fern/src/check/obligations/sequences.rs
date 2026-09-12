//! Sequence patterns partition retained identities; a prefix never acknowledges its suffix.
use super::*;
impl Engine<'_> {
    /// Known layouts split exactly; unknown-length prefixes require a separate family partition proof.
    pub(super) fn sequence_pattern(
        &mut self,
        pattern: &ir::Pattern,
        value: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        if let Region::Choice(choices) = &value.node.kind {
            return self.sequence_choices(pattern, choices, value.complete, span, depth + 1);
        }
        match (pattern, &value.node.kind) {
            (
                ir::Pattern::List { prefix, rest },
                Region::List {
                    items, exact: true, ..
                },
            ) => {
                if items.len() < prefix.len() || (rest.is_none() && items.len() != prefix.len()) {
                    return Ok(Predicate::FALSE);
                }
                self.sequence_fields(
                    (prefix, rest.as_deref()),
                    items,
                    (value.complete, true),
                    span,
                    depth,
                )
            }
            (ir::Pattern::List { prefix, rest }, Region::List { nonempty, .. })
                if prefix.is_empty() =>
            {
                if let Some(rest) = rest {
                    self.pattern(rest, value, span, depth)
                } else {
                    self.predicates.not(*nonempty, &mut self.work, span)
                }
            }
            (ir::Pattern::List { prefix, rest }, Region::List { .. }) => {
                self.dynamic_sequence(prefix, rest.as_deref(), value, span, depth + 1)
            }
            (ir::Pattern::TupleRest { prefix, rest }, Region::Product(items)) => {
                if items.len() < prefix.len() {
                    return self.unsupported(span);
                }
                self.sequence_fields(
                    (prefix, Some(rest)),
                    items,
                    (value.complete, false),
                    span,
                    depth,
                )
            }
            _ => self.unsupported(span),
        }
    }
    /// Partition fixed storage while propagating partial-family status into every derived view.
    fn sequence_fields(
        &mut self,
        pattern: (&[ir::Pattern], Option<&ir::Pattern>),
        items: &[Value],
        mode: (bool, bool),
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        let (prefix, rest) = pattern;
        let (complete, list) = mode;
        self.charge(items.len(), span)?;
        let mut items: Vec<_> = items
            .iter()
            .map(|v| if complete { v.clone() } else { v.partial() })
            .collect();
        let suffix = items.split_off(prefix.len());
        let mut selected = self.pattern_fields(prefix, &items, Predicate::TRUE, span, depth)?;
        if let Some(rest) = rest {
            let kind = if list {
                let nonempty = if suffix.is_empty() {
                    Predicate::FALSE
                } else {
                    Predicate::TRUE
                };
                Region::List {
                    items: suffix,
                    exact: true,
                    nonempty,
                }
            } else if suffix.is_empty() {
                Region::Empty
            } else {
                Region::Product(suffix)
            };
            let value = self.node(kind, span)?;
            let parent = self.path;
            self.path = self
                .predicates
                .and(parent, selected, &mut self.work, span)?;
            let matched = self.pattern(rest, &value, span, depth)?;
            self.path = parent;
            selected = self
                .predicates
                .and(selected, matched, &mut self.work, span)?;
        }
        Ok(selected)
    }
    /// A branch-selected sequence binds each name to its guarded value, never a union of aliases.
    fn sequence_choices(
        &mut self,
        pattern: &ir::Pattern,
        choices: &[(Predicate, Value)],
        complete: bool,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        let ids = self.sequence_bindings(pattern, span)?;
        let parent = self.path;
        let mut selected = Predicate::FALSE;
        let mut bindings: BTreeMap<usize, Vec<(Predicate, Value)>> = BTreeMap::new();
        self.charge(choices.len(), span)?;
        for (guard, value) in choices {
            self.path = self.predicates.and(parent, *guard, &mut self.work, span)?;
            if self.path == Predicate::FALSE {
                continue;
            }
            let value = if complete {
                value.clone()
            } else {
                value.partial()
            };
            let matched = self.sequence_pattern(pattern, &value, span, depth)?;
            let matched = self.predicates.and(*guard, matched, &mut self.work, span)?;
            selected = self
                .predicates
                .or(selected, matched, &mut self.work, span)?;
            if matched == Predicate::FALSE {
                continue;
            }
            for id in &ids {
                self.charge(1, span)?;
                let value = self
                    .locals
                    .get(id)
                    .cloned()
                    .ok_or_else(|| Diagnostic::new(span, "missing Result sequence binding"))?;
                bindings.entry(*id).or_default().push((matched, value));
            }
        }
        self.path = parent;
        for (id, values) in bindings {
            let value = self.node(Region::Choice(values), span)?;
            self.locals.insert(id, value);
        }
        Ok(selected)
    }
    /// Collect only pattern-owned local identities using the already bounded typed pattern graph.
    fn sequence_bindings(&mut self, pattern: &ir::Pattern, span: Span) -> Checked<Vec<usize>> {
        let mut pending = vec![pattern];
        let mut ids = Vec::new();
        while let Some(pattern) = pending.pop() {
            self.charge(1, span)?;
            match pattern {
                ir::Pattern::Bind(id)
                | ir::Pattern::Constructor {
                    binding: Some(id), ..
                } => ids.push(id.0),
                ir::Pattern::UnionSelect {
                    binding: Some(param),
                    ..
                } => ids.push(param.id.0),
                ir::Pattern::Newtype(inner) => pending.push(inner),
                ir::Pattern::Tuple(fields) | ir::Pattern::Variant { fields, .. } => {
                    self.charge(fields.len(), span)?;
                    pending.extend(fields);
                }
                ir::Pattern::List { prefix, rest } => {
                    self.charge(prefix.len().saturating_add(1), span)?;
                    pending.extend(prefix);
                    if let Some(rest) = rest {
                        pending.push(rest);
                    }
                }
                ir::Pattern::TupleRest { prefix, rest } => {
                    self.charge(prefix.len().saturating_add(1), span)?;
                    pending.extend(prefix);
                    pending.push(rest);
                }
                _ => {}
            }
        }
        Ok(ids)
    }
}
