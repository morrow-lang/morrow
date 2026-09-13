//! Discover a bounded Unit-tail subset without changing ordinary helper calls.
use super::*;

/// Only actor-reachable tail paths leading to recursion opt into resumable
/// copies. Finite helpers keep their existing scheduling and ordinary ABI.
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
        if function.mailbox.is_some() || function.return_type != Type::Unit {
            continue;
        }
        let mut captures_owned = true;
        for param in function.params.iter().chain(&function.captures) {
            charge(&mut work, function.body.span)?;
            let owned = *types.entry(param.ty.clone()).or_insert_with(|| {
                validate::sendable(&param.ty, layouts, function.body.span).is_ok()
            });
            captures_owned &= owned;
        }
        if captures_owned && body_supported(&function.body, &mut work)? {
            eligible.insert(function.id.0);
            // An entry may arrive through a local alias or another ordinary
            // closure value. Descriptors can dispatch every zero-argument Unit
            // callable without rewriting any ordinary call to that identity.
            if function.params.is_empty() {
                roots.insert(function.id.0);
            }
        }
    }
    let mut selected = BTreeSet::new();
    let mut edges = BTreeMap::<usize, BTreeSet<usize>>::new();
    let mut visited = BTreeSet::new();
    let mut pending: Vec<_> = roots.into_iter().collect();
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
        for callee in tail_calls(&function.body, &mut work)? {
            if !eligible.contains(&callee) || !functions[&callee].captures.is_empty() {
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
    Ok(selected)
}

/// Defer needs a dynamic cleanup continuation; loops/With need their own state
/// machines. Preserve those existing bodies synchronously in this first subset.
fn body_supported(body: &Expr, work: &mut usize) -> Lowering<bool> {
    let mut pending = vec![body];
    while let Some(expr) = pending.pop() {
        charge(work, expr.span)?;
        match &expr.kind {
            ExprKind::Defer(_) | ExprKind::For { .. } | ExprKind::With { .. } => return Ok(false),
            ExprKind::If { condition, .. } if lower::needs(condition) => return Ok(false),
            ExprKind::Match { value, .. } if lower::needs(value) => return Ok(false),
            ExprKind::Block(stmts) => {
                for stmt in stmts {
                    if let Stmt::LetElse {
                        value, else_branch, ..
                    } = stmt
                        && (lower::needs(value) || lower::needs(else_branch))
                    {
                        return Ok(false);
                    }
                }
            }
            ExprKind::Return(_) | ExprKind::If { .. } | ExprKind::Match { .. } => {}
            _ if ir::children(expr).into_iter().any(lower::needs) => return Ok(false),
            _ => {}
        }
        pending.extend(ir::children(expr));
    }
    Ok(true)
}

fn tail_calls(body: &Expr, work: &mut usize) -> Lowering<BTreeSet<usize>> {
    let mut calls = BTreeSet::new();
    let mut pending = vec![(body, true)];
    while let Some((expr, tail)) = pending.pop() {
        charge(work, expr.span)?;
        match &expr.kind {
            ExprKind::Call {
                target: CallTarget::Function(id),
                ..
            } if tail => {
                calls.insert(id.0);
            }
            ExprKind::Return(value) => pending.push((value, true)),
            ExprKind::Block(stmts) => {
                for (index, stmt) in stmts.iter().enumerate() {
                    match stmt {
                        Stmt::Expr(value) => {
                            pending.push((value, tail && index + 1 == stmts.len()))
                        }
                        Stmt::Let { value, .. } => pending.push((value, false)),
                        Stmt::LetElse { .. } => {}
                    }
                }
            }
            ExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                pending.push((then_branch, tail));
                if let Some(otherwise) = else_branch {
                    pending.push((otherwise, tail));
                }
            }
            ExprKind::Match { arms, .. } => {
                pending.extend(arms.iter().map(|arm| (&arm.body, tail)))
            }
            ExprKind::Actor(ir::ActorExpr::Receive { arms, timeout, .. }) => {
                pending.extend(arms.iter().map(|arm| (&arm.body, tail)));
                if let Some((_, body)) = timeout {
                    pending.push((body, tail));
                }
            }
            _ => {}
        }
    }
    Ok(calls)
}

fn charge(work: &mut usize, span: Span) -> Lowering<()> {
    *work += 1;
    if *work > MAX_NODES {
        Err(invalid(
            span,
            "actor tail-helper discovery work limit exceeded",
        ))
    } else {
        Ok(())
    }
}
