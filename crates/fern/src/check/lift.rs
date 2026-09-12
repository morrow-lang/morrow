//! Closure conversion after every source specialization has stable concrete identities.
use super::*;

struct Lifter {
    base: usize,
    generated: Vec<ir::Function>,
    wrappers: Vec<(ir::CallTarget, Type, ir::FunctionId)>,
}

/// Append lifted functions only after generic source functions have finished allocating IDs.
pub(super) fn run(functions: &mut Vec<ir::Function>) -> Checked<()> {
    let mut lifter = Lifter {
        base: functions.len(),
        generated: Vec::new(),
        wrappers: Vec::new(),
    };
    for function in functions.iter_mut() {
        lifter.expression(&mut function.body)?;
    }
    functions.extend(lifter.generated);
    Ok(())
}

impl Lifter {
    fn next_id(&self, span: Span) -> Checked<ir::FunctionId> {
        let id = self.base + self.generated.len();
        if id >= MAX_FUNCTIONS {
            return Err(Diagnostic::new(
                span,
                "closure specialization limit exceeded",
            ));
        }
        Ok(ir::FunctionId(id))
    }
    /// Lift nested bodies first so each emitted closure points at complete typed code.
    fn expression(&mut self, expr: &mut ir::Expr) -> Checked<()> {
        for child in children_mut(expr) {
            self.expression(child)?;
        }
        match &expr.kind {
            ir::ExprKind::Lambda { .. } => self.lambda(expr),
            ir::ExprKind::FunctionValue { target } => {
                let function = self.wrapper(*target, &expr.ty, expr.span)?;
                expr.kind = ir::ExprKind::Closure {
                    function,
                    captures: Vec::new(),
                };
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn lambda(&mut self, expr: &mut ir::Expr) -> Checked<()> {
        let id = self.next_id(expr.span)?;
        let ir::ExprKind::Lambda {
            params,
            captures,
            body,
            local_count,
        } = std::mem::replace(&mut expr.kind, ir::ExprKind::Unit)
        else {
            unreachable!()
        };
        let Some((mailbox, _, result)) = crate::actors::function(&expr.ty) else {
            return Err(Diagnostic::new(expr.span, "invalid lambda type"));
        };
        let (capture_params, values) = captures.into_iter().map(|c| (c.param, c.value)).unzip();
        self.generated.push(ir::Function {
            mailbox: mailbox.cloned(),
            id,
            name: format!("$lambda{}", id.0),
            params,
            captures: capture_params,
            return_type: result.clone(),
            body: *body,
            local_count,
        });
        expr.kind = ir::ExprKind::Closure {
            function: id,
            captures: values,
        };
        Ok(())
    }
    /// User functions already share the closure ABI; intrinsics get concrete cached wrappers.
    fn wrapper(
        &mut self,
        target: ir::CallTarget,
        ty: &Type,
        span: Span,
    ) -> Checked<ir::FunctionId> {
        if let ir::CallTarget::Function(id) = target {
            return Ok(id);
        }
        if let Some((_, _, id)) = self
            .wrappers
            .iter()
            .find(|(t, kind, _)| *t == target && kind == ty)
        {
            return Ok(*id);
        }
        let Type::Function(params, result) = ty else {
            return Err(Diagnostic::new(span, "invalid function wrapper type"));
        };
        let id = self.next_id(span)?;
        let params = params
            .iter()
            .enumerate()
            .map(|(index, ty)| ir::Param {
                id: ir::LocalId(index),
                ty: ty.clone(),
            })
            .collect::<Vec<_>>();
        let args = params
            .iter()
            .map(|param| ir::Expr {
                kind: ir::ExprKind::Local(param.id),
                ty: param.ty.clone(),
                span,
            })
            .collect();
        self.generated.push(ir::Function {
            mailbox: None,
            id,
            name: format!("$callable{}", id.0),
            local_count: params.len(),
            params,
            captures: Vec::new(),
            return_type: (**result).clone(),
            body: ir::Expr {
                kind: ir::ExprKind::Call { target, args },
                ty: (**result).clone(),
                span,
            },
        });
        self.wrappers.push((target, ty.clone(), id));
        Ok(id)
    }
}

/// Mutable expression children in source order, shared by specialization and closure conversion.
pub(super) fn children_mut(expr: &mut ir::Expr) -> Vec<&mut ir::Expr> {
    use ir::ExprKind::*;
    match &mut expr.kind {
        Actor(actor) => crate::actors::children_mut(actor),
        Probe { children, .. } => children.iter_mut().collect(),
        Range { start, end, .. } => vec![start, end],
        For { iterable, body, .. } => vec![iterable, body],
        With {
            steps,
            body,
            handlers,
        } => steps
            .iter_mut()
            .map(|s| &mut s.value)
            .chain(std::iter::once(body.as_mut()))
            .chain(handlers.iter_mut().map(|h| &mut h.body))
            .collect(),
        Lambda { captures, body, .. } => captures
            .iter_mut()
            .map(|c| &mut c.value)
            .chain(std::iter::once(body.as_mut()))
            .collect(),
        Map(entries) => entries.iter_mut().flat_map(|(k, v)| [k, v]).collect(),
        Closure { captures, .. } => captures.iter_mut().collect(),
        Invoke { callee, args } => std::iter::once(callee.as_mut())
            .chain(args.iter_mut())
            .collect(),
        JsonCodecTemplate { input: value, .. }
        | JsonCodec { input: value, .. }
        | UnionInject { value }
        | UnionWiden { value }
        | Wrap(value)
        | Unwrap(value)
        | Return(value)
        | Defer(value)
        | Unary { value, .. }
        | Try(value)
        | Field { value, .. } => {
            vec![value]
        }
        Binary { left, right, .. } => vec![left, right],
        Call { args, .. }
        | Tuple(args)
        | List(args)
        | Interpolate(args)
        | CustomConstruct { fields: args, .. } => args.iter_mut().collect(),
        Construct {
            value: Some(value), ..
        } => vec![value],
        If {
            condition,
            then_branch,
            else_branch,
        } => {
            let mut children = vec![condition.as_mut(), then_branch.as_mut()];
            children.extend(else_branch.as_deref_mut());
            children
        }
        Match { value, arms } => {
            let mut children = vec![value.as_mut()];
            for arm in arms {
                children.extend(arm.guard.as_mut());
                children.push(&mut arm.body);
            }
            children
        }
        Block(stmts) => stmts.iter_mut().flat_map(statement_children).collect(),
        _ => Vec::new(),
    }
}

/// Preserve both initializer and failure-branch traversal for finalized lexical blocks.
fn statement_children(stmt: &mut ir::Stmt) -> Vec<&mut ir::Expr> {
    match stmt {
        ir::Stmt::LetElse {
            value, else_branch, ..
        } => vec![value, else_branch],
        ir::Stmt::Let { value, .. } | ir::Stmt::Expr(value) => vec![value],
    }
}
