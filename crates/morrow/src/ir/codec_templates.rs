//! A narrow symbolic-effect boundary; executable publication still rejects every template.
use super::*;
use crate::{Diagnostic, json_codec::Direction};

/// Validate semantic relationships before an effect proof can trust a private template.
pub(super) fn validate(expr: &Expr, work: &mut usize) -> Result<(), Diagnostic> {
    charge(work, 1, expr.span)?;
    let ExprKind::JsonCodecTemplate {
        direction,
        input,
        target,
        ..
    } = &expr.kind
    else {
        return Ok(());
    };
    for ty in [target, &input.ty, &expr.ty] {
        type_work(ty, expr.span, work)?;
    }
    let Type::Result(output, error) = &expr.ty else {
        return Err(invalid(expr.span));
    };
    let expected_input = if *direction == Direction::Encode {
        target
    } else {
        &Type::String
    };
    let expected_output = if *direction == Direction::Decode {
        target
    } else {
        &Type::String
    };
    if &input.ty != expected_input
        || output.as_ref() != expected_output
        || **error != Type::Native(crate::runtime::NativeType::JsonError)
    {
        return Err(invalid(expr.span));
    }
    Ok(())
}

/// Bound complete trees before equality; unresolved existential or bottom storage is forbidden.
fn type_work(ty: &Type, span: Span, work: &mut usize) -> Result<(), Diagnostic> {
    crate::unions::bound(ty, span)?;
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        charge(work, 1, span)?;
        match ty {
            Type::Infer(_) | Type::Never => return Err(invalid(span)),
            Type::Named(name, xs) => {
                charge(work, name.len() + xs.len(), span)?;
                pending.extend(xs);
            }
            Type::Generic(name) => charge(work, name.len(), span)?,
            Type::Union(xs) | Type::Tuple(xs) => {
                charge(work, xs.len(), span)?;
                pending.extend(xs);
            }
            Type::Function(xs, result) => {
                charge(work, xs.len() + 1, span)?;
                pending.extend(xs);
                pending.push(result);
            }
            Type::List(a) | Type::Option(a) => {
                charge(work, 1, span)?;
                pending.push(a);
            }
            Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                charge(work, 2, span)?;
                pending.extend([a.as_ref(), b.as_ref()]);
            }
            _ => {}
        }
    }
    Ok(())
}

/// The allowance is shared across all templates, including unreachable bodies.
fn charge(work: &mut usize, amount: usize, span: Span) -> Result<(), Diagnostic> {
    *work = work.saturating_add(amount);
    if *work > 400_000 {
        return Err(Diagnostic::new(
            span,
            "JSON template proof work limit exceeded",
        ));
    }
    Ok(())
}
fn invalid(span: Span) -> Diagnostic {
    Diagnostic::new(span, "invalid JSON codec template types")
}
