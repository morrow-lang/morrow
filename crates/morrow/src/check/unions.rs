//! Directional assignment preserves exact equality and explicit value representation.
use super::*;

impl Checker<'_> {
    /// Inference-derived expectations supply shape context without introducing a union join.
    pub(super) fn expression_equal(
        &mut self,
        expr: &ast::Expr,
        expected: &Type,
        depth: usize,
    ) -> Checked<ir::Expr> {
        self.expression_budget(expr.span, depth)?;
        let (kind, ty) = self.expression_kind(expr, Some(expected), depth)?;
        let (kind, ty) = control::strict_divergence(kind, ty);
        returns::charge_output(&self.inference, &ty, expr.span)?;
        self.inference
            .unify(&ty, expected, expr.span, "inferred expression type")?;
        let value = ir::Expr {
            kind,
            ty,
            span: expr.span,
        };
        self.observe_source(expr, &value.kind, &value.ty);
        Ok(value)
    }
    /// Erase envelopes that disappear after inference and retain only witnessed conversions.
    pub(super) fn finalize_union_conversion(&self, expr: &mut ir::Expr) -> Checked<()> {
        let value = match &expr.kind {
            ir::ExprKind::UnionInject { value } | ir::ExprKind::UnionWiden { value } => value,
            _ => return Ok(()),
        };
        if !crate::unions::subset(&value.ty, &expr.ty) {
            return Err(Diagnostic::new(
                expr.span,
                "invalid finalized union conversion",
            ));
        }
        let same = value.ty == expr.ty;
        let wide = matches!(value.ty, Type::Union(_));
        let kind = std::mem::replace(&mut expr.kind, ir::ExprKind::Unit);
        let (ir::ExprKind::UnionInject { value } | ir::ExprKind::UnionWiden { value }) = kind
        else {
            unreachable!("conversion inspected before extraction");
        };
        if same {
            expr.kind = value.kind;
        } else if wide {
            expr.kind = ir::ExprKind::UnionWiden { value };
        } else {
            expr.kind = ir::ExprKind::UnionInject { value };
        }
        Ok(())
    }

    /// Capture contextual provenance before sibling expressions add inference constraints.
    pub(super) fn directional_context(&self, expected: Option<&Type>, span: Span) -> Checked<bool> {
        Ok(expected
            .map(|ty| self.inference.resolve(ty, span))
            .transpose()?
            .is_some_and(|ty| !matches!(ty, Type::Infer(_))))
    }

    /// Insert only witnessed member/subset conversions; ordinary constraints remain equality.
    pub(super) fn assign_expression(
        &mut self,
        mut value: ir::Expr,
        expected: Option<&Type>,
    ) -> Checked<ir::Expr> {
        let Some(expected) = expected else {
            return Ok(value);
        };
        let span = value.span;
        let actual = self.inference.resolve(&value.ty, span)?;
        let expected = self.inference.resolve(expected, span)?;
        if actual == Type::Never {
            return Ok(value);
        }
        if matches!(expected, Type::Union(_)) && !matches!(actual, Type::Infer(_)) {
            self.union_context(&actual, &expected, span)?;
            let actual = self.inference.resolve(&actual, span)?;
            let expected = self.inference.resolve(&expected, span)?;
            if !crate::unions::subset(&actual, &expected) {
                return Err(Diagnostic::new(
                    span,
                    format!(
                        "union assignment: {}",
                        diagnostics::mismatch(&expected, &actual)
                    ),
                ));
            }
            value.ty = actual.clone();
            if actual != expected {
                let kind = if matches!(actual, Type::Union(_)) {
                    ir::ExprKind::UnionWiden {
                        value: Box::new(value),
                    }
                } else {
                    ir::ExprKind::UnionInject {
                        value: Box::new(value),
                    }
                };
                return Ok(ir::Expr {
                    kind,
                    ty: expected,
                    span,
                });
            }
        } else {
            self.inference
                .unify(&actual, &expected, span, "expression type")?;
        }
        value.ty = self.inference.resolve(&expected, span)?;
        Ok(value)
    }

    /// Derive contextual payload types only from a unique compatible alternative.
    pub(super) fn union_context(
        &mut self,
        actual: &Type,
        expected: &Type,
        span: Span,
    ) -> Checked<()> {
        if matches!(actual, Type::Infer(_)) {
            return Ok(());
        }
        for actual in crate::unions::members(actual) {
            let expected = self.inference.resolve(expected, span)?;
            if crate::unions::members(&expected).contains(actual) {
                continue;
            }
            let choices: Vec<_> = crate::unions::members(&expected)
                .iter()
                .filter(|member| compatible(actual, member))
                .cloned()
                .collect();
            if choices.len() > 1 {
                return Err(Diagnostic::new(
                    span,
                    "ambiguous union membership; add a concrete annotation",
                ));
            }
            if let Some(choice) = choices.first() {
                self.inference.unify(actual, choice, span, "union member")?;
            }
        }
        Ok(())
    }

    /// Typed binders expose only a proven nonempty member subset inside their lexical arm.
    pub(super) fn typed_pattern(
        &mut self,
        inner: &ast::Pattern,
        annotation: &Type,
        subject: &Type,
        names: &mut HashSet<String>,
        span: Span,
    ) -> Checked<ir::Pattern> {
        let narrowed = self.inference.resolve(annotation, span)?;
        let subject = self.inference.resolve(subject, span)?;
        self.registry
            .validate(&narrowed, &self.inference.template_names, span)?;
        if !crate::unions::subset(&narrowed, &subject) {
            return Err(Diagnostic::new(
                span,
                "typed pattern must select members of its subject type",
            ));
        }
        let binding = match &inner.kind {
            ast::PatternKind::Bind(name) => {
                self.pattern_name(name, names, inner.span)?;
                Some(ir::Param {
                    id: self.bind_source(name, narrowed.clone(), inner.span),
                    ty: narrowed.clone(),
                })
            }
            ast::PatternKind::Wildcard => None,
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "typed union patterns require a binding or wildcard",
                ));
            }
        };
        if !matches!(subject, Type::Union(_)) {
            return Ok(binding.map_or(ir::Pattern::Wildcard, |p| ir::Pattern::Bind(p.id)));
        }
        Ok(ir::Pattern::UnionSelect { narrowed, binding })
    }
}

impl Inference {
    /// Share normalization/subset comparison work with all nominal representation queries.
    pub(super) fn charge_union(&self, ty: &Type, span: Span) -> Checked<()> {
        crate::unions::charge(&self.newtype_work, ty, span)
    }
    /// Independent non-union positions settle variables before canonical alternatives can collapse.
    pub(super) fn unify_signature(
        &mut self,
        pairs: &[(&Type, &Type)],
        span: Span,
        context: &str,
    ) -> Checked<()> {
        let mut deferred = Vec::with_capacity(pairs.len());
        for (actual, expected) in pairs {
            let a = self.resolve(actual, span)?;
            let b = self.resolve(expected, span)?;
            deferred.push(contains_union(&a) || contains_union(&b));
        }
        for phase in [false, true] {
            for ((actual, expected), delayed) in pairs.iter().zip(&deferred) {
                if *delayed == phase {
                    self.unify(actual, expected, span, context)?;
                }
            }
        }
        Ok(())
    }
    /// Match exact union sets without treating directional subset membership as equality.
    pub(super) fn unify_unions(
        &mut self,
        actual: &[Type],
        expected: &[Type],
        span: Span,
        context: &str,
    ) -> Checked<()> {
        if actual.len() != expected.len() {
            return Err(Diagnostic::new(
                span,
                "exact union types have different alternatives",
            ));
        }
        let mut remaining = expected.to_vec();
        let mut unresolved = Vec::new();
        for member in actual {
            if let Some(index) = remaining.iter().position(|other| other == member) {
                remaining.remove(index);
            } else {
                unresolved.push(member);
            }
        }
        for member in unresolved {
            let choices: Vec<_> = remaining
                .iter()
                .enumerate()
                .filter(|(_, other)| compatible(member, other))
                .map(|(i, _)| i)
                .collect();
            let exact = remaining.iter().position(|other| other == member);
            let index = exact
                .or_else(|| {
                    if choices.len() == 1 {
                        Some(choices[0])
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    Diagnostic::new(span, "ambiguous or incompatible exact union alternatives")
                })?;
            self.unify(member, &remaining.remove(index), span, context)?;
        }
        Ok(())
    }
}

/// Inspect already-bounded types without assigning candidate-specific inference variables.
fn compatible(a: &Type, b: &Type) -> bool {
    if a == b || matches!(a, Type::Infer(_)) || matches!(b, Type::Infer(_)) {
        return true;
    }
    match (a, b) {
        (Type::ChildKey(a), Type::ChildKey(b)) | (Type::RootFunction(a), Type::RootFunction(b)) | (Type::List(a), Type::List(b)) | (Type::Option(a), Type::Option(b)) => compatible(a, b),
        (Type::Result(a, b), Type::Result(c, d)) | (Type::Map(a, b), Type::Map(c, d)) => {
            compatible(a, c) && compatible(b, d)
        }
        (Type::Tuple(a), Type::Tuple(b)) => fields(a, b),
        (Type::Named(a, x), Type::Named(b, y)) => a == b && fields(x, y),
        (Type::Function(a, x), Type::Function(b, y)) => fields(a, b) && compatible(x, y),
        _ => false,
    }
}
fn fields(a: &[Type], b: &[Type]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| compatible(a, b))
}

/// Recover union specialization arguments only after prior ordinary argument evidence.
pub(super) fn capture(
    template: &Type,
    actual: &Type,
    values: &mut HashMap<String, Type>,
    depth: usize,
) -> Checked<()> {
    let template = nominal::substitute(template, values)?;
    if template == *actual {
        return Ok(());
    }
    let mut actuals = crate::unions::members(actual).to_vec();
    let mut unresolved = Vec::new();
    for member in crate::unions::members(&template) {
        if let Some(index) = actuals.iter().position(|ty| ty == member) {
            actuals.remove(index);
        } else {
            unresolved.push(member);
        }
    }
    if unresolved.len() != 1 || actuals.len() != 1 {
        return Err(Diagnostic::new(
            Span::default(),
            "ambiguous concrete union specialization",
        ));
    }
    nominal::capture(unresolved[0], &actuals[0], values, depth + 1)
}

/// Concrete non-union evidence must precede subset templates even when source arguments differ.
pub(super) fn capture_pairs(
    pairs: &[(&Type, &Type)],
    values: &mut HashMap<String, Type>,
) -> Checked<()> {
    for union in [false, true] {
        for (template, actual) in pairs {
            if contains_union(template) == union {
                nominal::capture(template, actual, values, 0)?;
            }
        }
    }
    Ok(())
}
/// Inspect already-bounded signatures without treating an ordinary generic as a union template.
fn contains_union(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Type::Union(_) => return true,
            Type::Tuple(xs) | Type::Named(_, xs) => pending.extend(xs),
            Type::Function(xs, result) => {
                pending.extend(xs);
                pending.push(result);
            }
            Type::ChildKey(x) | Type::RootFunction(x) | Type::List(x) | Type::Option(x) => pending.push(x),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.extend([a.as_ref(), b.as_ref()])
            }
            _ => {}
        }
    }
    false
}
