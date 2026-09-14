//! Result sequencing becomes ordinary typed branches before continuation conversion.
use super::*;
use crate::Constructor;

impl Builder<'_> {
    /// Each step is evaluated once; an error bypasses every later success binding.
    pub(super) fn normalize_with(&mut self, source: &Expr, depth: usize) -> Lowering<Expr> {
        let ExprKind::With {
            steps,
            body,
            handlers,
        } = &source.kind
        else {
            return Err(invalid(source.span, "expected with expression"));
        };
        if steps.is_empty() || steps.len() > 4096 || handlers.len() > 1024 {
            return Err(invalid(
                source.span,
                "actor with step/handler limit exceeded",
            ));
        }
        if body.ty != Type::Never {
            expect_type(body.ty.clone(), source.ty.clone(), body.span)?;
        }
        // A handler can serve many steps. Bound its expanded cost before cloning it:
        // otherwise a small flat source tree could allocate a quadratic branch tree.
        let handler_costs = handlers
            .iter()
            .map(|handler| tree_cost(&handler.body))
            .collect::<Lowering<Vec<_>>>()?;
        let mut expansion = tree_cost(body)?;
        let mut used = BTreeSet::new();
        for step in steps {
            expansion = expansion
                .checked_add(tree_cost(&step.value)? + 5)
                .ok_or_else(|| invalid(source.span, "actor with expansion limit exceeded"))?;
            if step.value.ty == Type::Never {
                continue;
            }
            let Type::Result(ok, error) = &step.value.ty else {
                return Err(invalid(step.value.span, "with step requires Result"));
            };
            self.with_pattern(&step.pattern, ok, step.value.span, 0)?;
            if let Some(index) = step.error_handler {
                let handler = handlers
                    .get(index)
                    .ok_or_else(|| invalid(step.value.span, "unknown with handler"))?;
                expect_type(*error.clone(), handler.error.ty.clone(), step.value.span)?;
                used.insert(index);
                expansion = expansion
                    .checked_add(handler_costs[index])
                    .ok_or_else(|| invalid(source.span, "actor with expansion limit exceeded"))?;
            } else {
                self.propagation_type(error, step.value.span)?;
            }
        }
        if expansion > MAX_NODES.saturating_sub(self.work) {
            return Err(invalid(source.span, "actor with expansion limit exceeded"));
        }
        for (index, handler) in handlers.iter().enumerate() {
            if !used.contains(&index) {
                return Err(invalid(handler.body.span, "unused with handler"));
            }
            if handler.body.ty != Type::Never {
                expect_type(
                    handler.body.ty.clone(),
                    source.ty.clone(),
                    handler.body.span,
                )?;
            }
        }
        let mut next = *body.clone();
        for step in steps.iter().rev() {
            if step.value.ty == Type::Never {
                next = step.value.clone();
                continue;
            }
            let Type::Result(_, error) = &step.value.ty else {
                unreachable!("validated Result step")
            };
            let (error_id, failure) = if let Some(index) = step.error_handler {
                let handler = &handlers[index];
                (handler.error.id, handler.body.clone())
            } else {
                let id = self.local(step.value.span)?;
                (id, self.propagate(id, error, step.value.span)?)
            };
            next = node(
                ExprKind::Match {
                    value: Box::new(step.value.clone()),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Variant {
                                tag: 0,
                                fields: vec![step.pattern.clone()],
                            },
                            guard: None,
                            body: next,
                            span: step.value.span,
                        },
                        MatchArm {
                            pattern: Pattern::Constructor {
                                constructor: Constructor::Err,
                                binding: Some(error_id),
                            },
                            guard: None,
                            body: failure,
                            span: step.value.span,
                        },
                    ],
                },
                source.ty.clone(),
                source.span,
            );
        }
        self.normalize(&next, depth + 1)
    }

    /// Reconstruct only the Err variant with the enclosing function's success type.
    pub(super) fn normalize_try(&mut self, source: &Expr, depth: usize) -> Lowering<Expr> {
        let ExprKind::Try(value) = &source.kind else {
            return Err(invalid(source.span, "expected Result propagation"));
        };
        if value.ty == Type::Never {
            return self.normalize(value, depth + 1);
        }
        let Type::Result(ok, error) = &value.ty else {
            return Err(invalid(source.span, "try requires Result"));
        };
        expect_type(*ok.clone(), source.ty.clone(), source.span)?;
        let success = self.local(source.span)?;
        let failure = self.local(source.span)?;
        let matching = node(
            ExprKind::Match {
                value: value.clone(),
                arms: vec![
                    MatchArm {
                        pattern: Pattern::Constructor {
                            constructor: Constructor::Ok,
                            binding: Some(success),
                        },
                        guard: None,
                        body: node(ExprKind::Local(success), *ok.clone(), source.span),
                        span: source.span,
                    },
                    MatchArm {
                        pattern: Pattern::Constructor {
                            constructor: Constructor::Err,
                            binding: Some(failure),
                        },
                        guard: None,
                        body: self.propagate(failure, error, source.span)?,
                        span: source.span,
                    },
                ],
            },
            source.ty.clone(),
            source.span,
        );
        self.normalize(&matching, depth + 1)
    }
    fn propagation_type(&self, error: &Type, span: Span) -> Lowering<()> {
        let Type::Result(_, expected) = &self.source_return_type else {
            return Err(invalid(
                span,
                "Result propagation requires enclosing Result return",
            ));
        };
        expect_type(error.clone(), *expected.clone(), span)
    }
    fn propagate(&self, id: ir::LocalId, error: &Type, span: Span) -> Lowering<Expr> {
        self.propagation_type(error, span)?;
        let result = node(
            ExprKind::Construct {
                constructor: Constructor::Err,
                value: Some(Box::new(node(ExprKind::Local(id), error.clone(), span))),
            },
            self.source_return_type.clone(),
            span,
        );
        Ok(node(ExprKind::Return(Box::new(result)), Type::Never, span))
    }

    /// Retain irrefutability evidence when the original With node is erased.
    fn with_pattern(
        &mut self,
        pattern: &Pattern,
        ty: &Type,
        span: Span,
        depth: usize,
    ) -> Lowering<()> {
        self.work += 1;
        if depth >= MAX_DEPTH || self.work > MAX_NODES {
            return Err(invalid(span, "with pattern proof limit exceeded"));
        }
        let fields = match (pattern, ty) {
            (Pattern::Bind(_) | Pattern::Wildcard, _) => return Ok(()),
            (Pattern::Tuple(fields), Type::Unit) if fields.is_empty() => return Ok(()),
            (Pattern::Tuple(fields), Type::Tuple(types)) if fields.len() == types.len() => {
                fields.iter().zip(types).collect::<Vec<_>>()
            }
            (Pattern::TupleRest { prefix, rest }, Type::Tuple(types))
                if prefix.len() <= types.len() =>
            {
                for (pattern, ty) in prefix.iter().zip(types) {
                    self.with_pattern(pattern, ty, span, depth + 1)?;
                }
                return self.with_pattern(
                    rest,
                    &Type::Tuple(types[prefix.len()..].to_vec()),
                    span,
                    depth + 1,
                );
            }
            (
                Pattern::List {
                    prefix,
                    rest: Some(rest),
                },
                Type::List(_),
            ) if prefix.is_empty() => return self.with_pattern(rest, ty, span, depth + 1),
            (Pattern::Newtype(inner), Type::Named(..)) => {
                let layout = self
                    .layouts
                    .get(ty)
                    .ok_or_else(|| invalid(span, "unknown with newtype"))?;
                if layout.storage != ir::LayoutStorage::Unboxed
                    || layout.variants.len() != 1
                    || layout.variants[0].len() != 1
                {
                    return Err(invalid(span, "invalid with newtype pattern"));
                }
                let inner_type = layout.variants[0][0].clone();
                return self.with_pattern(inner, &inner_type, span, depth + 1);
            }
            (Pattern::Variant { tag: 0, fields }, Type::Named(..)) => {
                let layout = self
                    .layouts
                    .get(ty)
                    .ok_or_else(|| invalid(span, "unknown with variant"))?;
                if layout.storage != ir::LayoutStorage::Tagged
                    || layout.variants.len() != 1
                    || layout.variants[0].len() != fields.len()
                {
                    return Err(invalid(span, "with binding must be irrefutable"));
                }
                fields.iter().zip(&layout.variants[0]).collect::<Vec<_>>()
            }
            (Pattern::UnionSelect { narrowed, binding }, _)
                if narrowed == ty && binding.as_ref().is_none_or(|p| p.ty == *ty) =>
            {
                return Ok(());
            }
            _ => return Err(invalid(span, "with binding must be irrefutable")),
        };
        let fields: Vec<_> = fields
            .into_iter()
            .map(|(p, t)| (p.clone(), t.clone()))
            .collect();
        for (pattern, ty) in fields {
            self.with_pattern(&pattern, &ty, span, depth + 1)?;
        }
        Ok(())
    }
}

/// Count borrowed nodes without materializing the prospective expanded tree.
fn tree_cost(root: &Expr) -> Lowering<usize> {
    let mut pending = vec![(root, 0)];
    let mut count = 0;
    while let Some((expr, depth)) = pending.pop() {
        count += 1;
        if depth >= MAX_DEPTH || count > MAX_NODES {
            return Err(invalid(expr.span, "actor with expansion limit exceeded"));
        }
        pending.extend(
            ir::children(expr)
                .into_iter()
                .map(|child| (child, depth + 1)),
        );
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> ir::Program {
        let source = "fn result() -> Result(Int,String): Ok(7)\nfn worker():\n    receive:\n        1 -> ()\n    with\n        value <- result()\n    do\n        println(value)\n    else\n        Err(error) -> println(error)\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n";
        crate::check::check(&crate::parse::parse(source).unwrap()).unwrap()
    }
    fn alter(program: &mut ir::Program, change: impl FnOnce(&mut Expr)) {
        let worker = program
            .functions
            .iter_mut()
            .find(|f| f.name == "worker")
            .unwrap();
        let ExprKind::Block(statements) = &mut worker.body.kind else {
            panic!("worker block");
        };
        let Some(Stmt::Expr(expr)) = statements.last_mut() else {
            panic!("worker tail");
        };
        assert!(matches!(expr.kind, ExprKind::With { .. }));
        change(expr);
    }
    #[test]
    fn actor_with_erasure_rejects_forged_refutable_patterns_and_handler_metadata() {
        crate::lowering::prepare_interactive_actors(&source()).unwrap();
        for case in 0..3 {
            let mut program = source();
            alter(&mut program, |expr| {
                let ExprKind::With {
                    steps, handlers, ..
                } = &mut expr.kind
                else {
                    unreachable!();
                };
                match case {
                    0 => steps[0].pattern = Pattern::Int(7),
                    1 => steps[0].error_handler = Some(99),
                    _ => handlers[0].error.ty = Type::Bool,
                }
            });
            let error = crate::lowering::prepare_interactive_actors(&program).unwrap_err();
            assert!(
                error
                    .message
                    .contains(["irrefutable", "unknown with handler", "expected"][case]),
                "{error:?}"
            );
        }
    }
    #[test]
    fn actor_with_erasure_bounds_handler_duplication_before_cloning() {
        let mut program = source();
        alter(&mut program, |expr| {
            let span = expr.span;
            let ExprKind::With {
                steps, handlers, ..
            } = &mut expr.kind
            else {
                unreachable!();
            };
            steps[0].pattern = Pattern::Wildcard;
            *steps = vec![steps[0].clone(); 2000];
            handlers[0].body = node(
                ExprKind::Block(vec![Stmt::Expr(unit(span)); 1000]),
                Type::Unit,
                span,
            );
        });
        let error = crate::lowering::prepare_interactive_actors(&program).unwrap_err();
        assert!(
            error.message.contains("actor with expansion limit"),
            "{error:?}"
        );
    }
}
