//! Recursive callable collections retain original targets plus a bounded set of independently safe code.
use super::*;
#[derive(Clone)]
pub(super) struct Contract {
    targets: Vec<usize>,
    input: Option<usize>,
    map: bool,
}
/// Admit only structurally closed callable interfaces; their bodies are still independently checked.
pub(super) fn candidate(
    program: &ir::Program,
    function: &ir::Function,
    work: &mut usize,
) -> Checked<Option<Contract>> {
    let span = function.body.span;
    let (map, callable) = match &function.return_type {
        Type::List(inner) => (false, inner.as_ref()),
        Type::Map(key, inner) if recursive::closed_type(key, false, work, span)? => {
            (true, inner.as_ref())
        }
        _ => return Ok(None),
    };
    let Type::Function(params, result) = callable else {
        return Ok(None);
    };
    for ty in params.iter().chain([result.as_ref()]) {
        if !recursive::closed_type(ty, false, work, span)? {
            return Ok(None);
        }
    }
    let mut input = None;
    for (index, param) in function.params.iter().chain(&function.captures).enumerate() {
        gate::type_cost(&param.ty, work, span)?;
        if param.ty == function.return_type {
            if input.replace(index).is_some() {
                return Ok(None);
            }
        } else if !recursive::closed_type(&param.ty, false, work, span)? {
            return Ok(None);
        }
    }
    let targets = safe_targets(program, &function.body, callable, work)?;
    Ok((!targets.is_empty()).then_some(Contract {
        targets,
        input,
        map,
    }))
}
/// Finite code identities may vary scalar captures, but may not conceal captured duties or callbacks.
fn safe_targets(
    program: &ir::Program,
    body: &ir::Expr,
    callable: &Type,
    work: &mut usize,
) -> Checked<Vec<usize>> {
    let mut pending = vec![body];
    let mut targets = BTreeMap::new();
    while let Some(expr) = pending.pop() {
        charge(work, 1, expr.span)?;
        if let ir::ExprKind::Closure { function, .. } = &expr.kind {
            gate::type_cost(&expr.ty, work, expr.span)?;
            if expr.ty == *callable && closed_target(program, function.0, work, expr.span)? {
                if targets.len() >= DEPTH_LIMIT && !targets.contains_key(&function.0) {
                    return Err(Diagnostic::new(
                        expr.span,
                        "Result recursive callable target limit exceeded",
                    ));
                }
                targets.insert(function.0, ());
            }
        }
        for child in ir::children(expr) {
            charge(work, 1, expr.span)?;
            pending.push(child);
        }
    }
    if targets.len() > DEPTH_LIMIT {
        return Err(Diagnostic::new(
            body.span,
            "Result recursive callable target limit exceeded",
        ));
    }
    Ok(targets.into_keys().collect())
}
/// Closed code has no conditional callback requirement; its complete body remains in ordinary proof.
fn closed_target(program: &ir::Program, id: usize, work: &mut usize, span: Span) -> Checked<bool> {
    charge(work, program.functions.len(), span)?;
    let function = program
        .functions
        .iter()
        .find(|f| f.id.0 == id)
        .ok_or_else(|| Diagnostic::new(span, "missing recursive callable code"))?;
    for ty in function
        .params
        .iter()
        .chain(&function.captures)
        .map(|p| &p.ty)
        .chain([&function.return_type])
    {
        if !recursive::closed_type(ty, false, work, span)? {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Approximate any finite output length without dropping possible original input function targets.
pub(super) fn apply(
    engine: &mut Engine<'_>,
    contract: &Contract,
    args: &[Value],
    span: Span,
) -> Checked<Value> {
    engine.charge(contract.targets.len().saturating_add(1), span)?;
    let mut values = Vec::new();
    if let Some(index) = contract.input {
        let input = args
            .get(index)
            .ok_or_else(|| Diagnostic::new(span, "missing recursive callable collection input"))?;
        values.push(substitute::Substitution::family(
            engine,
            input,
            contract.map,
            span,
            0,
        )?);
    }
    for id in &contract.targets {
        engine.charge(engine.program.functions.len(), span)?;
        let function = engine
            .program
            .functions
            .iter()
            .find(|f| f.id.0 == *id)
            .ok_or_else(|| Diagnostic::new(span, "missing recursive callable capture layout"))?;
        engine.charge(function.captures.len(), span)?;
        let captures = function
            .captures
            .iter()
            .map(|p| engine.fresh(&p.ty, None, span, 0))
            .collect::<Checked<_>>()?;
        values.push(engine.node(
            Region::Callable {
                function: *id,
                captures,
            },
            span,
        )?);
    }
    let kind = if contract.map {
        Region::Map {
            entries: values.into_iter().map(|v| (None, v)).collect(),
            exact: false,
            nonempty: engine.predicates.variable(&mut engine.work, span)?,
        }
    } else {
        Region::List {
            items: values,
            exact: false,
            nonempty: engine.predicates.variable(&mut engine.work, span)?,
        }
    };
    engine.node(kind, span)
}
/// Every actual returned leaf must retain an input target or one of the independently closed targets.
pub(super) fn verify(contract: &Contract, summary: &mut Summary, span: Span) -> Checked<()> {
    let mut allowed = HashSet::new();
    if let Some(index) = contract.input {
        let input = summary
            .inputs
            .get(index)
            .ok_or_else(|| Diagnostic::new(span, "missing recursive callable proof input"))?;
        let mut pending = vec![input];
        while let Some(value) = pending.pop() {
            charge(&mut summary.work, 1, span)?;
            allowed.insert(value.node.id);
            for child in values::children(&value.node.kind) {
                charge(&mut summary.work, 1, span)?;
                pending.push(child);
            }
        }
    }
    let mut pending = vec![(&summary.output, 0usize)];
    while let Some((value, depth)) = pending.pop() {
        charge(&mut summary.work, 1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result recursive callable proof depth limit exceeded",
            ));
        }
        if allowed.contains(&value.node.id) {
            continue;
        }
        match &value.node.kind {
            Region::Callable { function, .. } => {
                charge(&mut summary.work, contract.targets.len(), span)?;
                if !contract.targets.contains(function) {
                    return Err(Diagnostic::new(
                        span,
                        "Result recursive collection contains unproven callable code",
                    ));
                }
            }
            Region::Choice(choices) => {
                charge(&mut summary.work, choices.len(), span)?;
                for (guard, value) in choices {
                    if *guard != Predicate::FALSE {
                        pending.push((value, depth + 1));
                    }
                }
            }
            Region::List { .. } | Region::Map { .. } => {
                for value in values::children(&value.node.kind) {
                    charge(&mut summary.work, 1, span)?;
                    pending.push((value, depth + 1));
                }
            }
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "Result obligation recursive collection loses callable target provenance",
                ));
            }
        }
    }
    Ok(())
}
