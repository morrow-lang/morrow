//! Existence predicates prevent empty traversals from manufacturing callback-produced duties.
use super::*;
impl Engine<'_> {
    /// A dynamic list's representative exists only when the list contains at least one element.
    pub(super) fn fresh_list(
        &mut self,
        item: &Type,
        input: Option<usize>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let nonempty = self.predicates.variable(&mut self.work, span)?;
        let parent = self.path;
        self.path = self
            .predicates
            .and(parent, nonempty, &mut self.work, span)?;
        let value = self.fresh(item, input, span, depth)?;
        self.path = parent;
        self.node(
            Region::List {
                items: vec![value],
                exact: false,
                nonempty,
            },
            span,
        )
    }
    /// A dynamic map's representative exists only when the list contains at least one element.
    pub(super) fn fresh_map(
        &mut self,
        item: &Type,
        input: Option<usize>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let nonempty = self.predicates.variable(&mut self.work, span)?;
        let parent = self.path;
        self.path = self
            .predicates
            .and(parent, nonempty, &mut self.work, span)?;
        let value = self.fresh(item, input, span, depth)?;
        self.path = parent;
        self.node(
            Region::Map {
                entries: vec![(None, value)],
                exact: false,
                nonempty,
            },
            span,
        )
    }
    /// Conditional collection identities retain their original branch and cardinality guards.
    pub(super) fn list_nonempty(
        &mut self,
        value: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        match &value.node.kind {
            Region::List { nonempty, .. } => Ok(*nonempty),
            Region::Choice(choices) => {
                self.charge(choices.len(), span)?;
                let mut result = Predicate::FALSE;
                for (guard, value) in choices {
                    let nonempty = self.list_nonempty(value, span, depth + 1)?;
                    let nonempty = self
                        .predicates
                        .and(*guard, nonempty, &mut self.work, span)?;
                    result = self.predicates.or(result, nonempty, &mut self.work, span)?;
                }
                Ok(result)
            }
            _ => self.unsupported(span),
        }
    }
    /// Conditional collection identities retain their original branch and cardinality guards.
    pub(super) fn map_nonempty(
        &mut self,
        value: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        match &value.node.kind {
            Region::Map { nonempty, .. } => Ok(*nonempty),
            Region::Choice(choices) => {
                self.charge(choices.len(), span)?;
                let mut result = Predicate::FALSE;
                for (guard, value) in choices {
                    let nonempty = self.map_nonempty(value, span, depth + 1)?;
                    let nonempty = self
                        .predicates
                        .and(*guard, nonempty, &mut self.work, span)?;
                    result = self.predicates.or(result, nonempty, &mut self.work, span)?;
                }
                Ok(result)
            }
            _ => self.unsupported(span),
        }
    }
    /// Removing a dynamic head may empty its tail; permutations preserve cardinality exactly.
    pub(super) fn transformed_nonempty(
        &mut self,
        builtin: ir::Builtin,
        list: &Value,
        exact: bool,
        length: usize,
        span: Span,
    ) -> Checked<Predicate> {
        if exact {
            return Ok(if length == 0 {
                Predicate::FALSE
            } else {
                Predicate::TRUE
            });
        }
        let nonempty = self.list_nonempty(list, span, 0)?;
        if builtin != ir::Builtin::ListTail {
            return Ok(nonempty);
        }
        let longer = self.predicates.variable(&mut self.work, span)?;
        self.predicates.and(nonempty, longer, &mut self.work, span)
    }
}
