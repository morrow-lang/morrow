//! Value-return continuations use pure frame factories, avoiding retained native stacks.
use super::*;

impl Builder<'_> {
    /// Completion stages a typed return value into a fresh caller frame.
    pub(super) fn finish(&mut self, value: Expr, next: Option<&Continuation>) -> Lowering<Expr> {
        if let Some(next) = next {
            // Branches share the continuation identity, but every defining occurrence
            // requires its own SSA source local in the generated physical function.
            let id = self.local(value.span)?;
            let mut next = next.clone();
            let ExprKind::Closure { captures, .. } = &mut next.entry.kind else {
                return Err(invalid(value.span, "invalid actor continuation recipe"));
            };
            for capture in captures {
                if matches!(capture.kind, ExprKind::Local(found) if found == next.value.id) {
                    capture.kind = ExprKind::Local(id);
                }
            }
            next.value.id = id;
            return Ok(finish(value, Some(&next)));
        }
        let span = value.span;
        let mut prefix = Vec::new();
        let value = if self.scoped {
            let id = self.local(span)?;
            let ty = value.ty.clone();
            prefix.push(Stmt::Let { id, value });
            prefix.push(Stmt::Expr(operation(Operation::ScopeLeave, span)));
            node(ExprKind::Local(id), ty, span)
        } else {
            value
        };
        let completion = if let Some(target) = &self.return_to {
            let frame = node(
                ExprKind::Invoke {
                    callee: Box::new(node(ExprKind::Local(target.id), target.ty.clone(), span)),
                    args: vec![value],
                },
                frame_type(),
                span,
            );
            operation(Operation::Continue(Box::new(frame)), span)
        } else {
            finish(value, None)
        };
        if prefix.is_empty() {
            Ok(completion)
        } else {
            prefix.push(Stmt::Expr(completion));
            Ok(node(ExprKind::Block(prefix), Type::Int, span))
        }
    }

    /// Only source function entry starts a scope; later callbacks retain that same activation.
    pub(super) fn enter_scope(&self, body: Expr) -> Expr {
        if !self.scoped {
            return body;
        }
        let span = body.span;
        node(
            ExprKind::Block(vec![
                Stmt::Expr(operation(Operation::ScopeEnter, span)),
                Stmt::Expr(body),
            ]),
            Type::Int,
            span,
        )
    }

    /// Actor copies receive a return factory in addition to their ordinary lexical arguments.
    pub(super) fn returning_function(&mut self, original: &Function, body: &Expr) -> Lowering<()> {
        let target = ir::Param {
            id: self.local(body.span)?,
            ty: Type::Function(vec![original.return_type.clone()], Box::new(frame_type())),
        };
        self.return_to = Some(target.clone());
        let body = self.expression(body, None, 0);
        self.return_to = None;
        let body = self.enter_scope(body?);
        let mut function = self.function(body, vec![], false)?;
        self.plan.steps.remove(&function.id.0);
        self.plan.generic_steps.remove(&function.id.0);
        function.id = ir::FunctionId(self.returning[&original.id.0]);
        function.captures = original
            .captures
            .iter()
            .chain(&original.params)
            .cloned()
            .chain([target])
            .collect();
        self.plan.steps.insert(function.id.0, self.mailbox.clone());
        if self.generic {
            self.plan.generic_steps.insert(function.id.0);
        }
        self.plan.functions.push(function);
        Ok(())
    }

    /// A pure factory only packs values; the next source operation runs in a later callback.
    pub(super) fn returning_call(
        &mut self,
        id: ir::FunctionId,
        args: &[Expr],
        next: Option<&Continuation>,
        span: Span,
    ) -> Lowering<Expr> {
        self.returning_closure_call(id, &[], args, next, span)
    }

    pub(super) fn returning_closure_call(
        &mut self,
        id: ir::FunctionId,
        environment: &[Expr],
        args: &[Expr],
        next: Option<&Continuation>,
        span: Span,
    ) -> Lowering<Expr> {
        let target = self.targets[&id.0].clone();
        if environment.is_empty()
            && next.is_none()
            && self.return_to.is_none()
            && !self.scoped
            && target.mailbox.is_none()
            && target.return_type == Type::Unit
        {
            return self.helper_call(id, args, span);
        }
        if environment.len() != target.captures.len() || args.len() != target.params.len() {
            return Err(invalid(span, "invalid returning helper signature"));
        }
        for (arg, param) in environment
            .iter()
            .chain(args)
            .zip(target.captures.iter().chain(&target.params))
        {
            atomic(arg)?;
            expect_type(arg.ty.clone(), param.ty.clone(), arg.span)?;
        }
        let factory = if let Some(target) = self
            .return_to
            .as_ref()
            .filter(|_| next.is_none() && !self.scoped)
        {
            node(ExprKind::Local(target.id), target.ty.clone(), span)
        } else {
            let (value, entry) = if let Some(next) = next {
                (next.value.clone(), next.entry.clone())
            } else {
                let value = ir::Param {
                    id: self.local(span)?,
                    ty: target.return_type.clone(),
                };
                let completion = self.finish(
                    node(ExprKind::Local(value.id), value.ty.clone(), span),
                    None,
                )?;
                let entry = self.closure(completion, vec![], false)?;
                (value, entry)
            };
            expect_type(value.ty.clone(), target.return_type.clone(), span)?;
            let mut factory = self.function(entry, vec![value], false)?;
            self.plan.steps.remove(&factory.id.0);
            self.plan.generic_steps.remove(&factory.id.0);
            factory.return_type = frame_type();
            let closure = node(
                ExprKind::Closure {
                    function: factory.id,
                    captures: factory
                        .captures
                        .iter()
                        .map(|p| node(ExprKind::Local(p.id), p.ty.clone(), span))
                        .collect(),
                },
                Type::Function(vec![target.return_type], Box::new(frame_type())),
                span,
            );
            self.plan.functions.push(factory);
            closure
        };
        let mut captures: Vec<_> = environment.iter().chain(args).cloned().collect();
        captures.push(factory);
        let entry = node(
            ExprKind::Closure {
                function: ir::FunctionId(self.returning[&id.0]),
                captures,
            },
            frame_type(),
            span,
        );
        Ok(operation(Operation::Continue(Box::new(entry)), span))
    }

    pub(super) fn needs(&self, expr: &Expr) -> bool {
        needs(expr)
            || tail_helpers::expression_work(expr, &self.inline_work) > tail_helpers::STEP_WORK
            || matches!(expr.kind, ExprKind::Invoke { .. })
            || matches!(expr.kind, ExprKind::Call { target: CallTarget::Builtin(b), .. } if crate::lowering::higher_order::is_higher_order(b))
            || matches!(&expr.kind, ExprKind::Call { target: CallTarget::Function(id), .. } if self.returning.contains_key(&id.0))
            || ir::children(expr).into_iter().any(|e| self.needs(e))
    }
}
fn frame_type() -> Type {
    Type::Function(vec![], Box::new(Type::Int))
}
