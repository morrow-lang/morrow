//! Bounded public-IR validation includes inactive conversion children and pattern-only metadata.
use super::*;

/// Validate shapes and charge every structural occurrence before any emitter clones or comparisons.
pub(super) fn preflight(program: &ir::Program) -> Lowering<()> {
    let mut work = 0usize;
    for layout in &program.types {
        for name in layout.fields.iter().chain(&layout.variant_names) {
            work = work.saturating_add(name.len() + 1);
            if work > 400_000 {
                return Err(invalid(
                    Span::default(),
                    "nominal metadata work limit exceeded",
                ));
            }
        }
    }
    walk(program, &mut |ty, selected| {
        type_work(ty, selected, &mut work)
    })
}

/// Check exact nominal references even in unreachable conversion tails before selecting an ABI.
pub(super) fn references(
    program: &ir::Program,
    layouts: &HashMap<Type, &ir::TypeLayout>,
) -> Lowering<()> {
    walk(program, &mut |ty, _| {
        if *ty != Type::Never {
            nominal::resolved(ty, layouts, Span::default(), 0)?;
        }
        Ok(())
    })
}

/// Visit source-free signatures, expressions and every pattern while borrowing all existing nodes.
fn walk(
    program: &ir::Program,
    visit: &mut impl FnMut(&Type, bool) -> Lowering<()>,
) -> Lowering<()> {
    for layout in &program.types {
        visit(&layout.ty, false)?;
        for field in layout.variants.iter().flatten() {
            visit(field, false)?;
        }
    }
    for function in &program.functions {
        visit(&function.return_type, false)?;
        if let Some(mailbox) = &function.mailbox {
            visit(mailbox, false)?;
        }
        for param in function.params.iter().chain(&function.captures) {
            visit(&param.ty, false)?;
        }
        let mut pending = vec![&function.body];
        while let Some(expr) = pending.pop() {
            visit(&expr.ty, false)?;
            if let ExprKind::Actor(
                ir::ActorExpr::Spawn { mailbox, .. }
                | ir::ActorExpr::Receive { mailbox, .. }
                | ir::ActorExpr::Call { mailbox, .. },
            ) = &expr.kind
            {
                visit(mailbox, false)?;
            }
            if let ExprKind::UnionInject { value } | ExprKind::UnionWiden { value } = &expr.kind {
                visit(&value.ty, false)?;
                conversion_shape(expr, value)?;
            }
            for pattern in expression_patterns(expr) {
                pattern_types(pattern, visit)?;
            }
            pending.extend(ir::children(expr));
        }
    }
    Ok(())
}

/// Validate union membership on inactive paths too; bottom-typed children retain strict termination.
fn conversion_shape(expr: &Expr, value: &Expr) -> Lowering<()> {
    if expr.ty == Type::Never && value.ty == Type::Never {
        return Ok(());
    }
    let valid = match &expr.kind {
        ExprKind::UnionInject { .. } => {
            matches!(&expr.ty,Type::Union(xs) if xs.contains(&value.ty))
        }
        ExprKind::UnionWiden { .. } => {
            matches!(expr.ty, Type::Union(_))
                && matches!(value.ty, Type::Union(_))
                && crate::unions::subset(&value.ty, &expr.ty)
        }
        _ => false,
    };
    if !valid {
        return Err(invalid(expr.span, "invalid union conversion membership"));
    }
    Ok(())
}

/// Bound queues before extending them; reject unresolved or noncanonical types before any clone.
fn type_work(ty: &Type, selected: bool, work: &mut usize) -> Lowering<()> {
    let mut pending = vec![(ty, 0)];
    let mut nodes = 0usize;
    while let Some((ty, depth)) = pending.pop() {
        nodes += 1;
        if depth > MAX_DEPTH || nodes > MAX_NODES {
            return Err(invalid(
                Span::default(),
                "type nesting or size limit exceeded",
            ));
        }
        if matches!(ty, Type::Union(_)) {
            charge_type(ty, work)?;
        }
        if matches!(ty, Type::ActorFunction(_, signature) if !matches!(signature.as_ref(), Type::Function(_, _)))
        {
            return Err(invalid(
                Span::default(),
                "actor callable requires a function signature",
            ));
        }
        let children = type_children(ty);
        if children.size_hint().1.unwrap_or(MAX_NODES)
            > MAX_NODES.saturating_sub(nodes + pending.len())
        {
            return Err(invalid(Span::default(), "type size limit exceeded"));
        }
        pending.extend(children.map(|child| (child, depth + 1)));
    }
    if selected {
        charge_type(ty, work)?;
    }
    if *ty != Type::Never {
        concrete(ty, Span::default(), 0)?;
    }
    Ok(())
}

/// Account for comparison work and retained identifier bytes under the program's shared cap.
fn charge_type(ty: &Type, work: &mut usize) -> Lowering<()> {
    *work = work.saturating_add(crate::unions::cost(ty, Span::default())?);
    if *work > 400_000 {
        return Err(invalid(
            Span::default(),
            "union representation work limit exceeded",
        ));
    }
    Ok(())
}

/// Borrow direct children only after rejecting widths exceeding the global bound.
fn type_children(ty: &Type) -> impl Iterator<Item = &Type> {
    let (fields, first, second) = match ty {
        Type::Union(fields) | Type::Tuple(fields) | Type::Named(_, fields) => {
            (fields.as_slice(), None, None)
        }
        Type::Function(fields, result) => (fields.as_slice(), Some(result.as_ref()), None),
        Type::Pid(item) | Type::List(item) | Type::Option(item) => {
            (&[][..], Some(item.as_ref()), None)
        }
        Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
            (&[][..], Some(a.as_ref()), Some(b.as_ref()))
        }
        _ => (&[][..], None, None),
    };
    fields.iter().chain(first).chain(second)
}

/// Locate statement and expression patterns without revisiting their value/body expressions.
fn expression_patterns(expr: &Expr) -> Vec<&Pattern> {
    match &expr.kind {
        ExprKind::Actor(ir::ActorExpr::Receive { arms, .. }) | ExprKind::Match { arms, .. } => {
            arms.iter().map(|arm| &arm.pattern).collect()
        }
        ExprKind::For { pattern, .. } => vec![pattern],
        ExprKind::With { steps, .. } => steps.iter().map(|step| &step.pattern).collect(),
        ExprKind::Block(stmts) => stmts
            .iter()
            .filter_map(|stmt| {
                if let Stmt::LetElse { pattern, .. } = stmt {
                    Some(pattern)
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

/// Validate every narrowed/binding type before copying patterns or installing lexical identities.
fn pattern_types(
    pattern: &Pattern,
    visit: &mut impl FnMut(&Type, bool) -> Lowering<()>,
) -> Lowering<()> {
    let mut pending = vec![(pattern, 0)];
    let mut count = 0;
    while let Some((pattern, depth)) = pending.pop() {
        count += 1;
        if depth > MAX_DEPTH || count > MAX_NODES {
            return Err(invalid(
                Span::default(),
                "pattern nesting or size limit exceeded",
            ));
        }
        if let Pattern::UnionSelect { narrowed, binding } = pattern {
            if *narrowed == Type::Never {
                return Err(invalid(
                    Span::default(),
                    "union pattern requires a concrete selected type",
                ));
            }
            visit(narrowed, true)?;
            if let Some(binding) = binding {
                visit(&binding.ty, true)?;
                if binding.ty != *narrowed || binding.ty == Type::Never {
                    return Err(invalid(
                        Span::default(),
                        "union binding type differs from selected type",
                    ));
                }
            }
        }
        let children = pattern_children(pattern);
        if children.size_hint().1.unwrap_or(MAX_NODES)
            > MAX_NODES.saturating_sub(count + pending.len())
        {
            return Err(invalid(Span::default(), "pattern size limit exceeded"));
        }
        pending.extend(children.map(|child| (child, depth + 1)));
    }
    Ok(())
}

/// Borrow nested patterns without allocating an unbounded intermediate child list.
fn pattern_children(pattern: &Pattern) -> impl Iterator<Item = &Pattern> {
    let (fields, rest) = match pattern {
        Pattern::Newtype(inner) => (&[][..], Some(inner.as_ref())),
        Pattern::List { prefix, rest } => (prefix.as_slice(), rest.as_deref()),
        Pattern::TupleRest { prefix, rest } => (prefix.as_slice(), Some(rest.as_ref())),
        Pattern::Tuple(fields) | Pattern::Variant { fields, .. } => (fields.as_slice(), None),
        _ => (&[][..], None),
    };
    fields.iter().chain(rest)
}
