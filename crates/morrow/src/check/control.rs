//! Explicit divergence, persistent let-else bindings, and function-owned cleanup closures.
use super::*;

impl Checker<'_> {
    /// Constrain an early return against the nearest function, never its enclosing expression.
    pub(super) fn returning(
        &mut self,
        value: &ast::Expr,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        if self.deferred {
            return Err(Diagnostic::new(span, "defer cannot return"));
        }
        let returning = self.function_return.clone();
        let value = self
            .expression_expected(value, Some(&returning), depth)
            .map_err(|e| closures::context(e, "return value"))?;
        Ok((ir::ExprKind::Return(Box::new(value)), Type::Never))
    }

    /// Build a mandatory zero-argument cleanup closure with captured lexical snapshots.
    pub(super) fn defer(
        &mut self,
        value: &ast::Expr,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let function_type = Type::Function(vec![], Box::new(Type::Unit));
        let previous = self.deferred;
        self.deferred = true;
        let checked = self.lambda_body(&[], value, Some(&function_type), span, depth);
        self.deferred = previous;
        let (kind, ty) =
            checked.map_err(|e| closures::context(e, "defer requires Unit cleanup"))?;
        Ok((
            ir::ExprKind::Defer(Box::new(ir::Expr { kind, ty, span })),
            Type::Unit,
        ))
    }

    /// Finalize mandatory cleanup captures without treating them as optional escaped callbacks.
    pub(super) fn finalize_defer(&self, closure: &mut ir::Expr) -> Checked<()> {
        closure.ty = self.inference.concrete(&closure.ty, closure.span)?;
        let ir::ExprKind::Lambda {
            params,
            captures,
            body,
            ..
        } = &mut closure.kind
        else {
            return Err(Diagnostic::new(
                closure.span,
                "invalid deferred cleanup closure",
            ));
        };
        for capture in captures {
            self.finalize(&mut capture.value)?;
            capture.param.ty = self
                .inference
                .concrete(&capture.param.ty, capture.value.span)?;
        }
        self.finalize(body)?;
        reject_unused_results(body, params, self.registry)
    }

    /// Check conditional arms in source order and lower them to lazy nested branches.
    pub(super) fn condition_match(
        &mut self,
        arms: &[ast::ConditionArm],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        if arms.len() + depth >= MAX_EXPR_DEPTH {
            return Err(Diagnostic::new(
                span,
                "condition match lowering depth limit exceeded",
            ));
        }
        if arms.is_empty() || arms.last().is_some_and(|a| a.condition.is_some()) {
            return Err(Diagnostic::new(
                span,
                "condition match requires a final wildcard arm",
            ));
        }
        let directional = self.directional_context(expected, span)?;
        let result = expected.cloned().unwrap_or_else(|| self.inference.fresh());
        let mut checked = Vec::new();
        for (index, arm) in arms.iter().enumerate() {
            if arm.condition.is_none() && index + 1 != arms.len() {
                return Err(Diagnostic::new(arm.span, "unreachable condition match arm"));
            }
            let condition = arm
                .condition
                .as_ref()
                .map(|c| self.expression_expected(c, Some(&Type::Bool), depth))
                .transpose()?;
            let body = if directional {
                self.expression_expected(&arm.body, Some(&result), depth)?
            } else {
                self.expression_equal(&arm.body, &result, depth)?
            };
            checked.push((condition, body));
        }
        let (_, mut tail) = checked.pop().expect("nonempty conditional arms checked");
        for (condition, body) in checked.into_iter().rev() {
            let ty = if body.ty == Type::Never {
                tail.ty.clone()
            } else {
                body.ty.clone()
            };
            tail = ir::Expr {
                kind: ir::ExprKind::If {
                    condition: Box::new(condition.expect("nonfinal arms have conditions")),
                    then_branch: Box::new(body),
                    else_branch: Some(Box::new(tail)),
                },
                ty,
                span,
            };
        }
        Ok((tail.kind, tail.ty))
    }

    /// Check one block statement and report whether execution continues after it.
    pub(super) fn statement(
        &mut self,
        stmt: &ast::Stmt,
        expected: Option<&Type>,
        depth: usize,
        checked: &mut Vec<ir::Stmt>,
    ) -> Checked<Type> {
        match stmt {
            ast::Stmt::Let {
                name,
                annotation,
                value,
                span,
            } => {
                if let Some(ty) = annotation {
                    self.registry
                        .validate(ty, &self.inference.template_names, *span)?;
                }
                let value = self
                    .expression_expected(value, annotation.as_ref(), depth)
                    .map_err(|e| closures::context(e, "let annotation"))?;
                if value.ty == Type::Never {
                    checked.push(ir::Stmt::Expr(value));
                    return Ok(Type::Never);
                }
                let id = self.bind_source(name, value.ty.clone(), *span);
                checked.push(ir::Stmt::Let { id, value });
            }
            ast::Stmt::LetPattern {
                pattern,
                annotation,
                value,
                span,
            } => {
                return self.destructure(
                    pattern,
                    annotation.as_ref(),
                    value,
                    *span,
                    depth,
                    checked,
                );
            }
            ast::Stmt::LetElse {
                pattern,
                annotation,
                value,
                else_branch,
                span,
            } => {
                return self.let_else(
                    pattern,
                    annotation.as_ref(),
                    value,
                    else_branch,
                    *span,
                    depth,
                    checked,
                );
            }
            ast::Stmt::Expr(value) => {
                let value = self.expression_expected(value, expected, depth)?;
                let ty = value.ty.clone();
                checked.push(ir::Stmt::Expr(value));
                return Ok(ty);
            }
        }
        Ok(Type::Unit)
    }

    /// Keep successful pattern names in the surrounding scope; failures cannot continue.
    #[allow(clippy::too_many_arguments)]
    fn let_else(
        &mut self,
        pattern: &ast::Pattern,
        annotation: Option<&Type>,
        value: &ast::Expr,
        otherwise: &ast::Expr,
        span: Span,
        depth: usize,
        statements: &mut Vec<ir::Stmt>,
    ) -> Checked<Type> {
        if let Some(ty) = annotation {
            self.registry
                .validate(ty, &self.inference.template_names, span)?;
        }
        let value = self.expression_expected(value, annotation, depth)?;
        if value.ty == Type::Never {
            statements.push(ir::Stmt::Expr(value));
            return Ok(Type::Never);
        }
        self.scopes.push(HashMap::new());
        let else_branch = self.expression(otherwise, depth);
        self.scopes.pop();
        let else_branch = else_branch?;
        if else_branch.ty != Type::Never {
            return Err(Diagnostic::new(
                otherwise.span,
                "let-else failure branch must diverge with return",
            ));
        }
        let pattern = self.pattern(pattern, &value.ty, &mut HashSet::new(), 0)?;
        statements.push(ir::Stmt::LetElse {
            pattern,
            value,
            else_branch,
        });
        Ok(Type::Unit)
    }
}

/// Locate a block statement without expanding its initializer.
pub(super) fn statement_span(stmt: &ast::Stmt) -> Span {
    match stmt {
        ast::Stmt::Let { span, .. }
        | ast::Stmt::LetPattern { span, .. }
        | ast::Stmt::LetElse { span, .. } => *span,
        ast::Stmt::Expr(expr) => expr.span,
    }
}

/// Remove operations whose strictly evaluated child exits before they can execute.
pub(super) fn strict_divergence(kind: ir::ExprKind, ty: Type) -> TypedKind {
    use ir::ExprKind::*;
    let children: Vec<&ir::Expr> = match &kind {
        For {
            iterable: value, ..
        }
        | JsonCodecTemplate { input: value, .. }
        | JsonCodec { input: value, .. }
        | UnionInject { value }
        | UnionWiden { value }
        | Wrap(value)
        | Unwrap(value)
        | Return(value)
        | Try(value)
        | Unary { value, .. }
        | Field { value, .. } => vec![value],
        Invoke { callee, args } => std::iter::once(callee.as_ref()).chain(args).collect(),
        Binary {
            op: ast::BinaryOp::And | ast::BinaryOp::Or,
            left,
            ..
        } => vec![left],
        Range {
            start: left,
            end: right,
            ..
        }
        | Binary { left, right, .. } => vec![left, right],
        Probe { children: args, .. }
        | Call { args, .. }
        | List(args)
        | Tuple(args)
        | Interpolate(args)
        | CustomConstruct { fields: args, .. } => args.iter().collect(),
        Map(pairs) => pairs.iter().flat_map(|(k, v)| [k, v]).collect(),
        Construct {
            value: Some(value), ..
        } => vec![value],
        If { condition, .. } => vec![condition],
        Match { value, .. } => vec![value],
        _ => vec![],
    };
    if let Some(index) = children.iter().position(|child| child.ty == Type::Never) {
        let stmts = children[..=index]
            .iter()
            .map(|child| ir::Stmt::Expr((*child).clone()))
            .collect();
        (Block(stmts), Type::Never)
    } else {
        (kind, ty)
    }
}

/// Enforce Result obligations for successful pattern payloads that a wildcard would erase.
pub(super) fn pattern_discards(
    pattern: &ir::Pattern,
    ty: &Type,
    span: Span,
    registry: &nominal::Registry,
) -> Checked<()> {
    match pattern {
        ir::Pattern::UnionSelect {
            narrowed,
            binding: None,
        } if registry.contains_result(narrowed)? => {
            return Err(Diagnostic::new(
                span,
                "Result payload cannot be discarded by a typed wildcard pattern",
            ));
        }
        ir::Pattern::Newtype(inner) => {
            pattern_discards(inner, &registry.newtype_inner(ty, span)?, span, registry)?;
        }
        ir::Pattern::List { .. } | ir::Pattern::TupleRest { .. } => {
            for (p, t) in sequences::parts(pattern, ty, span)? {
                pattern_discards(p, &t, span, registry)?;
            }
        }
        ir::Pattern::Wildcard if registry.contains_result(ty)? => {
            return Err(Diagnostic::new(
                span,
                "Result payload cannot be discarded by a wildcard pattern",
            ));
        }
        ir::Pattern::Tuple(fields) if fields.is_empty() && *ty == Type::Unit => {}
        ir::Pattern::Tuple(fields) | ir::Pattern::Variant { tag: 0, fields } => {
            let variants = registry.variants(ty, span)?;
            for (pattern, ty) in fields.iter().zip(&variants[0]) {
                pattern_discards(pattern, ty, span, registry)?;
            }
        }
        ir::Pattern::Variant { tag, fields } => {
            let variants = registry.variants(ty, span)?;
            for (pattern, ty) in fields.iter().zip(&variants[*tag]) {
                pattern_discards(pattern, ty, span, registry)?;
            }
        }
        ir::Pattern::Constructor {
            constructor,
            binding: None,
        } => {
            let variants = registry.variants(ty, span)?;
            let tag = usize::from(matches!(constructor, Constructor::None | Constructor::Err));
            for ty in &variants[tag] {
                pattern_discards(&ir::Pattern::Wildcard, ty, span, registry)?;
            }
        }
        _ => {}
    }
    Ok(())
}
