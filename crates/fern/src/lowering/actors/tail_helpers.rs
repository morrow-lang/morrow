//! Discover bounded actor copies along recursive and iterative direct-call paths.
use super::*;

/// Recursive and iterative paths opt into resumable copies. Finite straight-line
/// helpers preserve scheduling; every original function retains its ordinary ABI.
pub(super) fn discover(
    program: &ir::Program,
    layouts: &HashMap<Type, &ir::TypeLayout>,
) -> Lowering<BTreeSet<usize>> {
    let functions: BTreeMap<_, _> = program.functions.iter().map(|f| (f.id.0, f)).collect();
    let mut roots = BTreeSet::new();
    let mut active = false;
    let mut work = 0;
    for function in &program.functions {
        if function.mailbox.is_some() {
            roots.insert(function.id.0);
        }
        let mut pending = vec![&function.body];
        while let Some(expr) = pending.pop() {
            charge(&mut work, expr.span)?;
            active |= matches!(expr.kind, ExprKind::Actor(_));
            if let ExprKind::Actor(ir::ActorExpr::Spawn { entry, .. }) = &expr.kind
                && let ExprKind::Closure { function, .. } = entry.kind
            {
                roots.insert(function.0);
            }
            pending.extend(ir::children(expr));
        }
    }
    if roots.is_empty() && !active {
        return Ok(BTreeSet::new());
    }
    let mut types = BTreeMap::new();
    let mut eligible = BTreeSet::new();
    for function in &program.functions {
        if function.mailbox.is_some() || function.name == "main" {
            continue;
        }
        let mut captures_owned = true;
        for param in function.params.iter().chain(&function.captures) {
            charge(&mut work, function.body.span)?;
            let owned = *types.entry(param.ty.clone()).or_insert_with(|| {
                validate::frame_owned(&param.ty, layouts, function.body.span).is_ok()
            });
            captures_owned &= owned;
        }
        captures_owned &=
            validate::frame_owned(&function.return_type, layouts, function.body.span).is_ok();
        if captures_owned && body_supported(&function.body, &mut work)? {
            eligible.insert(function.id.0);
            // An entry may arrive through a local alias or another ordinary
            // closure value. Descriptors can dispatch every zero-argument Unit
            // callable without rewriting any ordinary call to that identity.
            if function.params.is_empty() && function.return_type == Type::Unit {
                roots.insert(function.id.0);
            }
        }
    }
    let mut selected = BTreeSet::new();
    let mut edges = BTreeMap::<usize, BTreeSet<usize>>::new();
    let mut visited = BTreeSet::new();
    // A callable may arrive through arbitrarily many local aliases or function returns.
    // Prepare every owned cycle in an actor-bearing program; ordinary ABIs stay intact.
    let mut pending: Vec<_> = roots.into_iter().chain(eligible.iter().copied()).collect();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(function) = functions.get(&id) else {
            continue;
        };
        if function.mailbox.is_none() && !eligible.contains(&id) {
            continue;
        }
        edges.entry(id).or_default();
        let mut expressions = vec![&function.body];
        while let Some(expr) = expressions.pop() {
            charge(&mut work, expr.span)?;
            if requires_step(expr) {
                edges.entry(id).or_default().insert(id);
                if function.mailbox.is_none() {
                    selected.insert(id);
                }
            }
            expressions.extend(ir::children(expr));
        }
        for callee in direct_calls(&function.body, &mut work)? {
            if !eligible.contains(&callee) {
                continue;
            }
            if function.mailbox.is_none() {
                selected.insert(id);
            }
            selected.insert(callee);
            edges.entry(id).or_default().insert(callee);
            pending.push(callee);
        }
    }
    // Removing sinks also removes every finite path to a sink. What remains is
    // exactly the nodes that can reach a cycle, without an unbounded SCC walk.
    let mut degrees: BTreeMap<_, _> = edges.iter().map(|(&id, edges)| (id, edges.len())).collect();
    let mut callers = BTreeMap::<usize, Vec<usize>>::new();
    for (&caller, targets) in &edges {
        for &target in targets {
            charge(&mut work, Span::default())?;
            callers.entry(target).or_default().push(caller);
        }
    }
    let mut leaves: Vec<_> = degrees
        .iter()
        .filter_map(|(&id, &degree)| (degree == 0).then_some(id))
        .collect();
    while let Some(leaf) = leaves.pop() {
        for &caller in callers.get(&leaf).into_iter().flatten() {
            charge(&mut work, Span::default())?;
            let degree = degrees.get_mut(&caller).unwrap();
            *degree -= 1;
            if *degree == 0 {
                leaves.push(caller);
            }
        }
    }
    selected.retain(|id| degrees.get(id).is_some_and(|&degree| degree != 0));
    // Every actor-callable collection loop has an explicit bounded iteration step.
    // Ordinary invocations still target the original synchronous function.
    for function in &program.functions {
        if eligible.contains(&function.id.0) {
            let mut pending = vec![&function.body];
            while let Some(expr) = pending.pop() {
                charge(&mut work, expr.span)?;
                if requires_step(expr) {
                    selected.insert(function.id.0);
                    break;
                }
                pending.extend(ir::children(expr));
            }
        }
    }
    Ok(selected)
}

/// Walk every source child before selecting a copy; normalization handles strict operands.
fn body_supported(body: &Expr, work: &mut usize) -> Lowering<bool> {
    let mut pending = vec![body];
    while let Some(expr) = pending.pop() {
        charge(work, expr.span)?;
        pending.extend(ir::children(expr));
    }
    Ok(true)
}

fn direct_calls(body: &Expr, work: &mut usize) -> Lowering<BTreeSet<usize>> {
    let mut calls = BTreeSet::new();
    let mut pending = vec![body];
    while let Some(expr) = pending.pop() {
        charge(work, expr.span)?;
        if let ExprKind::Call {
            target: CallTarget::Function(id),
            ..
        }
        | ExprKind::Closure { function: id, .. } = &expr.kind
        {
            calls.insert(id.0);
        }
        pending.extend(ir::children(expr));
    }
    Ok(calls)
}

fn charge(work: &mut usize, span: Span) -> Lowering<()> {
    *work += 1;
    if *work > MAX_NODES {
        Err(invalid(span, "actor helper discovery work limit exceeded"))
    } else {
        Ok(())
    }
}

fn requires_step(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::For { .. } | ExprKind::Defer(_) | ExprKind::Invoke { .. }
    ) || matches!(expr.kind, ExprKind::Call { target: CallTarget::Builtin(b), .. } if crate::lowering::higher_order::is_higher_order(b))
}
