//! Union envelopes preserve semantic member identity and borrow each guarded payload.
use super::*;
impl Engine<'_> {
    /// Charge bounded structural identities before cloning or comparing member tables.
    pub(super) fn union_members_cost(&mut self, members: &[Type], span: Span) -> Checked<()> {
        self.charge(members.len(), span)?;
        for member in members {
            self.charge(crate::unions::cost(member, span)?, span)?;
        }
        Ok(())
    }
    /// Wrap an origin-free tag carrier without changing any payload provenance.
    pub(super) fn union_value(
        &mut self,
        members: &[Type],
        value: Value,
        span: Span,
    ) -> Checked<Value> {
        self.union_members_cost(members, span)?;
        self.node(
            Region::Union {
                members: members.to_vec(),
                value,
            },
            span,
        )
    }
    /// Inject one already evaluated member with constant exclusive guards and no new obligation.
    pub(super) fn union_inject(
        &mut self,
        expr: &ir::Expr,
        target: &Type,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let value = self.expression(expr, depth)?;
        let Type::Union(members) = target else {
            return self.unsupported(span);
        };
        let mut guards = Vec::new();
        let mut variants = Vec::new();
        self.charge(members.len(), span)?;
        for member in members {
            self.union_members_cost(std::slice::from_ref(member), span)?;
            self.union_members_cost(std::slice::from_ref(&expr.ty), span)?;
            let selected = member == &expr.ty;
            guards.push(if selected {
                Predicate::TRUE
            } else {
                Predicate::FALSE
            });
            variants.push(if selected {
                vec![value.clone()]
            } else {
                vec![]
            });
        }
        let tag = guards
            .iter()
            .position(|p| *p == Predicate::TRUE)
            .ok_or_else(|| Diagnostic::new(span, "invalid Result obligation union injection"))?;
        let carrier = self.node(
            Region::Sum {
                origin: None,
                tag: Some(tag),
                guards,
                variants,
            },
            span,
        )?;
        self.union_value(members, carrier, span)
    }
    /// Reindex a selected subset while preserving original tag predicates and payload aliases.
    pub(super) fn union_narrow(
        &mut self,
        value: &Value,
        narrowed: &Type,
        span: Span,
        depth: usize,
    ) -> Checked<(Predicate, Value)> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        let Type::Union(members) = narrowed else {
            return self.union_member(value, narrowed, span, depth + 1);
        };
        self.charge(members.len(), span)?;
        let mut selected = Predicate::FALSE;
        let mut guards = Vec::new();
        let mut variants = Vec::new();
        for member in members {
            let (guard, payload) = self.union_member(value, member, span, depth + 1)?;
            selected = self.predicates.or(selected, guard, &mut self.work, span)?;
            guards.push(guard);
            variants.push(vec![payload]);
        }
        let carrier = self.node(
            Region::Sum {
                origin: None,
                tag: None,
                guards,
                variants,
            },
            span,
        )?;
        Ok((selected, self.union_value(members, carrier, span)?))
    }
    /// Exact member lookup cannot credit an unrelated member or an incompletely visited family.
    fn union_member(
        &mut self,
        value: &Value,
        member: &Type,
        span: Span,
        depth: usize,
    ) -> Checked<(Predicate, Value)> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        match &value.node.kind {
            Region::Union {
                members,
                value: carrier,
            } => {
                for (index, candidate) in members.iter().enumerate() {
                    self.union_members_cost(std::slice::from_ref(member), span)?;
                    self.union_members_cost(std::slice::from_ref(candidate), span)?;
                    if candidate != member {
                        continue;
                    }
                    let (guard, fields) = self.variant(carrier, index, span, depth + 1)?;
                    if guard == Predicate::FALSE {
                        return Ok((guard, self.node(Region::Empty, span)?));
                    }
                    let [payload] = fields.as_slice() else {
                        return self.unsupported(span);
                    };
                    return Ok((
                        guard,
                        if value.complete {
                            payload.clone()
                        } else {
                            payload.partial()
                        },
                    ));
                }
                Ok((Predicate::FALSE, self.node(Region::Empty, span)?))
            }
            Region::Choice(choices) => {
                self.union_choice(choices, value.complete, member, span, depth + 1)
            }
            _ => self.unsupported(span),
        }
    }
    /// Join member views under both the outer choice and the original union tag condition.
    fn union_choice(
        &mut self,
        choices: &[(Predicate, Value)],
        complete: bool,
        member: &Type,
        span: Span,
        depth: usize,
    ) -> Checked<(Predicate, Value)> {
        self.charge(choices.len(), span)?;
        let mut selected = Predicate::FALSE;
        let mut output = Vec::new();
        for (path, value) in choices {
            if *path == Predicate::FALSE {
                continue;
            }
            let (guard, payload) = self.union_member(value, member, span, depth)?;
            let guard = self.predicates.and(*path, guard, &mut self.work, span)?;
            selected = self.predicates.or(selected, guard, &mut self.work, span)?;
            if guard != Predicate::FALSE {
                output.push((guard, if complete { payload } else { payload.partial() }));
            }
        }
        Ok((selected, self.node(Region::Choice(output), span)?))
    }
    /// Expose only the tag carrier for structurally identical formal/actual union binding.
    pub(super) fn union_carrier(
        &mut self,
        value: &Value,
        expected: &[Type],
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        let carrier = match &value.node.kind {
            Region::Union { members, value } => {
                self.union_members_cost(members, span)?;
                self.union_members_cost(expected, span)?;
                if members != expected {
                    return self.unsupported(span);
                }
                value.clone()
            }
            Region::Choice(choices) => {
                self.charge(choices.len(), span)?;
                let mut output = Vec::new();
                for (guard, child) in choices {
                    if *guard != Predicate::FALSE {
                        output.push((
                            *guard,
                            self.union_carrier(child, expected, span, depth + 1)?,
                        ));
                    }
                }
                self.node(Region::Choice(output), span)?
            }
            _ => return self.unsupported(span),
        };
        Ok(if value.complete {
            carrier
        } else {
            carrier.partial()
        })
    }
}
