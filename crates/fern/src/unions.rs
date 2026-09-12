//! Canonical finite unions; exact member identity determines every runtime tag.
use crate::{Diagnostic, Span, Type};

/// Flatten already-normalized members under explicit structural and alternative bounds.
pub(crate) fn make(members: Vec<Type>, span: Span) -> Result<Type, Diagnostic> {
    let mut pending = members;
    let mut flat = Vec::new();
    let mut nodes = 0;
    while let Some(member) = pending.pop() {
        nodes += 1;
        if nodes > 4096 || flat.len() + pending.len() > 4096 {
            return Err(Diagnostic::new(span, "union type size limit exceeded"));
        }
        match member {
            Type::Union(members) => {
                queue_bound(nodes + pending.len(), members.len(), span)?;
                pending.extend(members);
            }
            Type::Never => {}
            ty => flat.push(ty),
        }
    }
    let mut size = 0;
    for member in &flat {
        size += cost(member, span)?;
        if size > 1_048_576 {
            return Err(Diagnostic::new(span, "union type size limit exceeded"));
        }
    }
    flat.sort();
    flat.dedup();
    if flat.is_empty() || flat.len() > 128 {
        return Err(Diagnostic::new(
            span,
            "union requires between 1 and 128 alternatives",
        ));
    }
    Ok(if flat.len() == 1 {
        flat.remove(0)
    } else {
        Type::Union(flat)
    })
}

/// Bound recursive comparisons before sorting, cloning or hashing structural members.
pub(crate) fn bound(ty: &Type, span: Span) -> Result<(), Diagnostic> {
    let mut pending = vec![(ty, 0)];
    let mut nodes = 0;
    while let Some((ty, depth)) = pending.pop() {
        nodes += 1;
        if depth >= 128 || nodes > 4096 {
            return Err(Diagnostic::new(
                span,
                "union type nesting or size limit exceeded",
            ));
        }
        let children = match ty {
            Type::Union(xs) | Type::Tuple(xs) | Type::Named(_, xs) => xs.len(),
            Type::Function(xs, _) => xs.len().saturating_add(1),
            Type::List(_) | Type::Option(_) => 1,
            Type::Result(_, _) | Type::Map(_, _) => 2,
            _ => 0,
        };
        queue_bound(nodes + pending.len(), children, span)?;
        match ty {
            Type::Union(xs) | Type::Tuple(xs) | Type::Named(_, xs) => {
                pending.extend(xs.iter().map(|x| (x, depth + 1)))
            }
            Type::Function(xs, result) => {
                pending.extend(xs.iter().map(|x| (x, depth + 1)));
                pending.push((result, depth + 1));
            }
            Type::List(x) | Type::Option(x) => pending.push((x, depth + 1)),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.push((a, depth + 1));
                pending.push((b, depth + 1));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Return exact semantic alternatives; singleton types require no runtime envelope.
pub(crate) fn members(ty: &Type) -> &[Type] {
    if let Type::Union(members) = ty {
        members
    } else {
        std::slice::from_ref(ty)
    }
}

/// Decide only exact-member subset inclusion; this never changes inference variables.
pub(crate) fn subset(actual: &Type, expected: &Type) -> bool {
    members(actual)
        .iter()
        .all(|a| members(expected).contains(a))
}

/// Bound comparisons by structural nodes and identifier bytes before member scans or sorting.
pub(crate) fn cost(ty: &Type, span: Span) -> Result<usize, Diagnostic> {
    bound(ty, span)?;
    let mut pending = vec![ty];
    let mut work = 0usize;
    while let Some(ty) = pending.pop() {
        work = work.saturating_add(1);
        match ty {
            Type::Named(name, xs) => {
                work = work.saturating_add(name.len());
                pending.extend(xs);
            }
            Type::Generic(name) => work = work.saturating_add(name.len()),
            Type::Union(xs) | Type::Tuple(xs) => pending.extend(xs),
            Type::Function(xs, result) => {
                pending.extend(xs);
                pending.push(result);
            }
            Type::List(x) | Type::Option(x) => pending.push(x),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.extend([a.as_ref(), b.as_ref()])
            }
            _ => {}
        }
        if work > 1_048_576 {
            return Err(Diagnostic::new(span, "union type text size limit exceeded"));
        }
    }
    Ok(work.saturating_mul(members(ty).len()))
}

/// Share comparison work across source validation, inference retries and concrete instances.
pub(crate) fn charge(
    work: &std::cell::Cell<usize>,
    ty: &Type,
    span: Span,
) -> Result<(), Diagnostic> {
    let used = work.get().saturating_add(cost(ty, span)?);
    if used > 400_000 {
        return Err(Diagnostic::new(
            span,
            "union representation work limit exceeded",
        ));
    }
    work.set(used);
    Ok(())
}

/// Check capacity before extending work queues with caller-owned structural children.
fn queue_bound(visited_and_pending: usize, children: usize, span: Span) -> Result<(), Diagnostic> {
    if children > 4096usize.saturating_sub(visited_and_pending) {
        return Err(Diagnostic::new(span, "union type size limit exceeded"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_child_queues_are_rejected_before_expansion() {
        assert!(queue_bound(1, 4096, Span::default()).is_err());
        assert!(bound(&Type::Tuple(vec![Type::Int; 4096]), Span::default()).is_err());
        assert!(make(vec![Type::Union(vec![Type::Int; 4096])], Span::default()).is_err());
        assert!(bound(&Type::Tuple(vec![Type::Int; 4095]), Span::default()).is_ok());
    }
}
