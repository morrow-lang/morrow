//! Preserve ordinary closure ABI while dispatching actor-owned calls to resumable copies.
use super::*;

impl Builder<'_> {
    pub(super) fn dynamic_call(
        &mut self,
        source: &Expr,
        callee: &Expr,
        args: &[Expr],
        next: Option<&Continuation>,
    ) -> Lowering<Expr> {
        if matches!(callee.ty, Type::RootFunction(_)) { return Err(invalid(source.span, "root callable cannot run in an actor callback")); }
        let (actor_mailbox, signature) = match &callee.ty { Type::ActorFunction(mailbox, signature) => (Some(mailbox.as_ref()), signature.as_ref()), ty => (None, ty) };
        let Type::Function(params, result) = signature else {
            return Err(invalid(
                source.span,
                "actor dynamic call requires an ordinary callable",
            ));
        };
        if params.len() != args.len() {
            return Err(invalid(source.span, "actor dynamic call arity mismatch"));
        }
        expect_type(source.ty.clone(), *result.clone(), source.span)?;
        let mut prefix = Vec::new();
        let mut bind = |value: &Expr, this: &mut Self| -> Lowering<Expr> {
            atomic(value)?;
            let id = this.local(value.span)?;
            prefix.push(Stmt::Let {
                id,
                value: value.clone(),
            });
            Ok(node(ExprKind::Local(id), value.ty.clone(), value.span))
        };
        let closure = bind(callee, self)?;
        let mut arguments = Vec::with_capacity(args.len());
        for (argument, param) in args.iter().zip(params) {
            expect_type(argument.ty.clone(), param.clone(), argument.span)?;
            arguments.push(bind(argument, self)?);
        }
        // Unsupported resource-bearing/native callbacks keep their original trusted ABI.
        let fallback = node(
            ExprKind::Invoke {
                callee: Box::new(closure.clone()),
                args: arguments.clone(),
            },
            source.ty.clone(),
            source.span,
        );
        let mut dispatch = if let Some(mailbox) = actor_mailbox {
            expect_type(mailbox.clone(), self.mailbox.clone(), source.span)?;
            operation(Operation::InvalidInvoke, source.span)
        } else { self.finish(fallback, next)? };
        let candidates: Vec<_> = self
            .targets
            .values()
            .filter(|target| {
                !target.root_context && target.mailbox.as_ref() == actor_mailbox
                    && target.return_type == **result
                    && target.params.iter().map(|p| &p.ty).eq(params.iter())
            })
            .cloned()
            .collect();
        for target in candidates.into_iter().rev() {
            self.work = self.work.saturating_add(1 + target.captures.len());
            if self.work > MAX_NODES {
                return Err(invalid(
                    source.span,
                    "actor dynamic dispatch work limit exceeded",
                ));
            }
            let environment: Vec<_> = target
                .captures
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let mut capture = operation(
                        Operation::ClosureCapture {
                            value: Box::new(closure.clone()),
                            index,
                            ty: param.ty.clone(),
                        },
                        source.span,
                    );
                    capture.ty = param.ty.clone();
                    capture
                })
                .collect();
            let matched = self.returning_closure_call(
                target.id,
                &environment,
                &arguments,
                next,
                source.span,
            )?;
            let mut condition = operation(
                Operation::ClosureIdentity {
                    value: Box::new(closure.clone()),
                    function: target.id,
                },
                source.span,
            );
            condition.ty = Type::Bool;
            dispatch = node(
                ExprKind::If {
                    condition: Box::new(condition),
                    then_branch: Box::new(matched),
                    else_branch: Some(Box::new(dispatch)),
                },
                Type::Int,
                source.span,
            );
        }
        prefix.push(Stmt::Expr(dispatch));
        Ok(node(ExprKind::Block(prefix), Type::Int, source.span))
    }
}
