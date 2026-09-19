//! The production boundary never treats a Result-free signature as a Result-free body.
use super::*;
/// Check every concrete definition, sharing the proof budget across all independent roots.
pub(in crate::check) fn check(program: &ir::Program, work: usize) -> Checked<()> {
    ir::reject_probes(program)?;
    check_mode(program, Mode::Concrete, work).map(|_| ())
}
/// Recovery has an explicit opaque borrowing operation, never an executable or Result-free value.
pub(in crate::check) fn check_recovery(program: &ir::Program, work: usize) -> Checked<()> {
    check_mode(program, Mode::Editor, work).map(|_| ())
}
/// Both publication boundaries share the same body relevance and obligation proof.
fn check_mode(program: &ir::Program, mode: Mode, work: usize) -> Checked<usize> {
    check_roots(program, mode, work, None, &HashSet::new())
}
/// Generic source roots supplement the complete concrete-body pass; dependencies remain available.
fn check_roots(
    program: &ir::Program,
    mode: Mode,
    mut work: usize,
    roots: Option<&HashSet<usize>>,
    abstract_traits: &HashSet<usize>,
) -> Checked<usize> {
    charge(&mut work, program.functions.len(), Span::default())?;
    let mut relevant = HashSet::new();
    for function in &program.functions {
        if function_relevant(program, function, &mut work)? {
            relevant.insert(function.id.0);
        }
    }
    close_relevance(program, &mut relevant, &mut work)?;
    let mut summaries = calls::Summaries::default();
    summaries.abstract_traits = abstract_traits.clone();
    for function in &program.functions {
        if !relevant.contains(&function.id.0)
            || roots.is_some_and(|roots| !roots.contains(&function.id.0))
        {
            continue;
        }
        calls::extend(
            program,
            function,
            Some(&relevant),
            mode,
            &mut summaries,
            &mut work,
        )?;
    }
    Ok(work)
}
/// Latent callback effects make their callers relevant even when every visible result is scalar.
fn close_relevance(
    program: &ir::Program,
    relevant: &mut HashSet<usize>,
    work: &mut usize,
) -> Checked<()> {
    let mut callers: HashMap<usize, Vec<usize>> = HashMap::new();
    for function in &program.functions {
        for target in calls::dependencies(&function.body, work)? {
            charge(work, 1, function.body.span)?;
            callers.entry(target).or_default().push(function.id.0);
        }
    }
    charge(work, relevant.len(), Span::default())?;
    let mut pending: Vec<_> = relevant.iter().copied().collect();
    while let Some(target) = pending.pop() {
        charge(work, 1, Span::default())?;
        if let Some(parents) = callers.get(&target) {
            for id in parents {
                charge(work, 1, Span::default())?;
                if relevant.insert(*id) {
                    pending.push(*id);
                }
            }
        }
    }
    Ok(())
}
/// Visit actual parameter, capture and body operations; function-valued types alone are not duties.
fn function_relevant(
    program: &ir::Program,
    function: &ir::Function,
    work: &mut usize,
) -> Checked<bool> {
    let span = function.body.span;
    if contains_mode(program, &function.return_type, work, span, true)? {
        return Ok(true);
    }
    for ty in function
        .params
        .iter()
        .chain(&function.captures)
        .map(|p| &p.ty)
        .chain([&function.return_type])
    {
        if contains(program, ty, work, span)? {
            return Ok(true);
        }
    }
    let mut pending = vec![&function.body];
    while let Some(expr) = pending.pop() {
        charge(work, 1, expr.span)?;
        if matches!(
            expr.kind,
            ir::ExprKind::EditorHole { .. }
                | ir::ExprKind::Actor(ir::ActorExpr::Process(
                    crate::processes::ProcessExpr::Exit { .. }
                ))
        ) || matches!(&expr.kind, ir::ExprKind::Actor(ir::ActorExpr::Supervisor(operation)) if operation.terminal()) || contains(program, &expr.ty, work, expr.span)?
        {
            return Ok(true);
        }
        for child in ir::children(expr) {
            charge(work, 1, child.span)?;
            pending.push(child);
        }
    }
    Ok(false)
}
/// Recursive nominal storage is a finite graph; visit layout identities without recursive expansion.
pub(super) fn contains<'a>(
    program: &'a ir::Program,
    ty: &'a Type,
    work: &mut usize,
    span: Span,
) -> Checked<bool> {
    contains_mode(program, ty, work, span, false)
}
/// Returned callable structure carries latent target identity even though it is not an existing duty.
pub(super) fn contains_mode<'a>(
    program: &'a ir::Program,
    ty: &'a Type,
    work: &mut usize,
    span: Span,
    callable: bool,
) -> Checked<bool> {
    let mut pending = vec![ty];
    let mut layouts = HashSet::new();
    while let Some(ty) = pending.pop() {
        type_cost(ty, work, span)?;
        match ty {
            Type::Result(..) => return Ok(true),
            Type::Function(..) | Type::RootFunction(_) | Type::ActorFunction(..) | Type::Generic(_) if callable => return Ok(true),
            Type::Option(inner) | Type::List(inner) => pending.push(inner),
            Type::Map(_, inner) => pending.push(inner),
            Type::Tuple(fields) | Type::Union(fields) => {
                charge(work, fields.len(), span)?;
                pending.extend(fields);
            }
            Type::Named(..) => {
                let index = layout_index(program, ty, work, span)?;
                if !layouts.insert(index) {
                    continue;
                }
                for fields in &program.types[index].variants {
                    for field in fields {
                        charge(work, 1, span)?;
                        pending.push(field);
                    }
                }
            }
            Type::Infer(_) => {
                return Err(Diagnostic::new(
                    span,
                    "Result obligation encountered unresolved type",
                ));
            }
            _ => {}
        }
    }
    Ok(false)
}
/// Charge complete structural identity before comparing layout type keys.
pub(super) fn layout_index(
    program: &ir::Program,
    ty: &Type,
    work: &mut usize,
    span: Span,
) -> Checked<usize> {
    charge(work, program.types.len(), span)?;
    for (index, layout) in program.types.iter().enumerate() {
        type_cost(&layout.ty, work, span)?;
        if layout.ty == *ty {
            return Ok(index);
        }
    }
    Err(Diagnostic::new(
        span,
        "Result obligation requires a nominal layout",
    ))
}
/// Bound every structural edge and name before equality, cloning or lookup can traverse it.
pub(super) fn type_cost(ty: &Type, work: &mut usize, span: Span) -> Checked<()> {
    let mut pending = vec![(ty, 0usize)];
    while let Some((ty, depth)) = pending.pop() {
        charge(work, 1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation type depth limit exceeded",
            ));
        }
        match ty {
            Type::Named(name, fields) => {
                charge(work, name.len().saturating_add(fields.len()), span)?;
                pending.extend(fields.iter().map(|t| (t, depth + 1)));
            }
            Type::Tuple(fields) | Type::Union(fields) => {
                charge(work, fields.len(), span)?;
                pending.extend(fields.iter().map(|t| (t, depth + 1)));
            }
            Type::Function(fields, result) => {
                charge(work, fields.len().saturating_add(1), span)?;
                pending.push((result, depth + 1));
                pending.extend(fields.iter().map(|t| (t, depth + 1)));
            }
            Type::Option(t) | Type::List(t) => pending.push((t, depth + 1)),
            Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                pending.push((a, depth + 1));
                pending.push((b, depth + 1));
            }
            Type::Generic(name) => charge(work, name.len(), span)?,
            _ => {}
        }
    }
    Ok(())
}
/// Symbolic input regions retain identity without inventing a concrete witness or acknowledging duties.
pub(in crate::check) fn templates(
    mut functions: Vec<ir::Function>,
    registry: &super::super::nominal::Registry,
    roots: &HashSet<usize>,
    abstract_traits: &HashSet<usize>,
) -> Checked<usize> {
    let reserved = abstract_traits
        .iter()
        .copied()
        .max()
        .map_or(0, |id| id.saturating_add(1));
    super::super::lift::run_reserved(&mut functions, reserved)?;
    let types = registry.layouts(&functions)?;
    let program = ir::Program { functions, types };
    let mut work = registry.codec_template_work.get();
    ir::validate_codec_templates(&program, &mut work)?;
    check_roots(&program, Mode::Template, work, Some(roots), abstract_traits)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concrete_publication_continues_the_template_proof_budget() {
        let function = ir::Function {
            root_context: false,
            mailbox: None,
            id: ir::FunctionId(0),
            name: "unit".into(),
            params: vec![],
            captures: vec![],
            return_type: Type::Unit,
            body: ir::Expr {
                kind: ir::ExprKind::Unit,
                ty: Type::Unit,
                span: Span::default(),
            },
            local_count: 0,
        };
        let program = ir::Program {
            functions: vec![function],
            types: vec![],
        };
        let error = check(&program, WORK_LIMIT).unwrap_err();
        assert!(error.message.contains("work limit"), "{error:?}");
    }
}
