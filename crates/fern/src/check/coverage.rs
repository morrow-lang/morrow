//! Bounded pattern-matrix usefulness for scalar and nested nominal/sum patterns.
use super::{Checked, MAX_EXPR_DEPTH, nominal::Registry};
use crate::{Constructor, Diagnostic, Span, Type, ir};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Head {
    Int(i64),
    Bool(bool),
    String(String),
    Variant(usize),
    Selected(Vec<usize>),
    Unit,
    Nil,
    Cons,
}
type Heads = Vec<(Head, Vec<Type>)>;

#[derive(Clone)]
enum Pattern {
    Any,
    Specific(Head, Vec<Pattern>),
}

/// Reject unreachable arms and require coverage independent of guard conditions.
pub(super) fn validate(
    subject: &Type,
    arms: &[ir::MatchArm],
    registry: &Registry,
    span: Span,
) -> Checked<()> {
    retained(subject, arms, registry, span, false).map(|_| ())
}

/// Concrete substitutions may merge alternatives after the rigid source body was validated.
pub(super) fn retained(
    subject: &Type,
    arms: &[ir::MatchArm],
    registry: &Registry,
    span: Span,
    prune: bool,
) -> Checked<Vec<bool>> {
    retained_mode(subject, arms, registry, span, prune, false)
}

/// Selective mailbox matching checks usefulness while allowing unmatched messages to remain queued.
pub(super) fn selective(
    subject: &Type,
    arms: &[ir::MatchArm],
    registry: &Registry,
    span: Span,
    prune: bool,
) -> Checked<Vec<bool>> {
    retained_mode(subject, arms, registry, span, prune, true)
}

/// Share the bounded matrix walk across exhaustive matches and partial mailbox selection.
fn retained_mode(
    subject: &Type,
    arms: &[ir::MatchArm],
    registry: &Registry,
    span: Span,
    prune: bool,
    partial: bool,
) -> Checked<Vec<bool>> {
    let mut matrix = Vec::new();
    let mut kept = Vec::new();
    let mut budget = 20_000;
    for arm in arms {
        bound_expansion(&arm.pattern, arm.span)?;
        let pattern = lower(&arm.pattern, subject, registry, arm.span)?;
        if !useful(
            &matrix,
            std::slice::from_ref(&pattern),
            std::slice::from_ref(subject),
            registry,
            0,
            &mut budget,
            span,
        )? {
            if prune {
                kept.push(false);
                continue;
            }
            return Err(Diagnostic::new(
                arm.span,
                "unreachable match arm: previous unguarded patterns already cover it",
            ));
        }
        kept.push(true);
        if arm.guard.is_none() {
            matrix.push(vec![pattern]);
        }
    }
    if !partial
        && useful(
            &matrix,
            &[Pattern::Any],
            std::slice::from_ref(subject),
            registry,
            0,
            &mut budget,
            span,
        )?
    {
        return Err(Diagnostic::new(
            span,
            "match must be exhaustive; guards do not guarantee coverage",
        ));
    }
    Ok(kept)
}

/// Convert resolved patterns into constructor matrices; binders behave as wildcards.
fn lower(pattern: &ir::Pattern, ty: &Type, registry: &Registry, span: Span) -> Checked<Pattern> {
    Ok(match pattern {
        ir::Pattern::UnionSelect { narrowed, .. } => {
            let members = crate::unions::members(ty);
            let selected = crate::unions::members(narrowed);
            let tags = members
                .iter()
                .enumerate()
                .filter(|(_, t)| selected.contains(t))
                .map(|(tag, _)| tag)
                .collect();
            Pattern::Specific(Head::Selected(tags), vec![Pattern::Any])
        }
        ir::Pattern::Newtype(inner) => {
            lower_variant(0, std::slice::from_ref(inner.as_ref()), ty, registry, span)?
        }
        ir::Pattern::Wildcard | ir::Pattern::Bind(_) => Pattern::Any,
        ir::Pattern::Int(n) => Pattern::Specific(Head::Int(*n), vec![]),
        ir::Pattern::Bool(b) => Pattern::Specific(Head::Bool(*b), vec![]),
        ir::Pattern::String(s) => Pattern::Specific(Head::String(s.clone()), vec![]),
        ir::Pattern::Tuple(fields) if fields.is_empty() => Pattern::Specific(Head::Unit, vec![]),
        ir::Pattern::Tuple(fields) | ir::Pattern::Variant { tag: 0, fields } => {
            lower_variant(0, fields, ty, registry, span)?
        }
        ir::Pattern::Variant { tag, fields } => lower_variant(*tag, fields, ty, registry, span)?,
        ir::Pattern::TupleRest { prefix, .. } => {
            let fields = super::sequences::tuple_fields(ty, span)?;
            let mut patterns = prefix
                .iter()
                .zip(fields)
                .map(|(p, t)| lower(p, t, registry, span))
                .collect::<Checked<Vec<_>>>()?;
            patterns.resize(fields.len(), Pattern::Any);
            Pattern::Specific(
                if fields.is_empty() {
                    Head::Unit
                } else {
                    Head::Variant(0)
                },
                patterns,
            )
        }
        ir::Pattern::List { prefix, rest } => {
            let Type::List(element) = ty else {
                return Err(Diagnostic::new(span, "list pattern requires List type"));
            };
            let mut tail = if rest.is_some() {
                Pattern::Any
            } else {
                Pattern::Specific(Head::Nil, vec![])
            };
            for field in prefix.iter().rev() {
                tail = Pattern::Specific(
                    Head::Cons,
                    vec![lower(field, element, registry, span)?, tail],
                );
            }
            tail
        }
        ir::Pattern::Constructor { constructor, .. } => {
            let (tag, fields) = match constructor {
                Constructor::Some | Constructor::Ok => (0, vec![Pattern::Any]),
                Constructor::Err => (1, vec![Pattern::Any]),
                Constructor::None => (1, vec![]),
            };
            Pattern::Specific(Head::Variant(tag), fields)
        }
    })
}

/// Resolve constructor payload types before lowering nested sequence patterns.
fn lower_variant(
    tag: usize,
    fields: &[ir::Pattern],
    ty: &Type,
    registry: &Registry,
    span: Span,
) -> Checked<Pattern> {
    let variants = registry.variants(ty, span)?;
    let payload = variants
        .get(tag)
        .ok_or_else(|| Diagnostic::new(span, "invalid pattern tag"))?;
    let fields = fields
        .iter()
        .zip(payload)
        .map(|(p, t)| lower(p, t, registry, span))
        .collect::<Checked<Vec<_>>>()?;
    Ok(Pattern::Specific(Head::Variant(tag), fields))
}

/// Bound conceptual Cons depth before allocating recursive matrices or cloning their tails.
fn bound_expansion(pattern: &ir::Pattern, span: Span) -> Checked<()> {
    let mut pending = vec![(pattern, 0)];
    while let Some((pattern, depth)) = pending.pop() {
        if depth > MAX_EXPR_DEPTH {
            return Err(Diagnostic::new(
                span,
                "pattern coverage expansion limit exceeded",
            ));
        }
        match pattern {
            ir::Pattern::Newtype(inner) => pending.push((inner, depth + 1)),
            ir::Pattern::List { prefix, .. } => {
                if depth + prefix.len() > MAX_EXPR_DEPTH {
                    return Err(Diagnostic::new(
                        span,
                        "pattern coverage expansion limit exceeded",
                    ));
                }
                pending.extend(prefix.iter().enumerate().map(|(i, p)| (p, depth + i + 1)));
            }
            ir::Pattern::Tuple(fields)
            | ir::Pattern::Variant { fields, .. }
            | ir::Pattern::TupleRest { prefix: fields, .. } => {
                pending.extend(fields.iter().map(|p| (p, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Determine whether a candidate tuple contains a value not covered by the matrix.
fn useful(
    matrix: &[Vec<Pattern>],
    candidate: &[Pattern],
    types: &[Type],
    registry: &Registry,
    depth: usize,
    budget: &mut usize,
    span: Span,
) -> Checked<bool> {
    if *budget == 0 || depth >= MAX_EXPR_DEPTH {
        return Err(Diagnostic::new(
            span,
            "pattern coverage complexity limit exceeded",
        ));
    }
    *budget -= 1;
    if matrix.is_empty() {
        return Ok(true);
    }
    if candidate.is_empty() {
        return Ok(false);
    }
    if matrix
        .iter()
        .any(|row| row.iter().all(|p| matches!(p, Pattern::Any)))
    {
        return Ok(false);
    }
    let common = shared_wildcards(matrix, candidate, budget, span)?;
    if common > 0 {
        charge_columns(
            matrix.iter().map(|row| row.len() - common).sum(),
            budget,
            span,
        )?;
        let rows: Vec<_> = matrix.iter().map(|row| row[common..].to_vec()).collect();
        return useful(
            &rows,
            &candidate[common..],
            &types[common..],
            registry,
            depth,
            budget,
            span,
        );
    }
    match &candidate[0] {
        Pattern::Specific(Head::Selected(_), _) => {
            useful_selected(matrix, candidate, types, registry, depth, budget, span)
        }
        Pattern::Specific(head, fields) => {
            let payload = payload_types(&types[0], head, registry, span)?;
            let matrix = specialize(matrix, head, fields.len());
            let mut next = fields.clone();
            next.extend_from_slice(&candidate[1..]);
            let mut next_types = payload;
            next_types.extend_from_slice(&types[1..]);
            useful(
                &matrix,
                &next,
                &next_types,
                registry,
                depth + 1,
                budget,
                span,
            )
        }
        Pattern::Any => useful_any(matrix, candidate, types, registry, depth, budget, span),
    }
}

/// Explore a typed subset using the same finite constructor coverage rules.
fn useful_selected(
    matrix: &[Vec<Pattern>],
    candidate: &[Pattern],
    types: &[Type],
    registry: &Registry,
    depth: usize,
    budget: &mut usize,
    span: Span,
) -> Checked<bool> {
    let Pattern::Specific(Head::Selected(tags), fields) = &candidate[0] else {
        unreachable!("selected candidate dispatched by useful");
    };
    for tag in tags {
        charge_columns(candidate.len() + fields.len(), budget, span)?;
        let mut candidate = candidate.to_vec();
        candidate[0] = Pattern::Specific(Head::Variant(*tag), fields.clone());
        if useful(matrix, &candidate, types, registry, depth, budget, span)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Remove unconstrained columns together so wide tuples do not consume nesting depth.
fn shared_wildcards(
    matrix: &[Vec<Pattern>],
    candidate: &[Pattern],
    budget: &mut usize,
    span: Span,
) -> Checked<usize> {
    let mut count = 0;
    for (index, pattern) in candidate.iter().enumerate() {
        charge_columns(1, budget, span)?;
        if !matches!(pattern, Pattern::Any) {
            break;
        }
        let mut shared = true;
        for row in matrix {
            charge_columns(1, budget, span)?;
            if !matches!(row[index], Pattern::Any) {
                shared = false;
                break;
            }
        }
        if !shared {
            break;
        }
        count += 1;
    }
    Ok(count)
}

/// Bound column inspection and copied cells as well as recursive matrix calls.
fn charge_columns(work: usize, budget: &mut usize, span: Span) -> Checked<()> {
    *budget = budget
        .checked_sub(work)
        .ok_or_else(|| Diagnostic::new(span, "pattern coverage complexity limit exceeded"))?;
    Ok(())
}

/// Search a wildcard candidate through finite constructors or the default matrix.
fn useful_any(
    matrix: &[Vec<Pattern>],
    candidate: &[Pattern],
    types: &[Type],
    registry: &Registry,
    depth: usize,
    budget: &mut usize,
    span: Span,
) -> Checked<bool> {
    if let Some(heads) = finite_heads(&types[0], registry, span)? {
        for (head, payload) in heads {
            let matrix = specialize(matrix, &head, payload.len());
            let mut next = vec![Pattern::Any; payload.len()];
            next.extend_from_slice(&candidate[1..]);
            let mut next_types = payload;
            next_types.extend_from_slice(&types[1..]);
            if useful(
                &matrix,
                &next,
                &next_types,
                registry,
                depth + 1,
                budget,
                span,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    } else {
        let defaults: Vec<_> = matrix
            .iter()
            .filter(|row| matches!(row[0], Pattern::Any))
            .map(|row| row[1..].to_vec())
            .collect();
        useful(
            &defaults,
            &candidate[1..],
            &types[1..],
            registry,
            depth + 1,
            budget,
            span,
        )
    }
}

/// Restrict each matrix row to one constructor, expanding wildcard payload slots.
fn specialize(matrix: &[Vec<Pattern>], head: &Head, arity: usize) -> Vec<Vec<Pattern>> {
    matrix
        .iter()
        .filter_map(|row| {
            let mut fields = match &row[0] {
                Pattern::Any => vec![Pattern::Any; arity],
                Pattern::Specific(Head::Selected(tags), fields)
                    if matches!(head, Head::Variant(tag) if tags.contains(tag)) => fields.clone(),
                Pattern::Specific(other, fields) if other == head => fields.clone(),
                _ => return None,
            };
            fields.extend_from_slice(&row[1..]);
            Some(fields)
        })
        .collect()
}

/// Enumerate a finite type's constructors, leaving scalar integers/strings open-ended.
fn finite_heads(ty: &Type, registry: &Registry, span: Span) -> Checked<Option<Heads>> {
    match ty {
        Type::Bool => Ok(Some(vec![
            (Head::Bool(true), vec![]),
            (Head::Bool(false), vec![]),
        ])),
        Type::Unit => Ok(Some(vec![(Head::Unit, vec![])])),
        Type::List(element) => Ok(Some(vec![
            (Head::Nil, vec![]),
            (Head::Cons, vec![*element.clone(), ty.clone()]),
        ])),
        Type::Union(_) | Type::Tuple(_) | Type::Option(_) | Type::Result(..) | Type::Named(..) => {
            Ok(Some(
                registry
                    .variants(ty, span)?
                    .into_iter()
                    .enumerate()
                    .map(|(tag, fields)| (Head::Variant(tag), fields))
                    .collect(),
            ))
        }
        _ => Ok(None),
    }
}

/// Return the checked payload types of one concrete constructor descriptor.
fn payload_types(ty: &Type, head: &Head, registry: &Registry, span: Span) -> Checked<Vec<Type>> {
    if let (Type::List(element), Head::Cons) = (ty, head) {
        return Ok(vec![*element.clone(), ty.clone()]);
    }
    if let Head::Variant(tag) = head {
        registry
            .variants(ty, span)?
            .get(*tag)
            .cloned()
            .ok_or_else(|| Diagnostic::new(span, "invalid constructor tag in pattern"))
    } else {
        Ok(Vec::new())
    }
}
