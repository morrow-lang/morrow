//! Infer private return schemes before concrete specialization, retaining recursive constraints.
use super::*;
use std::borrow::Cow;

pub(super) const WAITING: &str = "return inference is waiting for a dependency";
/// A call-site result stays independent until its callee's generic scheme is known.
pub(super) struct DeferredCall {
    target: Type,
    substitutions: HashMap<String, Type>,
    result: Type,
    template: bool,
    span: Span,
}

type Prepared<'a> = (Cow<'a, ast::Program>, HashMap<String, Signature>, usize);

/// Reserve one shared result variable per private definition lacking an annotation.
pub(super) fn initial_result(function: &ast::Function, inference: &mut Inference) -> Checked<Type> {
    if function.name != "main" && !function.public && function.return_type.is_none() {
        Ok(inference.fresh())
    } else {
        function_result(function)
    }
}

/// Accept the documented entry contracts without allowing polymorphic process results.
pub(super) fn main_result(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Unit => true,
        Type::Result(ok, error) => {
            **ok == Type::Unit
                && nominal::generics([*error.clone()]).is_empty()
                && !has_infer(error)
        }
        _ => false,
    }
}

/// Solve shared return constraints, then publish annotations for the existing monomorphizer.
pub(super) fn resolve<'a>(
    program: &'a ast::Program,
    registry: &nominal::Registry,
    dispatch: &HashSet<String>,
) -> Checked<Prepared<'a>> {
    let mut inference = Inference {
        newtypes: registry.newtype_definitions(),
        newtype_work: registry.newtype_budget(),
        ..Inference::default()
    };
    let mut signatures = super::signatures(program, registry, &mut inference, dispatch)?;
    let missing: Vec<_> = program
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| f.return_type.is_none() && f.name != "main")
        .map(|(i, _)| i)
        .collect();
    if missing.is_empty() {
        return Ok((
            Cow::Borrowed(program),
            signatures,
            inference.probe_work.get(),
        ));
    }
    solve(program, registry, &mut signatures, &mut inference, &missing)?;
    let mut source = program.clone();
    for index in missing {
        let function = &mut source.functions[index];
        let result = inference.resolve(&signatures[&function.name].result, function.span)?;
        if has_infer(&result) {
            return Err(unresolved(function));
        }
        validate_type(&result, function.span)?;
        let allowed = signatures[&function.name]
            .generics
            .iter()
            .cloned()
            .collect();
        registry.validate(&result, &allowed, function.span)?;
        signatures
            .get_mut(&function.name)
            .expect("collected function")
            .result = result.clone();
        function.return_type = Some(result);
    }
    Ok((Cow::Owned(source), signatures, inference.probe_work.get()))
}

/// Retry only shape-dependent inference, stopping after each definition can succeed once.
pub(super) fn solve(
    program: &ast::Program,
    registry: &nominal::Registry,
    signatures: &mut HashMap<String, Signature>,
    inference: &mut Inference,
    missing: &[usize],
) -> Checked<()> {
    let mut pending = missing.to_vec();
    for _ in 0..=missing.len() {
        if pending.is_empty() {
            settle_calls(inference)?;
            return Ok(());
        }
        let previous = pending.len();
        let mut waiting = Vec::new();
        for index in pending {
            let function = &program.functions[index];
            match probe(function, registry, signatures, inference) {
                Ok(_) => {}
                Err(error) if error.message.contains(WAITING) => waiting.push(index),
                Err(error) => return Err(error),
            }
        }
        let settled = settle_calls(inference)?;
        if waiting.len() == previous && !settled {
            return Err(unresolved(&program.functions[waiting[0]]));
        }
        pending = waiting;
    }
    Err(Diagnostic::new(
        Span::default(),
        "return inference work limit exceeded",
    ))
}

/// Check one body with shared recursive result slots, postponing concrete IR validation.
pub(super) fn probe(
    function: &ast::Function,
    registry: &nominal::Registry,
    signatures: &HashMap<String, Signature>,
    inference: &mut Inference,
) -> Checked<ir::Expr> {
    let signature = &signatures[&function.name];
    let mut checker = Checker {
        mailbox: signature.mailbox.clone(),
        editor: None,
        recovery: None,
        signatures,
        registry,
        scopes: vec![HashMap::new()],
        local_count: 0,
        expr_count: 0,
        inference: std::mem::take(inference),
        function_return: signature.result.clone(),
        deferred: false,
        loop_depth: 0,
    };
    checker.inference.probing = true;
    checker.inference.template = !signature.generics.is_empty();
    checker.inference.template_names = signature.generics.iter().cloned().collect();
    for param in &function.params {
        checker.bind(
            clauses::parameter_name(param),
            clauses::parameter_type(param).clone(),
        );
    }
    let expected = if function.name == "main" && signature.result == Type::Unit {
        None
    } else {
        Some(&signature.result)
    };
    let result = checker
        .expression_expected(&function.body, expected, 0)
        .map_err(|error| closures::context(error, "function return"));
    *inference = checker.inference;
    inference.probing = false;
    inference.template = false;
    inference.template_names.clear();
    result
}

/// Instantiate a known scheme or defer a fresh call result without leaking generic names.
pub(super) fn call_result(
    inference: &mut Inference,
    signature: &Signature,
    substitutions: &HashMap<String, Type>,
    span: Span,
) -> Checked<Type> {
    let result = inference.resolve(&signature.result, span)?;
    if inference.probing
        && (inference.template || !signature.generics.is_empty())
        && has_infer(&result)
    {
        let result = inference.fresh();
        inference.pending_returns.push(DeferredCall {
            target: signature.result.clone(),
            substitutions: substitutions.clone(),
            result: result.clone(),
            template: inference.template,
            span,
        });
        Ok(result)
    } else {
        nominal::substitute(&result, substitutions)
    }
}

/// Publish newly anchored schemes into independent call variables within the aggregate budget.
pub(super) fn settle_calls(inference: &mut Inference) -> Checked<bool> {
    let previous = std::mem::replace(&mut inference.settling, true);
    let result = settle_inner(inference);
    inference.settling = previous;
    result
}

/// Iterate only deferred obligations whose signatures gained concrete structure.
fn settle_inner(inference: &mut Inference) -> Checked<bool> {
    let mut changed = false;
    for _ in 0..=inference.pending_returns.len() {
        let calls = std::mem::take(&mut inference.pending_returns);
        let previous = calls.len();
        for call in calls {
            charge_work(inference, call.span)?;
            let target = inference.resolve(&call.target, call.span)?;
            if has_infer(&target) {
                inference.pending_returns.push(call);
                continue;
            }
            let target = nominal::substitute(&target, &call.substitutions)?;
            let previous_template = std::mem::replace(&mut inference.template, call.template);
            let result = inference.unify(&call.result, &target, call.span, "inferred call return");
            inference.template = previous_template;
            result?;
        }
        if inference.pending_returns.len() == previous {
            return Ok(changed);
        }
        changed = true;
    }
    Ok(changed)
}

/// Delay projections whose receiver shape may be established by a later definition.
pub(super) fn shape_ready(inference: &Inference, ty: &Type, span: Span) -> Checked<()> {
    if inference.probing && matches!(inference.resolve(ty, span)?, Type::Infer(_)) {
        Err(Diagnostic::new(span, WAITING))
    } else {
        Ok(())
    }
}

/// Distinguish unresolved payload variables from valid declared generic names.
pub(super) fn has_infer(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Type::Infer(_) => return true,
            Type::ChildKey(a) | Type::RootFunction(a) | Type::List(a) | Type::Option(a) => pending.push(a),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.push(a);
                pending.push(b);
            }
            Type::Function(args, result) => {
                pending.extend(args);
                pending.push(result);
            }
            Type::Union(args) | Type::Tuple(args) | Type::Named(_, args) => pending.extend(args),
            _ => {}
        }
    }
    false
}

/// Explain a missing inference anchor without inventing a default payload type.
fn unresolved(function: &ast::Function) -> Diagnostic {
    Diagnostic::new(
        function.span,
        format!(
            "cannot infer return type of '{}'; add a return type annotation to resolve recursive or generic dependencies",
            function.name
        ),
    )
}

impl Checker<'_> {
    /// Preserve symbolic numeric domains while inferring a private return scheme.
    pub(super) fn numeric_domain(&self, op: ast::BinaryOp, ty: &Type, span: Span) -> Checked<Type> {
        let ty = self.inference.resolve(ty, span)?;
        if op != ast::BinaryOp::Remainder {
            if ty == Type::Float {
                return Ok(Type::Float);
            }
            if self.inference.probing && matches!(ty, Type::Infer(_) | Type::Generic(_)) {
                return Ok(ty);
            }
        }
        Ok(Type::Int)
    }
}

/// Bound total work and fresh-variable storage across all dependency retries.
pub(super) fn charge(inference: &Inference, span: Span) -> Checked<()> {
    if inference.probing || inference.settling || inference.whole_signature {
        charge_work(inference, span)?;
    }
    Ok(())
}

/// Charge delayed constraints as well as expression visits to avoid quadratic unbounded work.
fn charge_work(inference: &Inference, span: Span) -> Checked<()> {
    inference.probe_work.set(inference.probe_work.get() + 1);
    if inference.probe_work.get() > MAX_EXPR_COUNT * 4
        || inference.bindings.len() > MAX_EXPR_COUNT * 4
    {
        return Err(Diagnostic::new(
            span,
            "return inference work limit exceeded; add return type annotations to dependency chains",
        ));
    }
    Ok(())
}

/// Charge expanded expression types, including nominal field projections, before retaining IR.
pub(super) fn charge_output(inference: &Inference, ty: &Type, span: Span) -> Checked<()> {
    if !inference.probing && !inference.whole_signature {
        return Ok(());
    }
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        charge_work(inference, span)?;
        match ty {
            Type::ChildKey(a) | Type::RootFunction(a) | Type::List(a) | Type::Option(a) => pending.push(a),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.push(a);
                pending.push(b);
            }
            Type::Function(args, result) => {
                pending.extend(args);
                pending.push(result);
            }
            Type::Union(args) | Type::Tuple(args) | Type::Named(_, args) => pending.extend(args),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probing_type_expansion_spends_the_shared_work_budget() {
        let inference = Inference {
            probing: true,
            probe_work: std::cell::Cell::new(MAX_EXPR_COUNT * 4 - 3),
            ..Inference::default()
        };
        let ty = Type::Tuple(vec![Type::Int; 10]);
        let error = inference.resolve(&ty, Span::default()).unwrap_err();
        assert!(error.message.contains("inference work limit"));
    }
}
