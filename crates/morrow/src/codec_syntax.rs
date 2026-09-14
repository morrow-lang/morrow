//! Canonical JSON codec static argument syntax; no runtime type witnesses.
use crate::{Diagnostic, Type, ast};
#[cfg(test)]
mod tests;

/// Only canonical stdlib identities may reinterpret one argument in the type namespace.
pub(crate) fn is_decode(name: &str) -> bool {
    matches!(name, "json.decode" | "Json.decode")
}
pub(crate) fn is_codec(name: &str) -> bool {
    is_decode(name) || matches!(name, "json.encode" | "Json.encode")
}

/// Normalize only the proven static slot, publishing no mutation when its source syntax is invalid.
pub(crate) fn prepare(expr: &mut ast::Expr, canonical: &str) -> Result<(), Diagnostic> {
    if !is_decode(canonical) {
        return Ok(());
    }
    let span = expr.span;
    let target = match &mut expr.kind {
        ast::ExprKind::Call { args, .. } | ast::ExprKind::GlobalCall { args, .. }
            if args.len() == 2 && args.iter().all(|a| a.label.is_none()) =>
        {
            &mut args[1]
        }
        ast::ExprKind::Pipe {
            args,
            position,
            label,
            ..
        }
        | ast::ExprKind::GlobalPipe {
            args,
            position,
            label,
            ..
        } if args.len() == 1 && *position == 0 && label.is_none() && args[0].label.is_none() => {
            &mut args[0]
        }
        _ => {
            return Err(Diagnostic::new(
                span,
                "json.decode expects input text and a compile-time target type",
            ));
        }
    };
    let ty = target_type(&target.value, 0, &mut 0)?;
    target.value.kind = ast::ExprKind::TypeTarget(ty);
    Ok(())
}

/// Interpret the restricted type-shaped syntax only after proving its canonical static slot.
fn target_type(expr: &ast::Expr, depth: usize, work: &mut usize) -> Result<Type, Diagnostic> {
    *work = work.saturating_add(1);
    if depth >= 128 || *work > 4096 {
        return Err(Diagnostic::new(
            expr.span,
            "JSON target type syntax limit exceeded",
        ));
    }
    match &expr.kind {
        ast::ExprKind::Name(name) => crate::parse::named_type(name.clone(), Vec::new(), expr.span),
        ast::ExprKind::Unit => Ok(Type::Unit),
        ast::ExprKind::Tuple(fields) => {
            if fields.len() > 4096usize.saturating_sub(*work) {
                return Err(Diagnostic::new(
                    expr.span,
                    "JSON target type syntax limit exceeded",
                ));
            }
            Ok(Type::Tuple(
                fields
                    .iter()
                    .map(|v| target_type(v, depth + 1, work))
                    .collect::<Result<_, _>>()?,
            ))
        }
        ast::ExprKind::Call { name, args }
            if name
                .rsplit('.')
                .next()
                .is_some_and(|n| n.starts_with(char::is_uppercase)) =>
        {
            if args.len() > 4096usize.saturating_sub(*work)
                || args.iter().any(|a| a.label.is_some())
            {
                return Err(Diagnostic::new(
                    expr.span,
                    "json.decode target must be a type",
                ));
            }
            let args = args
                .iter()
                .map(|a| target_type(a, depth + 1, work))
                .collect::<Result<_, _>>()?;
            crate::parse::named_type(name.clone(), args, expr.span)
        }
        ast::ExprKind::TypeTarget(ty) => {
            crate::unions::bound(ty, expr.span)?;
            Ok(ty.clone())
        }
        _ => Err(Diagnostic::new(
            expr.span,
            "json.decode target must be a type",
        )),
    }
}

/// Identify the unique static slot without trusting a TypeTarget in any nested value expression.
pub(crate) fn static_slot(expr: &ast::Expr) -> Option<usize> {
    match &expr.kind {
        ast::ExprKind::Call { name, args }
            if is_decode(name) && args.len() == 2 && args.iter().all(|a| a.label.is_none()) =>
        {
            Some(1)
        }
        ast::ExprKind::GlobalCall { resolved, args, .. }
            if is_decode(resolved) && args.len() == 2 && args.iter().all(|a| a.label.is_none()) =>
        {
            Some(1)
        }
        ast::ExprKind::Pipe {
            name,
            args,
            position,
            label,
            ..
        } if is_decode(name)
            && args.len() == 1
            && *position == 0
            && label.is_none()
            && args[0].label.is_none() =>
        {
            Some(0)
        }
        ast::ExprKind::GlobalPipe {
            resolved,
            args,
            position,
            label,
            ..
        } if is_decode(resolved)
            && args.len() == 1
            && *position == 0
            && label.is_none()
            && args[0].label.is_none() =>
        {
            Some(0)
        }
        _ => None,
    }
}

/// Direct in-memory checking proves the same type-only syntax without evaluating target expressions.
pub(crate) fn validate_call(
    name: &str,
    args: &[ast::Argument],
    span: crate::Span,
) -> Result<(), Diagnostic> {
    let expected = if is_decode(name) { 2 } else { 1 };
    if args.len() != expected || args.iter().any(|a| a.label.is_some()) {
        return Err(Diagnostic::new(
            span,
            if is_decode(name) {
                "json.decode expects input text and a compile-time target type"
            } else {
                "json.encode expects one positional value"
            },
        ));
    }
    if is_decode(name) {
        target_type(&args[1].value, 0, &mut 0)?;
    }
    Ok(())
}

/// Read a proven decoder target without evaluating it or changing source identity.
pub(crate) fn target(expr: &ast::Expr) -> Result<Type, Diagnostic> {
    target_type(expr, 0, &mut 0)
}
