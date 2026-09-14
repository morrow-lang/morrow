//! Lookup and deletion preserve retained aliases without acknowledging removed Result values.
use super::*;
impl Engine<'_> {
    /// Branch-selected maps keep their guards; unknown lookup keys yield partial payload views.
    pub(super) fn map_view(
        &mut self,
        builtin: ir::Builtin,
        map: &Value,
        key: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        if let Region::Choice(choices) = &map.node.kind {
            self.charge(choices.len(), span)?;
            let mut output = Vec::new();
            for (guard, value) in choices {
                let value = if map.complete {
                    value.clone()
                } else {
                    value.partial()
                };
                output.push((
                    *guard,
                    self.map_view(builtin, &value, key, span, depth + 1)?,
                ));
            }
            return self.node(Region::Choice(output), span);
        }
        let Region::Map { entries, exact, .. } = &map.node.kind else {
            return self.unsupported(span);
        };
        let key = self.key(key, span)?;
        self.charge(entries.len(), span)?;
        for (existing, _) in entries {
            self.charge(
                intrinsics::key_weight(existing.as_ref())
                    .saturating_add(intrinsics::key_weight(key.as_ref())),
                span,
            )?;
        }
        if *exact && key.is_some() && entries.iter().all(|(key, _)| key.is_some()) {
            let index = entries.iter().position(|(existing, _)| *existing == key);
            return self.exact_map_view(builtin, map, entries, index, span);
        }
        if builtin == ir::Builtin::MapDelete {
            return self.dynamic_map_delete(map, entries, span);
        }
        let choices = entries
            .iter()
            .map(|(_, v)| (Predicate::TRUE, v.partial()))
            .collect();
        let value = self.node(Region::Choice(choices), span)?;
        let guards = self.alternatives(2, span)?;
        self.node(
            Region::Sum {
                origin: None,
                tag: None,
                guards,
                variants: vec![vec![value], vec![]],
            },
            span,
        )
    }
    /// Deletion may remove the final entry; retained values cannot stand for the original whole map.
    fn dynamic_map_delete(
        &mut self,
        map: &Value,
        entries: &[(Option<Key>, Value)],
        span: Span,
    ) -> Checked<Value> {
        let old = self.map_nonempty(map, span, 0)?;
        let remains = self.predicates.variable(&mut self.work, span)?;
        let nonempty = self.predicates.and(old, remains, &mut self.work, span)?;
        self.charge(entries.len(), span)?;
        let entries = entries
            .iter()
            .map(|(_, value)| (None, value.partial()))
            .collect();
        self.node(
            Region::Map {
                entries,
                exact: false,
                nonempty,
            },
            span,
        )
    }
    /// A concrete key identifies one entry; all untouched entries retain their original obligations.
    fn exact_map_view(
        &mut self,
        builtin: ir::Builtin,
        map: &Value,
        entries: &[(Option<Key>, Value)],
        index: Option<usize>,
        span: Span,
    ) -> Checked<Value> {
        if builtin == ir::Builtin::MapGet {
            let value = index
                .and_then(|i| entries.get(i))
                .map(|(_, v)| if map.complete { v.clone() } else { v.partial() });
            return self.sum_value(false, usize::from(value.is_none()), value, span);
        }
        self.charge(entries.len(), span)?;
        let entries = entries
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != index)
            .map(|(_, entry)| entry.clone())
            .collect::<Vec<_>>();
        let value = self.node(
            Region::Map {
                nonempty: if entries.is_empty() {
                    Predicate::FALSE
                } else {
                    Predicate::TRUE
                },
                entries,
                exact: true,
            },
            span,
        )?;
        Ok(if map.complete { value } else { value.partial() })
    }
}
