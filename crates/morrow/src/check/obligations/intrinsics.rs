//! Intrinsic handling is semantic: metadata borrows, tags acknowledge only their own layer.
use super::*;
impl Engine<'_> {
    /// Intrinsics have explicit effects; unknown operations never silently consume their inputs.
    pub(super) fn builtin(
        &mut self,
        builtin: ir::Builtin,
        args: &[Value],
        result: &Type,
        span: Span,
    ) -> Checked<Value> {
        use ir::Builtin::*;
        match (builtin, args) {
            (OptionMap | ResultMap | ResultAndThen | ResultUnwrapOrElse, [value, callback]) => {
                self.sum_call(builtin, value, callback, result, span)
            }
            (ResultIsOk | ResultIsErr, [value]) => {
                self.acknowledge(value, span)?;
                self.tag_test(value, builtin == ResultIsErr, span)
            }
            (OptionIsSome | OptionIsNone, [value]) => {
                self.tag_test(value, builtin == OptionIsNone, span)
            }
            (ResultUnwrapOr, [value, fallback]) => {
                self.acknowledge(value, span)?;
                self.unwrap(value, fallback, span)
            }
            (OptionUnwrapOr, [value, fallback]) => self.unwrap(value, fallback, span),
            (ListHead, [list]) => self.list_get(list, Some(0), span),
            (ListGet, [list, index]) => {
                let index = match &index.node.kind {
                    Region::Scalar(Key::Int(i)) => usize::try_from(*i).ok(),
                    _ => None,
                };
                self.list_get(list, index, span)
            }
            (ListTail | ListReverse | ListEnumerate, [list]) => {
                self.list_transform(builtin, list, span)
            }
            (ListPush | ListConcat, [left, right]) => self.list_combine(builtin, left, right, span),
            (MapNew, []) => self.node(
                Region::Map {
                    entries: vec![],
                    exact: true,
                    nonempty: Predicate::FALSE,
                },
                span,
            ),
            (MapGet | MapDelete, [map, key]) => self.map_view(builtin, map, key, span, 0),
            (MapPut, [map, key, value]) => self.map_put(map, key, value, span),
            (MapValues, [map]) => self.map_values(map, span),
            (ListIsEmpty, [list]) => {
                let nonempty = self.list_nonempty(list, span, 0)?;
                let empty = self.predicates.not(nonempty, &mut self.work, span)?;
                self.node(Region::Boolean(empty), span)
            }
            (MapIsEmpty, [map]) => {
                let nonempty = self.map_nonempty(map, span, 0)?;
                let empty = self.predicates.not(nonempty, &mut self.work, span)?;
                self.node(Region::Boolean(empty), span)
            }
            (StringEq | ListContains | MapContains, _) => self.fresh(&Type::Bool, None, span, 0),
            // Keys cannot contain Result values, but their list shape must survive
            // later head/get/iteration operations. This never acknowledges map values.
            (MapKeys, [_]) => self.fresh(result, None, span, 0),
            (Print | Println | StringConcat | StringLen | ListLen | MapLen, _) => {
                self.node(Region::Empty, span)
            }
            _ if !super::gate::contains(self.program, result, &mut self.work, span)? => {
                self.fresh(result, None, span, 0)
            }
            _ => self.unsupported(span),
        }
    }
    /// A tag acknowledgement covers only the selected outer tag, never nested payloads.
    fn acknowledge(&mut self, value: &Value, span: Span) -> Checked<()> {
        self.dispose(value, true, false, span)
    }
    /// Tag predicates preserve the selected value's identity without correlating distinct family elements.
    fn tag_test(&mut self, value: &Value, error: bool, span: Span) -> Checked<Value> {
        let guard = if value.complete {
            self.variant(value, usize::from(error), span, 0)?.0
        } else {
            self.predicates.variable(&mut self.work, span)?
        };
        self.node(Region::Boolean(guard), span)
    }
    /// Eager fallback duties remain live unless the selected output actually carries them.
    fn unwrap(&mut self, value: &Value, fallback: &Value, span: Span) -> Checked<Value> {
        self.unwrap_at(value, fallback, span, 0)
    }
    /// Preserve uncertainty when a dynamic element view reaches a selected-payload combinator.
    fn unwrap_at(
        &mut self,
        value: &Value,
        fallback: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation projection depth limit exceeded",
            ));
        }
        if let Region::Choice(choices) = &value.node.kind {
            self.charge(choices.len(), span)?;
            let choices = choices
                .iter()
                .map(|(guard, choice)| {
                    self.unwrap_at(choice, fallback, span, depth + 1)
                        .map(|value| (*guard, value))
                })
                .collect::<Checked<Vec<_>>>()?;
            return self.node(Region::Choice(choices), span);
        }
        let Region::Sum {
            tag,
            guards,
            variants,
            ..
        } = &value.node.kind
        else {
            return self.unsupported(span);
        };
        let payload = variants.first().and_then(|v| v.first());
        let selected = match tag {
            Some(0) => payload
                .cloned()
                .ok_or_else(|| Diagnostic::new(span, "missing obligation payload"))?,
            Some(_) => fallback.clone(),
            None => {
                let payload = payload
                    .cloned()
                    .ok_or_else(|| Diagnostic::new(span, "missing obligation payload"))?;
                self.node(
                    Region::Choice(vec![(guards[0], payload), (guards[1], fallback.clone())]),
                    span,
                )?
            }
        };
        Ok(if value.complete {
            selected
        } else {
            selected.partial()
        })
    }
    /// Exact element indexes select one identity; unknown indexes and family elements remain partial.
    fn list_get(&mut self, list: &Value, index: Option<usize>, span: Span) -> Checked<Value> {
        self.list_get_at(list, index, span, 0)
    }
    /// Conditional lists retain the selected element from each original branch independently.
    fn list_get_at(
        &mut self,
        list: &Value,
        index: Option<usize>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        if let Region::Choice(choices) = &list.node.kind {
            self.charge(choices.len(), span)?;
            let mut output = Vec::new();
            for (guard, value) in choices {
                let value = if list.complete {
                    value.clone()
                } else {
                    value.partial()
                };
                output.push((*guard, self.list_get_at(&value, index, span, depth + 1)?));
            }
            return self.node(Region::Choice(output), span);
        }
        let Region::List { items, exact, .. } = &list.node.kind else {
            return self.unsupported(span);
        };
        if *exact && let Some(item) = index.and_then(|i| items.get(i)) {
            return Ok(if list.complete {
                item.clone()
            } else {
                item.partial()
            });
        }
        self.charge(items.len(), span)?;
        let choices = items
            .iter()
            .map(|value| (Predicate::TRUE, value.partial()))
            .collect();
        self.node(Region::Choice(choices), span)
    }
    /// Whole-order-preserving transforms keep origins; tail explicitly excludes the first region.
    fn list_transform(&mut self, builtin: ir::Builtin, list: &Value, span: Span) -> Checked<Value> {
        let Region::List { items, exact, .. } = &list.node.kind else {
            return self.unsupported(span);
        };
        self.charge(items.len(), span)?;
        let mut items = items.clone();
        match builtin {
            ir::Builtin::ListReverse => items.reverse(),
            ir::Builtin::ListTail if *exact => {
                if !items.is_empty() {
                    items.remove(0);
                }
            }
            ir::Builtin::ListTail => {
                items = items.iter().map(Value::partial).collect();
            }
            ir::Builtin::ListEnumerate => {
                for item in &mut items {
                    let index = self.node(Region::Empty, span)?;
                    *item = self.node(Region::Product(vec![index, item.clone()]), span)?;
                }
            }
            _ => return self.unsupported(span),
        }
        let nonempty = self.transformed_nonempty(builtin, list, *exact, items.len(), span)?;
        let value = self.node(
            Region::List {
                items,
                nonempty,
                exact: *exact,
            },
            span,
        )?;
        Ok(if list.complete {
            value
        } else {
            value.partial()
        })
    }
    /// Concatenation and push create views over existing duties rather than handling them.
    fn list_combine(
        &mut self,
        builtin: ir::Builtin,
        left: &Value,
        right: &Value,
        span: Span,
    ) -> Checked<Value> {
        let Region::List { items, exact, .. } = &left.node.kind else {
            return self.unsupported(span);
        };
        self.charge(items.len().saturating_add(1), span)?;
        let mut output = items.clone();
        let mut exact = *exact;
        if builtin == ir::Builtin::ListPush {
            output.push(right.clone());
        } else if let Region::List {
            items,
            exact: other,
            ..
        } = &right.node.kind
        {
            self.charge(items.len(), span)?;
            output.extend(items.iter().cloned());
            exact &= other;
        } else {
            return self.unsupported(span);
        }
        let nonempty = if builtin == ir::Builtin::ListPush {
            Predicate::TRUE
        } else {
            let left = self.list_nonempty(left, span, 0)?;
            let right = self.list_nonempty(right, span, 0)?;
            self.predicates.or(left, right, &mut self.work, span)?
        };
        let value = self.node(
            Region::List {
                items: output,
                nonempty,
                exact,
            },
            span,
        )?;
        Ok(if left.complete && right.complete {
            value
        } else {
            value.partial()
        })
    }
    /// A literal key is usable only when represented exactly; do not guess runtime equality.
    pub(super) fn key(&mut self, value: &Value, span: Span) -> Checked<Option<Key>> {
        match &value.node.kind {
            Region::Scalar(key) => {
                self.charge(
                    match key {
                        Key::String(s) => s.len() / 8 + 1,
                        _ => 1,
                    },
                    span,
                )?;
                Ok(Some(key.clone()))
            }
            _ => Ok(None),
        }
    }
    /// Replace a known entry without acknowledging its discarded payload's origins.
    pub(super) fn insert(
        &mut self,
        entries: &mut Vec<(Option<Key>, Value)>,
        key: Option<Key>,
        value: Value,
        span: Span,
    ) -> Checked<()> {
        self.charge(entries.len().saturating_add(1), span)?;
        if !entries.is_empty() && (key.is_none() || entries.iter().any(|(key, _)| key.is_none())) {
            return self.unsupported(span);
        }
        for (old, _) in entries.iter() {
            self.charge(
                key_weight(old.as_ref()).saturating_add(key_weight(key.as_ref())),
                span,
            )?;
        }
        if let Some(index) = key.as_ref().and_then(|key| {
            entries
                .iter()
                .position(|(old, _)| old.as_ref() == Some(key))
        }) {
            entries[index] = (key, value);
        } else {
            entries.push((key, value));
        }
        Ok(())
    }
    /// Updating a complete known map transfers its retained entries only.
    fn map_put(&mut self, map: &Value, key: &Value, value: &Value, span: Span) -> Checked<Value> {
        self.map_put_at(map, key, value, span, 0)
    }
    /// Update each branch-selected map without erasing its selector or merging old value coverage.
    fn map_put_at(
        &mut self,
        map: &Value,
        key: &Value,
        value: &Value,
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
            for (guard, old) in choices {
                let old = if map.complete {
                    old.clone()
                } else {
                    old.partial()
                };
                output.push((*guard, self.map_put_at(&old, key, value, span, depth + 1)?));
            }
            return self.node(Region::Choice(output), span);
        }
        let Region::Map { entries, exact, .. } = &map.node.kind else {
            return self.unsupported(span);
        };
        let key = self.key(key, span)?;
        if !exact || (key.is_none() && !entries.is_empty()) {
            return self.dynamic_map_put(map, value, span);
        }
        self.charge(entries.len(), span)?;
        for (key, _) in entries {
            self.charge(key_weight(key.as_ref()), span)?;
        }
        let mut output = entries.clone();
        self.insert(&mut output, key, value.clone(), span)?;
        let output = self.node(
            Region::Map {
                entries: output,
                exact: true,
                nonempty: Predicate::TRUE,
            },
            span,
        )?;
        Ok(if map.complete {
            output
        } else {
            output.partial()
        })
    }
    /// An unknown overwrite retains partial old views and the complete new value, never old coverage.
    fn dynamic_map_put(&mut self, map: &Value, value: &Value, span: Span) -> Checked<Value> {
        let Region::Map { entries, .. } = &map.node.kind else {
            return self.unsupported(span);
        };
        self.charge(entries.len().saturating_add(1), span)?;
        let mut output: Vec<_> = entries.iter().map(|(_, v)| (None, v.partial())).collect();
        output.push((None, value.clone()));
        self.node(
            Region::Map {
                entries: output,
                exact: false,
                nonempty: Predicate::TRUE,
            },
            span,
        )
    }
    /// Values enumerates every retained entry, but cannot recreate overwritten entries.
    fn map_values(&mut self, map: &Value, span: Span) -> Checked<Value> {
        let Region::Map { entries, exact, .. } = &map.node.kind else {
            return self.unsupported(span);
        };
        self.charge(entries.len(), span)?;
        let items = entries.iter().map(|(_, value)| value.clone()).collect();
        let nonempty = self.map_nonempty(map, span, 0)?;
        let value = self.node(
            Region::List {
                items,
                nonempty,
                exact: *exact,
            },
            span,
        )?;
        Ok(if map.complete { value } else { value.partial() })
    }
}

/// Charge string-key comparisons and copies according to their bounded byte size.
pub(super) fn key_weight(key: Option<&Key>) -> usize {
    match key {
        Some(Key::String(text)) => text.len() / 8 + 1,
        _ => 1,
    }
}
