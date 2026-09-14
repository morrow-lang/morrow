//! Source bodies are summarized once in dependency order, never interpreted per call site.
use super::*;
#[derive(Default)]
pub(super) struct Summaries {
    pub(super) abstract_traits: HashSet<usize>,
    ready: HashMap<usize, Rc<Summary>>,
    index: HashMap<usize, usize>,
    recursive_defaults: HashMap<usize, recursive::Contract>,
    mutual_groups: HashMap<usize, usize>,
    mutual_members: Vec<Vec<usize>>,
}
impl Summaries {
    /// Membership is structural proof context only, never sufficient handling evidence.
    pub(super) fn same_mutual_group(&self, left: usize, right: usize) -> bool {
        self.mutual_groups
            .get(&left)
            .is_some_and(|group| self.mutual_groups.get(&right) == Some(group))
    }
    /// No outside caller may use a partially verified group's provisional handling effects.
    fn mutual_ready(&self, id: usize, work: &mut usize, span: Span) -> Checked<bool> {
        let Some(group) = self.mutual_groups.get(&id) else {
            return Ok(true);
        };
        let members = &self.mutual_members[*group];
        charge(work, members.len(), span)?;
        Ok(members.iter().all(|id| self.ready.contains_key(id)))
    }
    /// Each eligible SCC member receives a provisional contract before any body is analyzed.
    fn register_mutual(
        &mut self,
        groups: Vec<Vec<(usize, usize)>>,
        work: &mut usize,
        span: Span,
    ) -> Checked<()> {
        charge(work, groups.len(), span)?;
        for group in groups {
            charge(work, group.len().saturating_mul(3), span)?;
            let id = self.mutual_members.len();
            let mut members = Vec::new();
            for (function, parameter) in group {
                self.mutual_groups.insert(function, id);
                self.recursive_defaults
                    .insert(function, recursive::Contract::TreeHandler(parameter));
                members.push(function);
            }
            self.mutual_members.push(members);
        }
        Ok(())
    }
    /// Expose only the checked structural contract identity needed for child traversal proofs.
    pub(super) fn tree_parameter(&self, function: usize) -> Option<usize> {
        match self.recursive_defaults.get(&function) {
            Some(recursive::Contract::TreeHandler(index)) => Some(*index),
            _ => None,
        }
    }
}
/// Build acyclic source summaries under one aggregate budget; recursive equations follow separately.
#[cfg(test)]
pub(super) fn analyze(program: &ir::Program, root: &ir::Function) -> Checked<Summary> {
    let mut work = 0;
    let mut summaries = Summaries::default();
    extend(
        program,
        root,
        None,
        Mode::Concrete,
        &mut summaries,
        &mut work,
    )?;
    let result = summaries
        .ready
        .remove(&root.id.0)
        .ok_or_else(|| Diagnostic::new(root.body.span, "missing Result obligation root summary"))?;
    Rc::try_unwrap(result)
        .map_err(|_| Diagnostic::new(root.body.span, "shared Result obligation root summary"))
}
/// Reuse already checked callees and one aggregate budget across independent source definitions.
pub(super) fn extend(
    program: &ir::Program,
    root: &ir::Function,
    relevance: Option<&HashSet<usize>>,
    mode: Mode,
    summaries: &mut Summaries,
    work: &mut usize,
) -> Checked<()> {
    if summaries.ready.contains_key(&root.id.0) {
        return Ok(());
    }
    if summaries.index.is_empty() {
        charge(work, program.functions.len(), root.body.span)?;
        summaries.index = program
            .functions
            .iter()
            .enumerate()
            .map(|(index, f)| (f.id.0, index))
            .collect();
    }
    let order = dependency_order(program, root, relevance, mode, summaries, work)?;
    summaries.register_mutual(
        mutual_trees::groups(program, &order, work)?,
        work,
        root.body.span,
    )?;
    build_order(
        program,
        &order,
        relevance,
        mode,
        summaries,
        work,
        root.body.span,
    )
}
/// Rebuild every dependent provisional summary after widening, without refunding failed proof work.
fn build_order(
    program: &ir::Program,
    order: &[&ir::Function],
    relevance: Option<&HashSet<usize>>,
    mode: Mode,
    summaries: &mut Summaries,
    work: &mut usize,
    span: Span,
) -> Checked<()> {
    // Each backedge can widen at most once; all dependent provisional summaries must be rebuilt.
    for _ in 0..=order.len() {
        charge(work, order.len(), span)?;
        let mut widened = false;
        for function in order {
            let mut engine = Engine::new(program);
            engine.work = *work;
            engine.summaries = Some(summaries);
            engine.relevance = relevance;
            engine.mode = mode;
            let mut summary = analyze_body(engine, function)?;
            if let Some(contract) = summaries.recursive_defaults.get(&function.id.0)
                && let Err(error) = recursive::verify(contract, &mut summary, function.body.span)
            {
                *work = summary.work;
                if let Some(next) = recursive::widen(program, function, contract, work)? {
                    summaries.recursive_defaults.insert(function.id.0, next);
                    widened = true;
                    break;
                }
                return Err(error);
            }
            *work = summary.work;
            summaries.ready.insert(function.id.0, Rc::new(summary));
        }
        if !widened {
            return Ok(());
        }
        for function in order {
            summaries.ready.remove(&function.id.0);
        }
    }
    Err(Diagnostic::new(
        span,
        "Result obligation builder widening limit exceeded",
    ))
}
/// A bounded graph walk orders declarations independently of their textual order.
fn dependency_order<'a>(
    program: &'a ir::Program,
    root: &'a ir::Function,
    relevance: Option<&HashSet<usize>>,
    mode: Mode,
    summaries: &mut Summaries,
    work: &mut usize,
) -> Checked<Vec<&'a ir::Function>> {
    let mut pending = vec![(root, false)];
    let mut active = HashSet::new();
    let mut done = HashSet::new();
    let mut ordered = Vec::new();
    while let Some((function, exit)) = pending.pop() {
        charge(work, 1, function.body.span)?;
        if exit {
            active.remove(&function.id.0);
            done.insert(function.id.0);
            ordered.push(function);
            continue;
        }
        if done.contains(&function.id.0)
            || summaries.ready.contains_key(&function.id.0)
            || relevance.is_some_and(|set| !set.contains(&function.id.0))
        {
            continue;
        }
        if !active.insert(function.id.0) {
            let contract = recursive::contract(program, function, mode, work)?;
            summaries.recursive_defaults.insert(function.id.0, contract);
            continue;
        }
        pending.push((function, true));
        for id in ordered_dependencies(program, &function.body, work)? {
            if mode == Mode::Template && summaries.abstract_traits.contains(&id) {
                continue;
            }
            let child = summaries
                .index
                .get(&id)
                .and_then(|index| program.functions.get(*index))
                .ok_or_else(|| {
                    Diagnostic::new(function.body.span, "missing Result obligation callee")
                })?;
            charge(work, 1, function.body.span)?;
            pending.push((child, false));
        }
    }
    Ok(ordered)
}
/// Deferred bodies execute in their registering proof context, not as independent recursive assumptions.
pub(super) fn ordered_dependencies(
    program: &ir::Program,
    expr: &ir::Expr,
    work: &mut usize,
) -> Checked<Vec<usize>> {
    let mut pending = vec![expr];
    let mut inline = HashSet::new();
    let mut found = BTreeMap::new();
    while let Some(expr) = pending.pop() {
        charge(work, 1, expr.span)?;
        if let ir::ExprKind::Defer(value) = &expr.kind
            && let ir::ExprKind::Closure { function, captures } = &value.kind
        {
            charge(work, captures.len(), expr.span)?;
            pending.extend(captures);
            if inline.insert(function.0) {
                charge(work, program.functions.len(), expr.span)?;
                let body = program
                    .functions
                    .iter()
                    .find(|f| f.id == *function)
                    .ok_or_else(|| {
                        Diagnostic::new(expr.span, "missing deferred obligation function")
                    })?;
                pending.push(&body.body);
            }
            continue;
        }
        match &expr.kind {
            ir::ExprKind::Call {
                target: ir::CallTarget::Function(id),
                ..
            }
            | ir::ExprKind::Actor(ir::ActorExpr::Call { function: id, .. })
            | ir::ExprKind::Closure { function: id, .. } => {
                found.insert(id.0, ());
            }
            _ => {}
        }
        for child in ir::children(expr) {
            charge(work, 1, child.span)?;
            pending.push(child);
        }
    }
    Ok(found.into_keys().collect())
}
/// Scan all typed children, charging each edge before it enters the bounded traversal queue.
pub(super) fn dependencies(expr: &ir::Expr, work: &mut usize) -> Checked<Vec<usize>> {
    let mut pending = vec![expr];
    let mut found = BTreeMap::new();
    while let Some(expr) = pending.pop() {
        charge(work, 1, expr.span)?;
        if let ir::ExprKind::Call {
            target: ir::CallTarget::Function(id),
            ..
        }
        | ir::ExprKind::Actor(ir::ActorExpr::Call { function: id, .. }) = &expr.kind
        {
            found.insert(id.0, ());
        }
        if let ir::ExprKind::Closure { function, .. } = &expr.kind {
            found.insert(function.0, ());
        }
        for child in ir::children(expr) {
            charge(work, 1, child.span)?;
            pending.push(child);
        }
    }
    Ok(found.into_keys().collect())
}
impl Engine<'_> {
    /// Instantiate finalized parameter and output provenance without treating a call as consuming.
    pub(super) fn source_call(&mut self, id: usize, args: &[Value], span: Span) -> Checked<Value> {
        if let Some(summaries) = self.summaries {
            if recursive_trees::context(self)
                .is_some_and(|(caller, _, _)| summaries.same_mutual_group(caller, id))
            {
                let function = summaries
                    .index
                    .get(&id)
                    .and_then(|index| self.program.functions.get(*index))
                    .ok_or_else(|| Diagnostic::new(span, "missing mutual tree function"))?;
                let parameter = summaries
                    .tree_parameter(id)
                    .ok_or_else(|| Diagnostic::new(span, "missing mutual tree contract"))?;
                return recursive_trees::apply(self, function, parameter, args, span);
            }
            if !summaries.mutual_ready(id, &mut self.work, span)? {
                return Err(Diagnostic::new(
                    span,
                    "Result obligation mutual handler group is not fully verified",
                ));
            }
        }
        if let Some(summaries) = self.summaries
            && let Some(contract) = summaries
                .recursive_defaults
                .get(&id)
                .filter(|_| !summaries.ready.contains_key(&id))
        {
            let function = summaries
                .index
                .get(&id)
                .and_then(|index| self.program.functions.get(*index))
                .ok_or_else(|| Diagnostic::new(span, "missing recursive obligation signature"))?;
            return recursive::apply(self, contract, function, args, span);
        }
        if let Some(summaries) = self.summaries
            && let Some(recursive::Contract::TreeHandler(index)) =
                summaries.recursive_defaults.get(&id)
            && summaries.ready.contains_key(&id)
        {
            return recursive_trees::complete(self, *index, args, span);
        }
        let specialized = self.effect_summary(id, args, span)?;
        let summary = specialized
            .or_else(|| self.summaries.and_then(|s| s.ready.get(&id)).cloned())
            .ok_or_else(|| Diagnostic::new(span, "missing Result obligation callee summary"))?;
        let mut substitution = super::substitute::Substitution::new(&summary);
        substitution.apply(self, args, span)
    }
}
