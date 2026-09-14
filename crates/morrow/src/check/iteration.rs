//! Immutable iteration snapshots, bounded range values, and lexical loop exits.
use super::*;
impl Checker<'_> {
    /// Check fixed-width range endpoints without materializing their values as a list.
    pub(super) fn range(
        &mut self,
        start: &ast::Expr,
        end: &ast::Expr,
        inclusive: bool,
        depth: usize,
    ) -> Checked<TypedKind> {
        let start = self.expression_expected(start, Some(&Type::Int), depth)?;
        let end = self.expression_expected(end, Some(&Type::Int), depth)?;
        Ok((
            ir::ExprKind::Range {
                start: Box::new(start),
                end: Box::new(end),
                inclusive,
            },
            Type::Range,
        ))
    }

    /// Resolve break or continue only within the current function's active loop.
    pub(super) fn loop_exit(&self, continuing: bool, span: Span) -> Checked<TypedKind> {
        if self.deferred {
            return Err(Diagnostic::new(span, "defer cannot break or continue"));
        }
        if self.loop_depth == 0 {
            return Err(Diagnostic::new(
                span,
                "break and continue require an enclosing loop in this function",
            ));
        }
        Ok((
            if continuing {
                ir::ExprKind::Continue
            } else {
                ir::ExprKind::Break
            },
            Type::Never,
        ))
    }

    /// Check each iteration in its own scope while the iterable is evaluated outside it.
    pub(super) fn for_loop(
        &mut self,
        pattern: &ast::Pattern,
        iterable: &ast::Expr,
        body: &ast::Expr,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let iterable = self.expression(iterable, depth)?;
        if iterable.ty == Type::Never {
            return Ok((iterable.kind, Type::Never));
        }
        let item = if self.unknown_shape(&iterable.ty, span)? {
            self.defer_item(&iterable.ty, span)?
        } else {
            returns::shape_ready(&self.inference, &iterable.ty, span)?;
            item_type(&self.inference.resolve(&iterable.ty, span)?, span)?
        };
        self.scopes.push(HashMap::new());
        let pattern = self.pattern(pattern, &item, &mut HashSet::new(), 0)?;
        self.loop_depth += 1;
        let body = self.expression(body, depth);
        self.loop_depth -= 1;
        self.scopes.pop();
        Ok((
            ir::ExprKind::For {
                pattern,
                iterable: Box::new(iterable),
                body: Box::new(body?),
            },
            Type::Unit,
        ))
    }

    /// Normalize receiver enumeration through the same typed builtin as List.enumerate.
    pub(super) fn enumerate_receiver(
        &mut self,
        receiver: &str,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        self.expression_budget(span, depth)?;
        let (kind, ty) = self.name(receiver, span)?;
        self.enumerate_value(ir::Expr { kind, ty, span }, args, expected, span)
    }

    /// Reuse one already-evaluated arbitrary receiver for method and namespace enumeration.
    pub(super) fn enumerate_value(
        &mut self,
        value: ir::Expr,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
    ) -> Checked<TypedKind> {
        if !args.is_empty() {
            return Err(Diagnostic::new(
                span,
                "enumerate expects zero explicit arguments",
            ));
        }
        let (params, result) = self.builtin_signature(ir::Builtin::ListEnumerate);
        self.constrain_result(&result, expected, span)?;
        self.inference
            .unify(&value.ty, &params[0], span, "enumerate receiver")?;
        Ok((
            ir::ExprKind::Call {
                target: ir::CallTarget::Builtin(ir::Builtin::ListEnumerate),
                args: vec![value],
            },
            result,
        ))
    }

    /// Finalize irrefutability and Result obligations after all item types are inferred.
    pub(super) fn finalize_for(
        &self,
        pattern: &ir::Pattern,
        iterable: &mut ir::Expr,
        body: &mut ir::Expr,
    ) -> Checked<()> {
        self.finalize(iterable)?;
        self.finalize(body)?;
        let item = item_type(&iterable.ty, iterable.span)?;
        irrefutable(pattern, &item, iterable.span, self.registry)?;
        control::pattern_discards(pattern, &item, iterable.span, self.registry)?;
        reject_discard(body, self.registry)
    }
}

/// Return the stable semantic item type for each supported immutable iterable.
pub(super) fn item_type(ty: &Type, span: Span) -> Checked<Type> {
    match ty {
        Type::List(item) => Ok((**item).clone()),
        Type::Map(key, value) => Ok(Type::Tuple(vec![(**key).clone(), (**value).clone()])),
        Type::Range => Ok(Type::Int),
        _ => Err(Diagnostic::new(
            span,
            "for requires a List, Map, or Range iterable",
        )),
    }
}

/// Require a binding pattern that accepts every possible item or successful payload.
pub(super) fn irrefutable(
    pattern: &ir::Pattern,
    ty: &Type,
    span: Span,
    registry: &nominal::Registry,
) -> Checked<()> {
    let arm = ir::MatchArm {
        pattern: pattern.clone(),
        guard: None,
        body: ir::Expr {
            kind: ir::ExprKind::Unit,
            ty: Type::Unit,
            span,
        },
        span,
    };
    coverage::validate(ty, &[arm], registry, span)
        .map_err(|e| closures::context(e, "iteration/with binding must be irrefutable"))
}

/// Extend ordinary Result usage checks to iteration and successful with-binding names.
pub(super) fn collect_bindings(
    expr: &ir::Expr,
    registry: &nominal::Registry,
    bindings: &mut Vec<(usize, Span)>,
) -> Checked<()> {
    match &expr.kind {
        ir::ExprKind::For {
            pattern, iterable, ..
        } => fallible_bindings(
            pattern,
            &item_type(&iterable.ty, iterable.span)?,
            expr.span,
            registry,
            bindings,
        )?,
        ir::ExprKind::With { steps, .. } => {
            for step in steps {
                if let Type::Result(ok, _) = &step.value.ty {
                    fallible_bindings(&step.pattern, ok, step.value.span, registry, bindings)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Inspect callback-bearing arguments without changing their eventual evaluation order.
pub(super) fn source_children(expr: &ast::Expr) -> Vec<&ast::Expr> {
    match &expr.kind {
        ast::ExprKind::Range { start, end, .. } => vec![start, end],
        ast::ExprKind::For { iterable, body, .. } => vec![iterable, body],
        ast::ExprKind::With {
            bindings,
            body,
            arms,
        } => {
            let mut children: Vec<_> = bindings.iter().map(|b| &b.value).collect();
            children.push(body);
            for arm in arms.iter().flatten() {
                children.extend(arm.guard.as_ref());
                children.push(&arm.body);
            }
            children
        }
        _ => vec![],
    }
}

impl Checker<'_> {
    /// Dispatch flat iteration and error-binding forms without expanding their source nesting.
    pub(super) fn iteration_kind(
        &mut self,
        expr: &ast::Expr,
        expected: Option<&Type>,
        depth: usize,
    ) -> Checked<TypedKind> {
        match &expr.kind {
            ast::ExprKind::Return(value) => self.returning(value, expr.span, depth),
            ast::ExprKind::Defer(value) => self.defer(value, expr.span, depth),
            ast::ExprKind::PostfixIf { value, condition } => {
                self.conditional(condition, value, None, expected, depth)
            }
            ast::ExprKind::ConditionMatch(arms) => {
                self.condition_match(arms, expected, expr.span, depth)
            }
            ast::ExprKind::Break => self.loop_exit(false, expr.span),
            ast::ExprKind::Continue => self.loop_exit(true, expr.span),
            ast::ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.range(start, end, *inclusive, depth),
            ast::ExprKind::For {
                pattern,
                iterable,
                body,
            } => self.for_loop(pattern, iterable, body, expr.span, depth),
            ast::ExprKind::With {
                bindings,
                body,
                arms,
            } => self.with(bindings, body, arms.as_deref(), expected, expr.span, depth),
            _ => Err(Diagnostic::new(expr.span, "invalid iteration control form")),
        }
    }
}
