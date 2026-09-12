//! A quantified family element belongs to one prefix position or the retained suffix, never both.
use super::*;
#[derive(Clone)]
pub(super) struct Partition {
    lengths: Vec<Predicate>,
    positions: Vec<Predicate>,
    remainder: Predicate,
}
impl Engine<'_> {
    /// Stable length and representative-position predicates are shared by aliases of one sequence.
    fn sequence_partition(
        &mut self,
        value: &Value,
        count: usize,
        span: Span,
    ) -> Checked<Partition> {
        if count > DEPTH_LIMIT {
            return self.unsupported(span);
        }
        let nonempty = self.list_nonempty(value, span, 0)?;
        let base = self
            .sequence_offsets
            .get(&value.node.id)
            .map_or(value.node.id, |(base, _)| *base);
        let mut state = self.partitions.remove(&base).unwrap_or_else(|| Partition {
            lengths: vec![Predicate::TRUE, nonempty],
            positions: vec![],
            remainder: nonempty,
        });
        while state.positions.len() < count {
            self.charge(1, span)?;
            let previous = state.lengths.last().copied().unwrap_or(Predicate::FALSE);
            let variable = self.predicates.variable(&mut self.work, span)?;
            let longer = self
                .predicates
                .and(previous, variable, &mut self.work, span)?;
            state.lengths.push(longer);
            let chosen = self.predicates.variable(&mut self.work, span)?;
            let last = self.predicates.not(longer, &mut self.work, span)?;
            let selected = self.predicates.or(last, chosen, &mut self.work, span)?;
            let position = self
                .predicates
                .and(state.remainder, selected, &mut self.work, span)?;
            state.positions.push(position);
            let inverse = self.predicates.not(selected, &mut self.work, span)?;
            state.remainder =
                self.predicates
                    .and(state.remainder, inverse, &mut self.work, span)?;
        }
        self.charge(
            state.lengths.len().saturating_add(state.positions.len()),
            span,
        )?;
        self.partitions.insert(base, state.clone());
        Ok(state)
    }
    /// Prefix projections cover only their quantified position; a suffix covers every other position.
    pub(super) fn dynamic_sequence(
        &mut self,
        prefix: &[ir::Pattern],
        rest: Option<&ir::Pattern>,
        value: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        let (base, offset) = self
            .sequence_offsets
            .get(&value.node.id)
            .copied()
            .unwrap_or((value.node.id, 0));
        let end = offset
            .checked_add(prefix.len())
            .ok_or_else(|| Diagnostic::new(span, "Result sequence partition limit exceeded"))?;
        let state = self.sequence_partition(value, end, span)?;
        let family = substitute::Substitution::family(self, value, false, span, depth)?;
        let mut fields = Vec::new();
        self.charge(prefix.len(), span)?;
        for position in state.positions.iter().skip(offset).take(prefix.len()) {
            fields.push(self.partition_view(&family, *position, span, depth)?);
        }
        let longer = state.lengths[end + 1];
        let mut selected = state.lengths[end];
        if rest.is_none() {
            let exact = self.predicates.not(longer, &mut self.work, span)?;
            selected = self.predicates.and(selected, exact, &mut self.work, span)?;
        }
        selected = self.pattern_fields(prefix, &fields, selected, span, depth)?;
        if let Some(rest) = rest {
            let mut mask = self.list_nonempty(value, span, depth)?;
            for position in state.positions.iter().take(end) {
                let inverse = self.predicates.not(*position, &mut self.work, span)?;
                mask = self.predicates.and(mask, inverse, &mut self.work, span)?;
            }
            let item = self.partition_view(&family, mask, span, depth)?;
            let tail = self.node(
                Region::List {
                    items: vec![item],
                    exact: false,
                    nonempty: longer,
                },
                span,
            )?;
            self.sequence_offsets.insert(tail.node.id, (base, end));
            let matched = self.pattern(rest, &tail, span, depth)?;
            selected = self
                .predicates
                .and(selected, matched, &mut self.work, span)?;
        }
        Ok(selected)
    }
    /// A non-representative element still has an independent runtime tag, but carries no family duty.
    fn partition_view(
        &mut self,
        family: &Value,
        guard: Predicate,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let other = self.predicates.not(guard, &mut self.work, span)?;
        let shadow = self.shadow_value(family, span, depth + 1)?;
        self.node(
            Region::Choice(vec![(guard, family.clone()), (other, shadow)]),
            span,
        )
    }
    /// Clone only runtime shape with independent selectors; no fresh or existing obligation is added.
    fn shadow_value(&mut self, value: &Value, span: Span, depth: usize) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        let kind = match &value.node.kind {
            Region::Empty | Region::Scalar(_) => Region::Empty,
            Region::EditorBorrow => Region::EditorBorrow,
            Region::Symbolic => Region::Symbolic,
            Region::UnknownCallable => Region::UnknownCallable,
            Region::Boolean(_) => Region::Boolean(self.predicates.variable(&mut self.work, span)?),
            Region::Sum { variants, .. } => {
                let guards = self.alternatives(variants.len(), span)?;
                let mut fields = Vec::new();
                for variant in variants {
                    fields.push(self.shadow_fields(variant, span, depth + 1)?);
                }
                Region::Sum {
                    origin: None,
                    tag: None,
                    guards,
                    variants: fields,
                }
            }
            Region::Product(fields) => {
                Region::Product(self.shadow_fields(fields, span, depth + 1)?)
            }
            Region::Callable { function, captures } => Region::Callable {
                function: *function,
                captures: self.shadow_fields(captures, span, depth + 1)?,
            },
            Region::RecursiveCut { layout, .. } => Region::RecursiveCut {
                layout: *layout,
                origin: None,
                retained: Vec::new(),
            },
            Region::Nominal { layout, .. } => Region::Nominal {
                layout: *layout,
                expanded: RefCell::new(None),
            },
            Region::Union { members, value } => {
                self.charge(members.len(), span)?;
                for ty in members {
                    gate::type_cost(ty, &mut self.work, span)?;
                }
                Region::Union {
                    members: members.clone(),
                    value: self.shadow_value(value, span, depth + 1)?,
                }
            }
            _ => return self.shadow_container(value, span, depth + 1),
        };
        self.node(kind, span)
    }
    /// Bound each product allocation before recursively rebuilding its element shapes.
    fn shadow_fields(&mut self, fields: &[Value], span: Span, depth: usize) -> Checked<Vec<Value>> {
        self.charge(fields.len(), span)?;
        fields
            .iter()
            .map(|v| self.shadow_value(v, span, depth))
            .collect()
    }
    /// Collection and branch shapes preserve alternative arity without sharing representative tags.
    fn shadow_container(&mut self, value: &Value, span: Span, depth: usize) -> Checked<Value> {
        let kind = match &value.node.kind {
            Region::List { items, exact, .. } => Region::List {
                items: self.shadow_fields(items, span, depth)?,
                exact: *exact,
                nonempty: self.predicates.variable(&mut self.work, span)?,
            },
            Region::Map { entries, .. } => {
                self.charge(entries.len(), span)?;
                let entries = entries
                    .iter()
                    .map(|(_, v)| Ok((None, self.shadow_value(v, span, depth)?)))
                    .collect::<Checked<_>>()?;
                Region::Map {
                    entries,
                    exact: false,
                    nonempty: self.predicates.variable(&mut self.work, span)?,
                }
            }
            Region::Choice(choices) => {
                let guards = self.alternatives(choices.len(), span)?;
                let mut fields = Vec::new();
                for (guard, (_, value)) in guards.into_iter().zip(choices) {
                    fields.push((guard, self.shadow_value(value, span, depth)?));
                }
                Region::Choice(fields)
            }
            _ => return self.unsupported(span),
        };
        self.node(kind, span)
    }
}
