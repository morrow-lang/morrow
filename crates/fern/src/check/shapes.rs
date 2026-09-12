//! Delayed structural evidence stays in non-executable probes until source is rechecked.
use super::*;

pub(super) struct Obligation {
    token: ir::ProbeToken,
    kind: Shape,
    span: Span,
}
enum Shape {
    Field {
        subject: Type,
        name: String,
        result: Type,
    },
    Update {
        subject: Type,
        fields: Vec<(String, Type)>,
    },
    Iterable {
        subject: Type,
        item: Type,
    },
    TupleRest {
        subject: Type,
        prefix: Vec<Type>,
        tail: Type,
    },
    Binding {
        subject: Type,
        pattern: ir::Pattern,
    },
}

impl Checker<'_> {
    /// Delay only existential roots in the whole-signature pass, never rigid universals.
    pub(super) fn unknown_shape(&self, ty: &Type, span: Span) -> Checked<bool> {
        Ok(self.inference.whole_signature
            && matches!(self.inference.resolve(ty, span)?, Type::Infer(_)))
    }

    /// Retain the actual receiver while its layout determines the field index later.
    pub(super) fn defer_field(
        &mut self,
        value: ir::Expr,
        name: &str,
        span: Span,
    ) -> Checked<TypedKind> {
        let result = self.inference.fresh();
        let token = self.obligation(
            Shape::Field {
                subject: value.ty.clone(),
                name: name.into(),
                result: result.clone(),
            },
            span,
        )?;
        Ok((
            ir::ExprKind::Probe {
                token,
                children: vec![value],
                bindings: vec![],
            },
            result,
        ))
    }

    /// Capture ordered update operands before a later use identifies the nominal record.
    pub(super) fn defer_update(
        &mut self,
        value: ir::Expr,
        fields: &[ast::RecordField],
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let subject = value.ty.clone();
        let mut children = vec![value];
        let mut typed = Vec::new();
        let mut names = HashSet::new();
        for field in fields {
            if !names.insert(&field.name) {
                return Err(Diagnostic::new(field.span, "duplicate record field"));
            }
            let value = self.expression(&field.value, depth)?;
            typed.push((field.name.clone(), value.ty.clone()));
            children.push(value);
        }
        let token = self.obligation(
            Shape::Update {
                subject: subject.clone(),
                fields: typed,
            },
            span,
        )?;
        Ok((
            ir::ExprKind::Probe {
                token,
                children,
                bindings: vec![],
            },
            subject,
        ))
    }

    /// Iteration retains an independent item type until List, Map, or Range is established.
    pub(super) fn defer_item(&mut self, subject: &Type, span: Span) -> Checked<Type> {
        let item = self.inference.fresh();
        self.obligation(
            Shape::Iterable {
                subject: subject.clone(),
                item: item.clone(),
            },
            span,
        )?;
        Ok(item)
    }

    /// Tuple suffix evidence may flow in either direction without inventing the tuple arity.
    pub(super) fn defer_tuple(
        &mut self,
        subject: &Type,
        count: usize,
        span: Span,
    ) -> Checked<(Vec<Type>, Type)> {
        let prefix: Vec<_> = (0..count).map(|_| self.inference.fresh()).collect();
        let tail = self.inference.fresh();
        self.obligation(
            Shape::TupleRest {
                subject: subject.clone(),
                prefix: prefix.clone(),
                tail: tail.clone(),
            },
            span,
        )?;
        Ok((prefix, tail))
    }

    /// Keep pattern-created IDs visible without fabricating executable projections.
    pub(super) fn defer_binding(
        &mut self,
        value: ir::Expr,
        pattern: ir::Pattern,
        first_id: usize,
        span: Span,
    ) -> Checked<ir::Expr> {
        let mut bindings = Vec::new();
        for (id, ty) in self.scopes.iter().flat_map(|s| s.values()) {
            returns::charge(&self.inference, span)?;
            if id.0 >= first_id {
                returns::charge_output(&self.inference, ty, span)?;
                bindings.push(ir::Param {
                    id: *id,
                    ty: ty.clone(),
                });
            }
        }
        let token = self.obligation(
            Shape::Binding {
                subject: value.ty.clone(),
                pattern,
            },
            span,
        )?;
        Ok(ir::Expr {
            kind: ir::ExprKind::Probe {
                token,
                children: vec![value],
                bindings,
            },
            ty: Type::Unit,
            span,
        })
    }

    /// Charge every retained obligation and keep its construction inaccessible outside the crate.
    fn obligation(&mut self, kind: Shape, span: Span) -> Checked<ir::ProbeToken> {
        returns::charge(&self.inference, span)?;
        let token = ir::ProbeToken::new(self.inference.next_shape);
        self.inference.next_shape += 1;
        self.inference.shapes.push(Obligation {
            token: token.clone(),
            kind,
            span,
        });
        Ok(token)
    }
}

/// No provisional expression can be finalized into published typed IR.
pub(super) fn reject(expr: &ir::Expr) -> Checked<()> {
    if matches!(expr.kind, ir::ExprKind::Probe { .. }) {
        Err(Diagnostic::new(
            expr.span,
            "inference probe cannot enter executable IR",
        ))
    } else {
        Ok(())
    }
}

/// Settle each component before quantification; stalled shapes require source annotations.
pub(super) fn finish(inference: &mut Inference, registry: &nominal::Registry) -> Checked<()> {
    loop {
        let revision = inference.revision;
        let mut pending = Vec::new();
        for obligation in std::mem::take(&mut inference.shapes) {
            returns::charge(inference, obligation.span)?;
            if !solve(inference, registry, &obligation)? {
                pending.push(obligation);
            }
        }
        inference.shapes = pending;
        returns::settle_calls(inference)?;
        if inference.shapes.is_empty() {
            return Ok(());
        }
        if inference.revision == revision {
            return Err(Diagnostic::new(
                inference.shapes[0].span,
                "cannot infer private signature shape or tuple arity; add a parameter or local type annotation",
            ));
        }
    }
}

/// Resolve a known structural owner, preserving unknown existential subjects for another pass.
fn solve(
    inference: &mut Inference,
    registry: &nominal::Registry,
    obligation: &Obligation,
) -> Checked<bool> {
    let span = obligation.span;
    debug_assert!(obligation.token.id() < inference.next_shape);
    if let Shape::TupleRest {
        subject,
        prefix,
        tail,
    } = &obligation.kind
    {
        return solve_tuple(inference, subject, prefix, tail, span);
    }
    let subject = match &obligation.kind {
        Shape::Field { subject, .. }
        | Shape::Update { subject, .. }
        | Shape::Iterable { subject, .. }
        | Shape::Binding { subject, .. } => subject,
        Shape::TupleRest { .. } => unreachable!("tuple obligations dispatched above"),
    };
    let subject = inference.resolve(subject, span)?;
    if matches!(subject, Type::Infer(_)) {
        return Ok(false);
    }
    match &obligation.kind {
        Shape::Field { name, result, .. } => {
            let field = field_type(registry, inference, &subject, name, span)?;
            inference.unify(result, &field, span, "record/tuple field")?;
        }
        Shape::Update { fields, .. } => {
            registry.record_shape(&subject, span)?;
            for (name, actual) in fields {
                let field = field_type(registry, inference, &subject, name, span)?;
                inference.unify(actual, &field, span, "record update field")?;
            }
        }
        Shape::Iterable { item, .. } => inference.unify(
            item,
            &iteration::item_type(&subject, span)?,
            span,
            "iteration item",
        )?,
        Shape::Binding { pattern, .. } => {
            iteration::irrefutable(pattern, &subject, span, registry)?
        }
        Shape::TupleRest { .. } => unreachable!("tuple obligations dispatched above"),
    }
    Ok(true)
}

/// A numeric projection requires a known tuple arity; names require a known nominal owner.
fn field_type(
    registry: &nominal::Registry,
    inference: &Inference,
    subject: &Type,
    name: &str,
    span: Span,
) -> Checked<Type> {
    if let Type::List(item) = subject
        && name == "enumerate"
    {
        return Ok(Type::Function(
            vec![],
            Box::new(Type::List(Box::new(Type::Tuple(vec![
                Type::Int,
                *item.clone(),
            ])))),
        ));
    }
    if let Type::Tuple(fields) = subject {
        let index = name
            .parse::<usize>()
            .map_err(|_| Diagnostic::new(span, "tuple field must be a numeric index"))?;
        return fields
            .get(index)
            .cloned()
            .ok_or_else(|| Diagnostic::new(span, "tuple field index is out of range"));
    }
    Ok(registry.project_field(subject, name, inference, span)?.1)
}

/// Relate an exact tuple to its prefix and exact suffix, using occurs-checked unification.
fn solve_tuple(
    inference: &mut Inference,
    subject: &Type,
    prefix: &[Type],
    tail: &Type,
    span: Span,
) -> Checked<bool> {
    let resolved = inference.resolve(subject, span)?;
    if matches!(resolved, Type::Infer(_)) {
        let suffix = inference.resolve(tail, span)?;
        if matches!(suffix, Type::Infer(_)) {
            return Ok(false);
        }
        let mut fields = prefix.to_vec();
        fields.extend_from_slice(sequences::tuple_fields(&suffix, span)?);
        inference.unify(
            subject,
            &sequences::tuple_type(fields),
            span,
            "tuple-rest shape",
        )?;
        return Ok(true);
    }
    let fields = sequences::tuple_fields(&resolved, span)?;
    if prefix.len() > fields.len() {
        return Err(Diagnostic::new(
            span,
            "tuple pattern prefix exceeds tuple arity",
        ));
    }
    for (actual, expected) in prefix.iter().zip(fields) {
        inference.unify(actual, expected, span, "tuple prefix")?;
    }
    inference.unify(
        tail,
        &sequences::tuple_type(fields[prefix.len()..].to_vec()),
        span,
        "tuple suffix",
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests;
