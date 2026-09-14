//! A single member hole is checked locally after its enclosing source signature is independently fixed.
//! Hole-dependent local unknowns never enter SCC inference, reusable schemes, or executable IR.
use super::*;
use crate::parse::HoleSite;

pub(super) struct State {
    pub(super) site: HoleSite,
    result: Option<Type>,
    receiver: Option<Type>,
}

/// Validate every unaffected function, then prove one local incomplete operation without code generation.
pub(crate) fn analyze(source: &ast::Program, site: &HoleSite) -> Checked<editor::Facts> {
    preflight::check(source)?;
    let expanded = aliases::expand(source)?;
    let source = expanded.program.as_ref();
    let registry = nominal::Registry::new(source)?;
    aliases::validate(source, &registry)?;
    let selected = source
        .functions
        .iter()
        .find(|f| f.span.start <= site.field().start && site.field().end <= f.span.end)
        .ok_or_else(|| failure(site, "member hole is outside a function"))?;
    independent_group(source, &selected.name, site)?;
    let graph = dependencies::analyze_with_work(source, expanded.work)?;
    independent_component(source, &graph, &selected.name, site)?;
    let (prepared, mut signatures, work) = whole::resolve(source, &registry, &graph)?;
    labels::finalize(&prepared, &mut signatures)?;
    let mut checked = schemes::validate(&prepared, &registry, &mut signatures, work)?;
    for function in &prepared.functions {
        if function.name != selected.name && signatures[&function.name].generics.is_empty() {
            checked.push(checker(&registry, &signatures, None).function(function)?);
        }
    }
    let function = prepared
        .functions
        .iter()
        .find(|f| f.name == selected.name)
        .ok_or_else(|| failure(site, "missing source group"))?;
    let mut checker = checker(&registry, &signatures, Some(site.clone()));
    checked.push(checker.function(function)?);
    prove_obligations(checked, &registry, registry.codec_template_work.get())?;
    let receiver = checker
        .recovery
        .and_then(|state| state.receiver)
        .ok_or_else(|| failure(site, "member receiver has no independent type"))?;
    let facts = editor::receiver_facts(source, &registry, receiver, site.field())?;
    if facts
        .members
        .iter()
        .any(|member| member.name == site.original())
    {
        return Err(failure(
            site,
            "complete known member cannot suppress an unrelated error",
        ));
    }
    Ok(facts)
}

/// Keep the private hole explicit while lifting all checked cleanup and closure bodies for proof.
fn prove_obligations(
    mut functions: Vec<ir::Function>,
    registry: &nominal::Registry,
    work: usize,
) -> Checked<()> {
    lift::run(&mut functions)?;
    let types = registry.layouts(&functions)?;
    obligations::check_recovery(&ir::Program { functions, types }, work)
}

/// Require a complete concrete source signature for every clause of the selected group.
fn independent_group(source: &ast::Program, name: &str, site: &HoleSite) -> Checked<()> {
    for function in source.functions.iter().filter(|f| f.name == name) {
        if !complete(function) {
            return Err(failure(
                site,
                "member recovery requires an independently annotated function signature",
            ));
        }
        let result = function_result(function)?;
        let types = function
            .params
            .iter()
            .filter_map(|p| p.annotation.clone())
            .chain([result]);
        if !nominal::generics(types).is_empty() {
            return Err(failure(
                site,
                "generic outer signatures require ordinary complete source",
            ));
        }
    }
    Ok(())
}

/// Refuse an incomplete recursive peer that would cause ordinary whole inference to inspect this hole.
fn independent_component(
    source: &ast::Program,
    graph: &dependencies::Graph,
    name: &str,
    site: &HoleSite,
) -> Checked<()> {
    let selected = graph
        .groups
        .iter()
        .position(|g| g.name == name)
        .ok_or_else(|| failure(site, "unknown source group"))?;
    for component in &graph.components {
        if !component.contains(&selected) {
            continue;
        }
        for &index in component {
            for function in &source.functions[graph.groups[index].clauses.clone()] {
                if !complete(function) {
                    return Err(failure(
                        site,
                        "recursive recovery requires fixed source signatures",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Source main retains its established Unit default; all other selected slots must be explicit.
fn complete(function: &ast::Function) -> bool {
    function.params.iter().all(|p| p.annotation.is_some())
        && (function.name == "main" || function.return_type.is_some())
}

/// All ordinary function checks use the same defaults; recovery is explicit and local to one body.
fn checker<'a>(
    registry: &'a nominal::Registry,
    signatures: &'a HashMap<String, Signature>,
    site: Option<HoleSite>,
) -> Checker<'a> {
    Checker {
        mailbox: None,
        editor: None,
        recovery: site.map(|site| State {
            site,
            result: None,
            receiver: None,
        }),
        signatures,
        registry,
        scopes: vec![HashMap::new()],
        local_count: 0,
        expr_count: 0,
        inference: Inference::default(),
        function_return: Type::Unit,
        deferred: false,
        loop_depth: 0,
    }
}

/// Keep recovery refusal located at the original operation, never at an inserted token.
fn failure(site: &HoleSite, message: &str) -> Diagnostic {
    Diagnostic::new(site.field(), message)
}

impl Checker<'_> {
    /// Ordinary checker finalization never accepts an editor operation, even when unreachable.
    pub(super) fn finalize_member_hole(&self, receiver: &mut ir::Expr, span: Span) -> Checked<()> {
        if self.recovery.is_none() {
            return Err(Diagnostic::new(
                span,
                "editor hole cannot enter ordinary finalization",
            ));
        }
        self.finalize(receiver)
    }

    /// Resolve the receiver completely before creating any member-result inference variable.
    pub(super) fn editor_member_hole(
        &mut self,
        expr: &ast::Expr,
        depth: usize,
    ) -> Checked<TypedKind> {
        let ast::ExprKind::Field { value, .. } = &expr.kind else {
            return Err(Diagnostic::new(expr.span, "invalid member hole"));
        };
        let receiver = self.expression(value, depth)?;
        let concrete = self.inference.concrete(&receiver.ty, receiver.span)?;
        if !nominal::generics([concrete.clone()]).is_empty() || concrete == Type::Never {
            return Err(Diagnostic::new(
                expr.span,
                "member receiver is not independently concrete",
            ));
        }
        let result = self.inference.fresh();
        let state = self
            .recovery
            .as_mut()
            .ok_or_else(|| Diagnostic::new(expr.span, "missing editor proof"))?;
        if state.receiver.replace(concrete).is_some() {
            return Err(failure(&state.site, "multiple editor member operations"));
        }
        state.result = Some(result.clone());
        Ok((
            ir::ExprKind::EditorHole {
                token: ir::EditorHoleToken::new(),
                receiver: Box::new(receiver),
            },
            result,
        ))
    }

    /// Rigidify only unresolved classes reachable from this local result; no scheme is generalized.
    pub(super) fn seal_member_hole(&mut self) -> Checked<()> {
        let Some(state) = &self.recovery else {
            return Ok(());
        };
        let result = state
            .result
            .clone()
            .ok_or_else(|| failure(&state.site, "member hole was not checked"))?;
        let span = state.site.field();
        let result = self.inference.resolve(&result, span)?;
        let variables = unresolved(&result, span)?;
        if !variables.is_empty() {
            self.inference.template = true;
            for (index, variable) in variables.into_iter().enumerate() {
                let name = format!("$editor_member_hole_{index}");
                self.inference.template_names.insert(name.clone());
                self.inference.unify(
                    &Type::Infer(variable),
                    &Type::Generic(name),
                    span,
                    "editor hole result",
                )?;
            }
        }
        Ok(())
    }
}

/// Collect only unresolved result classes under the ordinary structural type budget.
fn unresolved(ty: &Type, span: Span) -> Checked<Vec<u32>> {
    let mut pending = vec![ty];
    let mut variables = std::collections::BTreeSet::new();
    let mut nodes = 0;
    while let Some(ty) = pending.pop() {
        nodes += 1;
        if nodes > MAX_TYPE_NODES {
            return Err(Diagnostic::new(span, "editor hole type limit exceeded"));
        }
        match ty {
            Type::Infer(id) => {
                variables.insert(*id);
            }
            Type::Function(args, result) => {
                pending.extend(args);
                pending.push(result);
            }
            Type::Union(args) | Type::Tuple(args) | Type::Named(_, args) => pending.extend(args),
            Type::List(x) | Type::Option(x) => pending.push(x),
            Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                pending.push(a);
                pending.push(b);
            }
            _ => {}
        }
    }
    Ok(variables.into_iter().collect())
}
