//! Original control types must be proved before CPS replaces them with scheduler status types.
use super::*;

/// Validate the result evidence that private continuation conversion will erase or discard.
pub(super) fn expression(expr: &Expr, function: &Function) -> Lowering<()> {
    match &expr.kind {
        ExprKind::Return(value) => {
            compatible(&value.ty, &function.return_type, value.span)?;
            expect_type(expr.ty.clone(), Type::Never, expr.span)?;
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            compatible(&condition.ty, &Type::Bool, condition.span)?;
            if let Some(branch) = else_branch {
                compatible(&then_branch.ty, &expr.ty, then_branch.span)?;
                compatible(&branch.ty, &expr.ty, branch.span)?;
            } else {
                // Only a terminating condition makes the entire no-else expression bottom.
                let expected = if condition.ty == Type::Never {
                    Type::Never
                } else {
                    Type::Unit
                };
                if expr.ty != expected {
                    return Err(invalid(
                        expr.span,
                        "no-else conditional has an invalid result type",
                    ));
                }
            }
        }
        ExprKind::Match { arms, .. } => arms_type(arms, &expr.ty)?,
        ExprKind::Actor(ir::ActorExpr::Receive { arms, timeout, .. }) => {
            arms_type(arms, &expr.ty)?;
            if let Some((_, body)) = timeout {
                compatible(&body.ty, &expr.ty, body.span)?;
            }
        }
        ExprKind::Block(statements) => {
            let last = match statements.last() {
                Some(Stmt::Expr(value)) => &value.ty,
                _ => &Type::Unit,
            };
            compatible(last, &expr.ty, expr.span)?;
        }
        _ => {}
    }
    Ok(())
}

/// A diverging branch supplies no result; every completed branch must retain the declared type.
fn compatible(actual: &Type, expected: &Type, span: Span) -> Lowering<()> {
    if *actual == Type::Never {
        return Ok(());
    }
    expect_type(actual.clone(), expected.clone(), span)
}

/// Each completed arm and its guard retain ordinary typed-expression requirements before lowering.
fn arms_type(arms: &[ir::MatchArm], expected: &Type) -> Lowering<()> {
    for arm in arms {
        compatible(&arm.body.ty, expected, arm.body.span)?;
        if let Some(guard) = &arm.guard {
            expect_type(guard.ty.clone(), Type::Bool, guard.span)?;
        }
    }
    Ok(())
}
