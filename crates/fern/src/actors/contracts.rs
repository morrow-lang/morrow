//! Explicit source boundaries prevent ordinary closure calls from losing the execution context.
use crate::{Diagnostic, Span, ir};
use std::collections::{BTreeMap, BTreeSet};

/// Compute managed direct-call effects once with a reverse graph and a shared bounded work count.
pub(crate) fn effects(program: &ir::Program) -> Result<BTreeSet<usize>, Diagnostic> {
    let mut managed = BTreeSet::new();
    let mut callers = BTreeMap::<usize, Vec<usize>>::new();
    let mut work = 0usize;
    for function in &program.functions {
        if function.mailbox.is_some() {
            managed.insert(function.id.0);
        }
        let mut pending = vec![&function.body];
        while let Some(expr) = pending.pop() {
            charge(&mut work, expr.span)?;
            if matches!(expr.kind, ir::ExprKind::Actor(_)) {
                managed.insert(function.id.0);
            }
            if let ir::ExprKind::Call {
                target: ir::CallTarget::Function(id),
                ..
            } = expr.kind
            {
                callers.entry(id.0).or_default().push(function.id.0);
            }
            pending.extend(ir::children(expr));
        }
    }
    let mut pending: Vec<_> = managed.iter().copied().collect();
    while let Some(callee) = pending.pop() {
        for caller in callers.get(&callee).into_iter().flatten() {
            charge(&mut work, Span::default())?;
            if managed.insert(*caller) {
                pending.push(*caller);
            }
        }
    }
    Ok(managed)
}

/// Refuse the deliberately unsupported first-class ordinary-helper effect before native evaluation.
pub(crate) fn validate(program: &ir::Program) -> Result<BTreeSet<usize>, Diagnostic> {
    super::positions::validate(program)?;
    let managed = effects(program)?;
    // No actor node or mailbox exists: neither direct-spawn nor managed-closure rules can fire.
    if managed.is_empty() {
        return Ok(managed);
    }
    let functions: BTreeMap<_, _> = program.functions.iter().map(|f| (f.id.0, f)).collect();
    let layouts: BTreeMap<_, _> = program.types.iter().map(|l| (&l.ty, l)).collect();
    let mut work = 0;
    for function in &program.functions {
        let mut pending = vec![(&function.body, false)];
        while let Some((expr, direct_spawn)) = pending.pop() {
            charge(&mut work, expr.span)?;
            if let ir::ExprKind::Closure { function: id, .. } = expr.kind {
                if direct_spawn && let Some(function) = functions.get(&id.0) {
                    for capture in &function.captures {
                        capture_type(&capture.ty, &layouts, &mut work, expr.span)?;
                    }
                }
                if !direct_spawn
                    && managed.contains(&id.0)
                    && functions.get(&id.0).is_some_and(|f| f.mailbox.is_none())
                {
                    return Err(Diagnostic::new(
                        expr.span,
                        "first-class actor helper requires a direct spawn entry; indirect calls and deferred actor effects are unsupported in 105A",
                    ));
                }
            }
            if let ir::ExprKind::Actor(ir::ActorExpr::Spawn { entry, .. }) = &expr.kind {
                pending.push((entry, true));
            } else {
                pending.extend(ir::children(expr).into_iter().map(|e| (e, false)));
            }
        }
    }
    Ok(managed)
}

/// All repeated graph operations share one explicit finite allowance.
fn charge(work: &mut usize, span: Span) -> Result<(), Diagnostic> {
    *work += 1;
    if *work > 400_000 {
        Err(Diagnostic::new(
            span,
            "actor effect graph work limit exceeded",
        ))
    } else {
        Ok(())
    }
}

/// Statically known entry captures must use accounted immutable storage; callable graphs are rechecked at enqueue.
fn capture_type(
    ty: &crate::Type,
    layouts: &BTreeMap<&crate::Type, &ir::TypeLayout>,
    work: &mut usize,
    span: Span,
) -> Result<(), Diagnostic> {
    use crate::Type;
    let mut pending = vec![ty];
    let mut seen = BTreeSet::new();
    while let Some(ty) = pending.pop() {
        charge(work, span)?;
        if !seen.insert(ty) {
            continue;
        }
        match ty {
            Type::Native(_) | Type::Result(_, _) => {
                return Err(Diagnostic::new(
                    span,
                    "native or Result-bearing actor capture graphs are unsupported in 105A",
                ));
            }
            Type::List(item) | Type::Option(item) => pending.push(item),
            Type::Map(key, value) => pending.extend([key.as_ref(), value.as_ref()]),
            Type::Tuple(fields) | Type::Union(fields) => pending.extend(fields),
            Type::Named(_, _) => {
                if let Some(layout) = layouts.get(ty) {
                    pending.extend(layout.variants.iter().flatten());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Replayed selectors contain no allocation, hidden calls, suspension, or faulting operators.
pub(crate) fn guard(expr: &ir::Expr) -> Result<(), Diagnostic> {
    use crate::{Type, ast::BinaryOp};
    let mut pending = vec![expr];
    let mut work = 0;
    while let Some(expr) = pending.pop() {
        charge(&mut work, expr.span)?;
        let safe = match &expr.kind {
            ir::ExprKind::Local(_)
            | ir::ExprKind::Int(_)
            | ir::ExprKind::Float(_)
            | ir::ExprKind::Bool(_)
            | ir::ExprKind::String(_)
            | ir::ExprKind::Unit
            | ir::ExprKind::Field { .. }
            | ir::ExprKind::Unary { .. } => true,
            ir::ExprKind::Binary { op, .. } => {
                !(matches!(op, BinaryOp::Divide | BinaryOp::Remainder | BinaryOp::Power)
                    || matches!(op, BinaryOp::Add) && expr.ty == Type::String)
            }
            _ => false,
        };
        if !safe {
            return Err(Diagnostic::new(
                expr.span,
                "receive guard must be pure and non-failing",
            ));
        }
        pending.extend(ir::children(expr));
    }
    Ok(())
}
