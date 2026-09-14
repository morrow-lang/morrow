//! The first managed checkpoint has explicit suspension positions rather than accidental ABI fallbacks.
use crate::{
    Diagnostic,
    ir::{self, ActorExpr, Expr, ExprKind, Stmt},
};

/// Validate all receiving bodies before publication, including tails that never execute.
pub(super) fn validate(program: &ir::Program) -> Result<(), Diagnostic> {
    for function in &program.functions {
        if function.mailbox.is_some() {
            expression(&function.body, true, true, 0)?;
        }
    }
    Ok(())
}

/// Track both value continuations and actual actor-function exits through structured control.
fn expression(expr: &Expr, suspension: bool, tail: bool, depth: usize) -> Result<(), Diagnostic> {
    if depth >= 128 {
        return Err(Diagnostic::new(
            expr.span,
            "actor suspension position nesting limit exceeded",
        ));
    }
    match &expr.kind {
        ExprKind::Actor(ActorExpr::Call { args, .. }) => {
            if !suspension {
                return Err(Diagnostic::new(
                    expr.span,
                    "receiving calls require an actor suspension position",
                ));
            }
            for arg in args {
                expression(arg, suspension, false, depth + 1)?;
            }
        }
        ExprKind::Actor(ActorExpr::Receive { arms, timeout, .. }) => {
            receive(arms, timeout.as_ref(), expr.span, suspension, tail, depth)?
        }
        ExprKind::Block(stmts) if suspension => {
            for (index, stmt) in stmts.iter().enumerate() {
                match stmt {
                    Stmt::Let { value, .. } => expression(value, true, false, depth + 1)?,
                    Stmt::Expr(value) => {
                        expression(value, true, tail && index + 1 == stmts.len(), depth + 1)?
                    }
                    Stmt::LetElse {
                        value, else_branch, ..
                    } => {
                        expression(value, true, false, depth + 1)?;
                        expression(else_branch, true, tail, depth + 1)?;
                    }
                }
            }
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } if suspension => {
            expression(condition, true, false, depth + 1)?;
            expression(then_branch, true, tail, depth + 1)?;
            if let Some(branch) = else_branch {
                expression(branch, true, tail, depth + 1)?;
            }
        }
        ExprKind::Match { value, arms } if suspension => {
            expression(value, true, false, depth + 1)?;
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    expression(guard, false, false, depth + 1)?;
                }
                expression(&arm.body, true, tail, depth + 1)?;
            }
        }
        ExprKind::For { iterable, body, .. } if suspension => {
            expression(iterable, true, false, depth + 1)?;
            expression(body, true, false, depth + 1)?;
        }
        ExprKind::Return(value) if suspension => expression(value, true, true, depth + 1)?,
        ExprKind::Defer(value) => expression(value, false, false, depth + 1)?,
        _ => {
            for child in ir::children(expr) {
                expression(child, suspension, false, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// Receive permits suspension only in its bodies, never during pattern guard or deadline evaluation.
fn receive(
    arms: &[ir::MatchArm],
    timeout: Option<&(Box<Expr>, Box<Expr>)>,
    span: crate::Span,
    suspension: bool,
    tail: bool,
    depth: usize,
) -> Result<(), Diagnostic> {
    if !suspension {
        return Err(Diagnostic::new(
            span,
            "unsupported receive suspension position; bind the receive before this operand or loop",
        ));
    }
    for arm in arms {
        if let Some(guard) = &arm.guard {
            expression(guard, false, false, depth + 1)?;
        }
        expression(&arm.body, true, tail, depth + 1)?;
    }
    if let Some((duration, body)) = timeout {
        expression(duration, false, false, depth + 1)?;
        expression(body, true, tail, depth + 1)?;
    }
    Ok(())
}
