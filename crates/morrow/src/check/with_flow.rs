//! Flat typed with-steps dispatch distinct error types without erasing their payloads.
use super::*;
impl Checker<'_> {
    /// Check sequential successes and independent, outer-scope handlers for each error type.
    pub(super) fn with(
        &mut self,
        bindings: &[ast::WithBinding],
        body: &ast::Expr,
        arms: Option<&[ast::MatchArm]>,
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        if self.deferred {
            return Err(Diagnostic::new(span, "defer cannot use with/<-"));
        }
        if bindings.is_empty() {
            return Err(Diagnostic::new(span, "with requires a Result binding"));
        }
        if arms.is_some_and(|arms| arms.len().saturating_mul(bindings.len()) > MAX_EXPR_COUNT) {
            return Err(Diagnostic::new(
                span,
                "with handler expansion limit exceeded",
            ));
        }
        let directional = self.directional_context(expected, span)?;
        let result = expected.cloned().unwrap_or_else(|| self.inference.fresh());
        let outer = self.scopes.clone();
        self.scopes.push(HashMap::new());
        let (mut steps, exiting) = self.with_steps(bindings, depth)?;
        let body = match exiting {
            Some(body) => body,
            None => self.expression_expected(body, Some(&result), depth)?,
        };
        self.scopes = outer;
        let context = if directional { Some(&result) } else { None };
        let handlers = self.with_handlers(&mut steps, arms, context, span, depth)?;
        for handler in &handlers {
            self.inference
                .unify(&handler.body.ty, &result, span, "with branch")?;
        }
        let ty = if body.ty == Type::Never && handlers.iter().all(|h| h.body.ty == Type::Never) {
            Type::Never
        } else {
            result
        };
        Ok((
            ir::ExprKind::With {
                steps,
                body: Box::new(body),
                handlers,
            },
            ty,
        ))
    }

    /// Bind successful payloads once, retaining flat control flow for later failure dispatch.
    fn with_steps(
        &mut self,
        bindings: &[ast::WithBinding],
        depth: usize,
    ) -> Checked<(Vec<ir::WithStep>, Option<ir::Expr>)> {
        let mut steps = Vec::new();
        for binding in bindings {
            let value = self.expression(&binding.value, depth)?;
            if value.ty == Type::Never {
                return Ok((steps, Some(value)));
            }
            let ok = self.inference.fresh();
            let error = self.inference.fresh();
            self.inference.unify(
                &value.ty,
                &Type::Result(Box::new(ok.clone()), Box::new(error)),
                binding.span,
                "with binding requires Result",
            )?;
            let pattern = self.pattern(&binding.pattern, &ok, &mut HashSet::new(), 0)?;
            steps.push(ir::WithStep {
                pattern,
                value,
                error_handler: None,
            });
        }
        Ok((steps, None))
    }

    /// Group handlers by semantic error identity; no-else steps use the nearest function's E.
    fn with_handlers(
        &mut self,
        steps: &mut [ir::WithStep],
        arms: Option<&[ast::MatchArm]>,
        result: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<Vec<ir::WithHandler>> {
        let mut handlers: Vec<ir::WithHandler> = Vec::new();
        let mut used = vec![false; arms.map_or(0, |arms| arms.len())];
        let patterns = arms
            .map(|arms| arms.iter().map(error_pattern).collect::<Checked<Vec<_>>>())
            .transpose()?;
        for step in steps {
            let Type::Result(_, error) = self.inference.resolve(&step.value.ty, step.value.span)?
            else {
                unreachable!("with steps are checked Results")
            };
            if let (Some(arms), Some(patterns)) = (arms, patterns.as_deref()) {
                let mut existing = None;
                for (index, handler) in handlers.iter().enumerate() {
                    if self.inference.resolve(&handler.error.ty, span)? == *error {
                        existing = Some(index);
                        break;
                    }
                }
                let index = if let Some(index) = existing {
                    index
                } else {
                    handlers.push(
                        self.with_handler(&error, arms, patterns, &mut used, result, span, depth)?,
                    );
                    handlers.len() - 1
                };
                step.error_handler = Some(index);
            } else {
                self.propagated_error(&error, step.value.span)?;
            }
        }
        if let Some(arms) = arms {
            for (arm, used) in arms.iter().zip(used) {
                if !used {
                    return Err(Diagnostic::new(
                        arm.span,
                        "with error arm does not match any reachable error type",
                    ));
                }
            }
        }
        Ok(handlers)
    }

    /// Typecheck a compatible subset of source arms while preserving their original order.
    #[allow(clippy::too_many_arguments)]
    fn with_handler(
        &mut self,
        error: &Type,
        arms: &[ast::MatchArm],
        patterns: &[ast::Pattern],
        used: &mut [bool],
        result: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<ir::WithHandler> {
        let mut selected = Vec::new();
        for (index, (arm, pattern)) in arms.iter().zip(patterns).enumerate() {
            if compatible(
                pattern,
                &self.inference.resolve(error, span)?,
                self.registry,
            )? {
                used[index] = true;
                selected.push(ast::MatchArm {
                    pattern: pattern.clone(),
                    guard: arm.guard.clone(),
                    body: arm.body.clone(),
                    span: arm.span,
                });
            }
        }
        if selected.is_empty() {
            return Err(Diagnostic::new(
                span,
                format!("with handlers do not cover error type {error:?}"),
            ));
        }
        self.scopes.push(HashMap::new());
        let name = format!("$with_error{}", self.local_count);
        let id = self.bind(&name, error.clone());
        let subject = ast::Expr {
            kind: ast::ExprKind::Name(name),
            span,
        };
        let checked = self.matching(&subject, &selected, result, span, depth);
        self.scopes.pop();
        let (kind, ty) = checked?;
        Ok(ir::WithHandler {
            error: ir::Param {
                id,
                ty: error.clone(),
            },
            body: ir::Expr { kind, ty, span },
        })
    }

    /// Infer or validate the surrounding function's Result error for omitted else propagation.
    fn propagated_error(&mut self, error: &Type, span: Span) -> Checked<()> {
        let returning = self.inference.resolve(&self.function_return, span)?;
        let returning = if matches!(returning, Type::Infer(_)) {
            let result = Type::Result(Box::new(self.inference.fresh()), Box::new(error.clone()));
            self.inference
                .unify(&returning, &result, span, "with propagation")?;
            result
        } else {
            returning
        };
        let Type::Result(_, expected) = returning else {
            return Err(Diagnostic::new(
                span,
                "with without else requires a function returning Result",
            ));
        };
        self.inference
            .unify(error, &expected, span, "with propagated error type")
    }

    /// Finalize each handler and enforce success binding coverage and Result consumption.
    pub(super) fn finalize_with(
        &self,
        steps: &mut [ir::WithStep],
        body: &mut ir::Expr,
        handlers: &mut [ir::WithHandler],
    ) -> Checked<()> {
        for step in steps {
            self.finalize(&mut step.value)?;
            let Type::Result(ok, _) = &step.value.ty else {
                return Err(Diagnostic::new(step.value.span, "with requires Result"));
            };
            iteration::irrefutable(&step.pattern, ok, step.value.span, self.registry)?;
            control::pattern_discards(&step.pattern, ok, step.value.span, self.registry)?;
        }
        self.finalize(body)?;
        for handler in handlers {
            handler.error.ty = self
                .inference
                .concrete(&handler.error.ty, handler.body.span)?;
            self.finalize(&mut handler.body)?;
            if let ir::ExprKind::Match { arms, .. } = &handler.body.kind {
                for arm in arms {
                    control::pattern_discards(
                        &arm.pattern,
                        &handler.error.ty,
                        arm.span,
                        self.registry,
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// Normalize Err(payload) while rejecting impossible success and ambiguous named catchalls.
fn error_pattern(arm: &ast::MatchArm) -> Checked<ast::Pattern> {
    let kind = match &arm.pattern.kind {
        ast::PatternKind::Wildcard => ast::PatternKind::Wildcard,
        ast::PatternKind::Constructor {
            constructor: Constructor::Err,
            binding,
        } => binding
            .clone()
            .map_or(ast::PatternKind::Wildcard, ast::PatternKind::Bind),
        ast::PatternKind::NamedConstructor { name, fields }
            if name == "Err" && fields.len() == 1 =>
        {
            return Ok(fields[0].clone());
        }
        ast::PatternKind::Bind(_) => {
            return Err(Diagnostic::new(
                arm.span,
                "with error catchall must use Err(name) to bind the error payload",
            ));
        }
        _ => {
            return Err(Diagnostic::new(
                arm.span,
                "with error arms must use Err(pattern) or _",
            ));
        }
    };
    Ok(ast::Pattern {
        kind,
        span: arm.pattern.span,
    })
}

/// Filter only definitely incompatible patterns; real pattern checking supplies constraints.
fn compatible(pattern: &ast::Pattern, ty: &Type, registry: &nominal::Registry) -> Checked<bool> {
    use ast::PatternKind::*;
    if matches!(ty, Type::Infer(_)) {
        return Ok(true);
    }
    Ok(match &pattern.kind {
        Typed { annotation, .. } => crate::unions::subset(annotation, ty),
        Wildcard | Bind(_) => true,
        Int(_) => *ty == Type::Int,
        Bool(_) => *ty == Type::Bool,
        String(_) => *ty == Type::String,
        List { prefix, .. } => {
            if let Type::List(element) = ty {
                let mut matches = true;
                for p in prefix {
                    matches &= compatible(p, element, registry)?;
                }
                matches
            } else {
                false
            }
        }
        TupleRest { prefix, .. } => {
            if let Type::Tuple(types) = ty {
                prefix.len() <= types.len()
                    && compatible_fields(prefix, &types[..prefix.len()], registry)?
            } else {
                prefix.is_empty() && *ty == Type::Unit
            }
        }
        Tuple(fields) => {
            if let Type::Tuple(types) = ty {
                compatible_fields(fields, types, registry)?
            } else {
                fields.is_empty() && *ty == Type::Unit
            }
        }
        Constructor {
            constructor: crate::Constructor::Some | crate::Constructor::None,
            ..
        } => matches!(ty, Type::Option(_)),
        Constructor { .. } => matches!(ty, Type::Result(..)),
        NamedConstructor { name, fields } => {
            compatible_constructor(name, fields, ty, registry, pattern.span)?
        }
    })
}

/// Compare concrete constructor owners before descending into their actual payload types.
fn compatible_constructor(
    name: &str,
    fields: &[ast::Pattern],
    ty: &Type,
    registry: &nominal::Registry,
    span: Span,
) -> Checked<bool> {
    let tag = if let Some((owner, _, tag, _)) = registry.constructor(name) {
        if !matches!(ty, Type::Named(actual, _) if *actual == owner) {
            return Ok(false);
        }
        tag
    } else {
        match (name, ty) {
            ("Some", Type::Option(_)) | ("Ok", Type::Result(..)) => 0,
            ("None", Type::Option(_)) | ("Err", Type::Result(..)) => 1,
            ("Some" | "None" | "Ok" | "Err", _) => return Ok(false),
            _ => {
                return Err(Diagnostic::new(
                    span,
                    format!("unknown constructor '{name}'"),
                ));
            }
        }
    };
    compatible_fields(fields, &registry.variants(ty, span)?[tag], registry)
}

/// Retain wrong arities for normal diagnostics rather than silently filtering malformed arms.
fn compatible_fields(
    patterns: &[ast::Pattern],
    types: &[Type],
    registry: &nominal::Registry,
) -> Checked<bool> {
    if patterns.len() != types.len() {
        return Ok(true);
    }
    for (pattern, ty) in patterns.iter().zip(types) {
        if !compatible(pattern, ty, registry)? {
            return Ok(false);
        }
    }
    Ok(true)
}
