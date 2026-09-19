//! Bounded root/actor sensitivity is separate from actor-only mailbox inference.
use super::*;

/// Include nested closure bodies and lexical references: their instantiation depends on context.
pub(super) fn attach(program: &ast::Program, signatures: &mut HashMap<String, Signature>, inference: &mut Inference) -> Checked<()> {
    let mut seeded = HashSet::new();
    let mut work = 0usize;
    for function in &program.functions {
        let mut pending = vec![&function.body];
        while let Some(expr) = pending.pop() {
            work += 1;
            if work > MAX_EXPR_COUNT * 4 { return Err(Diagnostic::new(expr.span, "context effect work limit exceeded")); }
            if source_name(expr).is_some_and(crate::supervisors::contextual) { seeded.insert(function.name.clone()); }
            pending.extend(crate::actors::source_children(expr));
        }
    }
    if seeded.is_empty() { return Ok(()); }
    let graph = dependencies::analyze_with_work(program, work)?;
    inference.probe_work.set(inference.probe_work.get().saturating_add(graph.work));
    if inference.probe_work.get() > MAX_EXPR_COUNT * 4 { return Err(Diagnostic::new(Span::default(), "context effect inference work limit exceeded")); }
    let mut callers = vec![Vec::new(); graph.groups.len()];
    let mut pending = Vec::new();
    for (index, group) in graph.groups.iter().enumerate() {
        if seeded.contains(&group.name) { pending.push(index); }
        for &callee in &group.callees { callers[callee].push(index); }
    }
    while let Some(index) = pending.pop() {
        let signature = signatures.get_mut(&graph.groups[index].name).expect("registered context function");
        if signature.contextual { continue; }
        signature.contextual = true;
        pending.extend(&callers[index]);
    }
    Ok(())
}
fn source_name(expr: &ast::Expr) -> Option<&str> {
    match &expr.kind {
        ast::ExprKind::Call { name, .. } | ast::ExprKind::Pipe { name, .. } | ast::ExprKind::Name(name) => Some(name),
        ast::ExprKind::GlobalCall { resolved, .. } | ast::ExprKind::GlobalPipe { resolved, .. } | ast::ExprKind::GlobalName { resolved, .. } => Some(resolved),
        _ => None,
    }
}
impl Checker<'_> {
    /// Sensitivity changes code selection, not whether merely creating a lambda suspends its owner.
    pub(super) fn contextual_body(&self, body: &ast::Expr) -> bool {
        let mut pending = vec![body];
        while let Some(expr) = pending.pop() {
            if source_name(expr).is_some_and(|name| crate::supervisors::contextual(name) || self.signatures.get(name).is_some_and(|f| f.contextual)) { return true; }
            pending.extend(crate::actors::source_children(expr));
        }
        false
    }
}
