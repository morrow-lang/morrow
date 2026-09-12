//! Type-only source traversal preserves all executable identities and evaluation order.
use super::*;
/// Substitute the bounded expression tree's explicit local annotations and guards.
pub(super) fn expression(expr: &mut ast::Expr, expander: &mut Expander<'_>) -> Checked<()> {
    expander.charge(1, expr.span)?;
    expression_kind(expr, expander)
}

/// Dispatch each already-charged source node, preserving static annotations and value children.
fn expression_kind(expr: &mut ast::Expr, expander: &mut Expander<'_>) -> Checked<()> {
    match &mut expr.kind {
        ast::ExprKind::Receive { arms, timeout } => substitute_receive(arms, timeout, expander)?,
        ast::ExprKind::TypeTarget(ty) => *ty = expander.expand(ty, expr.span)?,
        ast::ExprKind::Range { .. } | ast::ExprKind::For { .. } | ast::ExprKind::With { .. } => {
            substitute_iteration(expr, expander)?
        }
        ast::ExprKind::Map(entries) => {
            for (key, value) in entries {
                expression(key, expander)?;
                expression(value, expander)?;
            }
        }
        ast::ExprKind::RecordUpdate { value, fields } => {
            substitute_update(value, fields, expander)?
        }
        ast::ExprKind::Lambda { params, body } => substitute_lambda(params, body, expander)?,
        ast::ExprKind::Apply { callee, args } => substitute_apply(callee, args, expander)?,
        ast::ExprKind::Return(value)
        | ast::ExprKind::Defer(value)
        | ast::ExprKind::Unary { value, .. }
        | ast::ExprKind::Try(value)
        | ast::ExprKind::Field { value, .. } => expression(value, expander)?,
        ast::ExprKind::PostfixIf {
            value: left,
            condition: right,
        }
        | ast::ExprKind::Binary { left, right, .. } => {
            expression(left, expander)?;
            expression(right, expander)?;
        }
        ast::ExprKind::Pipe { value, args, .. } | ast::ExprKind::GlobalPipe { value, args, .. } => {
            expression(value, expander)?;
            for arg in args {
                expression(arg, expander)?;
            }
        }
        ast::ExprKind::Interpolate(parts) | ast::ExprKind::MultilineString(parts) => {
            interpolate(parts, expander)?
        }
        ast::ExprKind::Call { args, .. } | ast::ExprKind::GlobalCall { args, .. } => {
            arguments(args, expander)?
        }
        ast::ExprKind::Tuple(args) | ast::ExprKind::List(args) => {
            for arg in args {
                expression(arg, expander)?;
            }
        }
        ast::ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expression(condition, expander)?;
            expression(then_branch, expander)?;
            if let Some(value) = else_branch {
                expression(value, expander)?;
            }
        }
        ast::ExprKind::Match { value, arms } => substitute_match(value, arms, expander)?,
        ast::ExprKind::ConditionMatch(arms) => substitute_conditions(arms, expander)?,
        ast::ExprKind::Block(stmts) => substitute_block(stmts, expander)?,
        _ => {}
    }
    Ok(())
}

/// Substitute block annotations while preserving initializer and statement traversal order.
fn substitute_block(stmts: &mut [ast::Stmt], expander: &mut Expander<'_>) -> Checked<()> {
    for stmt in stmts {
        if let ast::Stmt::LetElse { else_branch, .. } = stmt {
            expression(else_branch, expander)?;
        }
        match stmt {
            ast::Stmt::LetElse {
                annotation,
                value,
                span,
                ..
            }
            | ast::Stmt::Let {
                annotation,
                value,
                span,
                ..
            }
            | ast::Stmt::LetPattern {
                annotation,
                value,
                span,
                ..
            } => {
                *annotation = annotation
                    .as_ref()
                    .map(|t| expander.expand(t, *span))
                    .transpose()?;
                expression(value, expander)?;
            }
            ast::Stmt::Expr(value) => expression(value, expander)?,
        }
    }
    Ok(())
}

/// Substitute lambda annotations before its recursively checked body.
fn substitute_lambda(
    params: &mut [ast::LambdaParam],
    body: &mut ast::Expr,
    expander: &mut Expander<'_>,
) -> Checked<()> {
    for param in params {
        param.annotation = param
            .annotation
            .as_ref()
            .map(|ty| expander.expand(ty, param.span))
            .transpose()?;
    }
    expression(body, expander)
}

/// Substitute update initializers so generic callback annotations remain concrete.
fn substitute_update(
    base: &mut ast::Expr,
    fields: &mut [ast::RecordField],
    expander: &mut Expander<'_>,
) -> Checked<()> {
    expression(base, expander)?;
    for field in fields {
        expression(&mut field.value, expander)?;
    }
    Ok(())
}

/// Substitute match guards and bodies without conflating their lexical patterns.
fn substitute_match(
    value: &mut ast::Expr,
    arms: &mut [ast::MatchArm],
    expander: &mut Expander<'_>,
) -> Checked<()> {
    expression(value, expander)?;
    for arm in arms {
        pattern(&mut arm.pattern, expander)?;
        if let Some(guard) = &mut arm.guard {
            expression(guard, expander)?;
        }
        expression(&mut arm.body, expander)?;
    }
    Ok(())
}
/// Substitute source conditional arms before their bounded lazy lowering.
fn substitute_conditions(
    arms: &mut [ast::ConditionArm],
    expander: &mut Expander<'_>,
) -> Checked<()> {
    for arm in arms {
        if let Some(condition) = &mut arm.condition {
            expression(condition, expander)?;
        }
        expression(&mut arm.body, expander)?;
    }
    Ok(())
}

/// Substitute annotations nested inside flat loop and with control flow.
fn substitute_iteration(expr: &mut ast::Expr, expander: &mut Expander<'_>) -> Checked<()> {
    match &mut expr.kind {
        ast::ExprKind::Range { start, end, .. } => {
            expression(start, expander)?;
            expression(end, expander)?;
        }
        ast::ExprKind::For { iterable, body, .. } => {
            expression(iterable, expander)?;
            expression(body, expander)?;
        }
        ast::ExprKind::With {
            bindings,
            body,
            arms,
        } => {
            for binding in bindings {
                expression(&mut binding.value, expander)?;
            }
            expression(body, expander)?;
            for arm in arms.iter_mut().flatten() {
                if let Some(guard) = &mut arm.guard {
                    expression(guard, expander)?;
                }
                expression(&mut arm.body, expander)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Visit only interpolated expressions; literal string contents never denote types.
fn interpolate(parts: &mut [ast::StringPart], expander: &mut Expander<'_>) -> Checked<()> {
    for part in parts {
        if let ast::StringPart::Value(value) = part {
            expression(value, expander)?;
        }
    }
    Ok(())
}

/// Substitute typed pattern annotations without rewriting lexical binding identities.
fn pattern(pattern: &mut ast::Pattern, expander: &mut Expander<'_>) -> Checked<()> {
    match &mut pattern.kind {
        ast::PatternKind::Typed {
            pattern: inner,
            annotation,
        } => {
            *annotation = expander.expand(annotation, pattern.span)?;
            self::pattern(inner, expander)?;
        }
        ast::PatternKind::Tuple(fields) | ast::PatternKind::NamedConstructor { fields, .. } => {
            for field in fields {
                self::pattern(field, expander)?;
            }
        }
        ast::PatternKind::List { prefix, rest } => {
            for field in prefix {
                self::pattern(field, expander)?;
            }
            if let Some(rest) = rest {
                self::pattern(rest, expander)?;
            }
        }
        ast::PatternKind::TupleRest { prefix, rest } => {
            for field in prefix {
                self::pattern(field, expander)?;
            }
            self::pattern(rest, expander)?;
        }
        _ => {}
    }
    Ok(())
}

/// Substitute argument expression annotations without changing external labels.
fn arguments(args: &mut [ast::Argument], expander: &mut Expander<'_>) -> Checked<()> {
    for arg in args {
        expression(&mut arg.value, expander)?;
    }
    Ok(())
}

/// Substitute receive guards, bodies, and deadlines while preserving their source order.
fn substitute_receive(
    arms: &mut [ast::MatchArm],
    timeout: &mut Option<(Box<ast::Expr>, Box<ast::Expr>)>,
    expander: &mut Expander<'_>,
) -> Checked<()> {
    for arm in arms {
        if let Some(guard) = &mut arm.guard {
            expression(guard, expander)?;
        }
        expression(&mut arm.body, expander)?;
    }
    if let Some((duration, body)) = timeout {
        expression(duration, expander)?;
        expression(body, expander)?;
    }
    Ok(())
}

/// Preserve callee-before-arguments traversal while substituting all explicit annotations.
fn substitute_apply(
    callee: &mut ast::Expr,
    args: &mut [ast::Argument],
    expander: &mut Expander<'_>,
) -> Checked<()> {
    expression(callee, expander)?;
    for arg in args {
        expression(arg, expander)?;
    }
    Ok(())
}
