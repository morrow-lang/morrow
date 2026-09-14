//! Callee-first component inference, followed by rigid generic validation and specialization.
use super::*;

type Prepared = (ast::Program, HashMap<String, Signature>, usize);

/// Preserve the fully declared path while solving incomplete private signatures together.
pub(super) fn resolve(
    source: &ast::Program,
    registry: &nominal::Registry,
    graph: &dependencies::Graph,
) -> Checked<Prepared> {
    if source.functions.iter().all(|f| {
        f.params.iter().all(|p| p.annotation.is_some())
            && (f.return_type.is_some() || f.name == "main")
    }) {
        let parameters = parameters::resolve(source, registry)?;
        let normalized = clauses::normalize(&parameters)?;
        let (program, signatures, work) =
            returns::resolve(&normalized.program, registry, &normalized.dispatch)?;
        return Ok((program.into_owned(), signatures, work + graph.work));
    }
    let mut inference = Inference {
        probing: true,
        whole_signature: true,
        newtypes: registry.newtype_definitions(),
        newtype_work: registry.newtype_budget(),
        probe_work: std::cell::Cell::new(graph.work),
        ..Inference::default()
    };
    let (mut program, dispatch) = prepare(source, registry, graph, &mut inference)?;
    let mut signatures = signatures(&program, registry, &dispatch, &mut inference)?;
    for component in &graph.components {
        component_inference(
            &program,
            registry,
            component,
            &mut signatures,
            &mut inference,
        )?;
        shapes::finish(&mut inference, registry)?;
        for &index in component {
            publish(
                &mut program.functions[index],
                index,
                &mut signatures,
                &inference,
            )?;
        }
    }
    Ok((program, signatures, inference.probe_work.get()))
}

/// Namespace explicit universals before sharing existential variables between definitions.
fn prepare(
    source: &ast::Program,
    registry: &nominal::Registry,
    graph: &dependencies::Graph,
    inference: &mut Inference,
) -> Checked<(ast::Program, HashSet<String>)> {
    let mut program = source.clone();
    for (index, group) in graph.groups.iter().enumerate() {
        let first = &program.functions[group.clauses.start];
        if first.group_start != group.group_start || first.name != group.name {
            return Err(Diagnostic::new(
                group.span,
                "invalid dependency source identity",
            ));
        }
        rename_group(&mut program.functions[group.clauses.clone()], index)?;
        let signatures = HashMap::new();
        let mut checker = Checker {
            mailbox: None,
            editor: None,
            recovery: None,
            signatures: &signatures,
            registry,
            scopes: Vec::new(),
            local_count: 0,
            expr_count: 0,
            inference: std::mem::take(inference),
            function_return: Type::Unit,
            deferred: false,
            loop_depth: 0,
        };
        let result =
            parameters::infer_group(&program.functions[group.clauses.clone()], &mut checker);
        *inference = checker.inference;
        let params = result?;
        for function in &mut program.functions[group.clauses.clone()] {
            for (param, ty) in function.params.iter_mut().zip(&params) {
                returns::charge_output(inference, ty, param.span)?;
                param.annotation = Some(ty.clone());
            }
        }
    }
    let normalized = clauses::normalize(&program)?;
    Ok((normalized.program.into_owned(), normalized.dispatch))
}

/// Only declaration generics acquire a private owner; undeclared body annotations stay invalid.
fn rename_group(group: &mut [ast::Function], index: usize) -> Checked<()> {
    let generics = nominal::generics(group.iter().flat_map(|f| {
        f.params
            .iter()
            .filter_map(|p| p.annotation.clone())
            .chain(f.return_type.clone())
    }));
    let values = generics
        .into_iter()
        .map(|name| {
            let ty = Type::Generic(format!("$rigid{index}:{name}"));
            (name, ty)
        })
        .collect();
    for function in group {
        for bound in &mut function.constraints {
            bound.ty = nominal::substitute(&bound.ty, &values)?;
        }
        for param in &mut function.params {
            param.annotation = param
                .annotation
                .as_ref()
                .map(|t| nominal::substitute(t, &values))
                .transpose()?;
        }
        function.return_type = function
            .return_type
            .as_ref()
            .map(|t| nominal::substitute(t, &values))
            .transpose()?;
        specialize::substitute_expr(&mut function.body, &values)?;
        if let Some(guard) = &mut function.guard {
            specialize::substitute_expr(guard, &values)?;
        }
    }
    Ok(())
}

/// Internal signatures bypass only the source prohibition on inference variables.
fn signatures(
    program: &ast::Program,
    registry: &nominal::Registry,
    dispatch: &HashSet<String>,
    inference: &mut Inference,
) -> Checked<HashMap<String, Signature>> {
    let mut signatures = HashMap::new();
    for (index, function) in program.functions.iter().enumerate() {
        if reserved(&function.name) || registry.constructor(&function.name).is_some() {
            return Err(Diagnostic::new(
                function.span,
                "function name is reserved for a builtin",
            ));
        }
        validate_parameter_names(function, true)?;
        let params: Vec<_> = function
            .params
            .iter()
            .map(|p| clauses::parameter_type(p).clone())
            .collect();
        let result = returns::initial_result(function, inference)?;
        let generics = nominal::generics(params.iter().cloned().chain([result.clone()]));
        let allowed = generics.iter().cloned().collect();
        for ty in params.iter().chain([&result]) {
            if !returns::has_infer(ty) {
                registry.validate(ty, &allowed, function.span)?;
            }
        }
        signatures.insert(
            function.name.clone(),
            Signature {
                constant: function.syntax == ast::FunctionSyntax::Constant,
                mailbox: None,
                labels: labels::parameters(function),
                required_labels: Vec::new(),
                id: ir::FunctionId(index),
                params,
                result,
                generics,
                dispatch: dispatch.contains(&function.name),
                requirements: registry.traits.requirements(function)?,
                monotype: false,
            },
        );
    }
    actors::attach(program, registry, &mut signatures, inference)?;
    Ok(signatures)
}

/// Keep annotated-parameter return inference compatible; missing parameters require monotype recursion.
fn component_inference(
    program: &ast::Program,
    registry: &nominal::Registry,
    component: &[usize],
    signatures: &mut HashMap<String, Signature>,
    inference: &mut Inference,
) -> Checked<()> {
    let incomplete = component.iter().any(|&i| {
        signatures[&program.functions[i].name]
            .params
            .iter()
            .any(returns::has_infer)
    });
    if !incomplete {
        let missing: Vec<_> = component
            .iter()
            .copied()
            .filter(|&i| returns::has_infer(&signatures[&program.functions[i].name].result))
            .collect();
        return returns::solve(program, registry, signatures, inference, &missing);
    }
    for &index in component {
        let signature = signatures
            .get_mut(&program.functions[index].name)
            .expect("known component");
        signature.monotype = signature.params.iter().any(returns::has_infer)
            || returns::has_infer(&signature.result);
    }
    let mut pending = component.to_vec();
    loop {
        let revision = inference.revision;
        let mut waiting = Vec::new();
        for index in pending {
            match returns::probe(&program.functions[index], registry, signatures, inference) {
                Ok(_) => {}
                Err(error) if error.message.contains(returns::WAITING) => waiting.push(index),
                Err(error) => return Err(error),
            }
        }
        if waiting.is_empty() {
            return Ok(());
        }
        if inference.revision == revision {
            return Err(Diagnostic::new(
                program.functions[waiting[0]].span,
                "cannot infer private signature shape; add a parameter or return type annotation",
            ));
        }
        pending = waiting;
    }
}

/// Generalize one resolved signature while preserving explicit universal names and correlation.
fn publish(
    function: &mut ast::Function,
    index: usize,
    signatures: &mut HashMap<String, Signature>,
    inference: &Inference,
) -> Checked<()> {
    let signature = signatures
        .get_mut(&function.name)
        .expect("known source function");
    let params = signature
        .params
        .iter()
        .map(|p| inference.resolve(p, function.span))
        .collect::<Checked<Vec<_>>>()?;
    let result = inference.resolve(&signature.result, function.span)?;
    if matches!(result, Type::Infer(_)) && !params.iter().any(|p| contains(p, &result)) {
        return Err(Diagnostic::new(
            function.span,
            "cannot infer an unanchored recursive return type; add a return type annotation",
        ));
    }
    let mut generalized = Generalization {
        index,
        values: HashMap::new(),
        inference,
        span: function.span,
    };
    let params = params
        .iter()
        .map(|ty| generalized.ty(ty))
        .collect::<Checked<Vec<_>>>()?;
    let result = generalized.ty(&result)?;
    let mailbox = signature
        .mailbox
        .as_ref()
        .map(|ty| {
            inference
                .resolve(ty, function.span)
                .and_then(|ty| generalized.ty(&ty))
        })
        .transpose()?;
    let values = generalized
        .values
        .into_iter()
        .filter_map(|(key, value)| {
            if let Type::Generic(name) = key {
                Some((name, value))
            } else {
                None
            }
        })
        .collect();
    specialize::substitute_expr(&mut function.body, &values)?;
    for bound in &mut function.constraints {
        bound.ty = nominal::substitute(&bound.ty, &values)?;
    }
    for requirement in &mut signature.requirements {
        requirement.ty = nominal::substitute(&requirement.ty, &values)?;
    }
    for (param, ty) in function.params.iter_mut().zip(&params) {
        param.annotation = Some(ty.clone());
    }
    function.return_type = Some(result.clone());
    signature.mailbox = mailbox;
    signature.params = params;
    signature.result = result;
    signature.generics = nominal::generics(
        signature
            .params
            .iter()
            .cloned()
            .chain([signature.result.clone()])
            .chain(signature.mailbox.clone()),
    );
    signature.monotype = false;
    Ok(())
}

/// Search a resolved input type without treating a capability-only variable as a return anchor.
fn contains(ty: &Type, needle: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if ty == needle {
            return true;
        }
        match ty {
            Type::Function(args, result) => {
                pending.extend(args);
                pending.push(result);
            }
            Type::Union(args) | Type::Tuple(args) | Type::Named(_, args) => pending.extend(args),
            Type::List(a) | Type::Option(a) => pending.push(a),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.extend([a.as_ref(), b.as_ref()])
            }
            _ => {}
        }
    }
    false
}

/// One signature shares quantified identities across every input and its result.
struct Generalization<'a> {
    index: usize,
    values: HashMap<Type, Type>,
    inference: &'a Inference,
    span: Span,
}

impl Generalization<'_> {
    /// Give unresolved roots deterministic quantified names without consulting callers.
    fn ty(&mut self, ty: &Type) -> Checked<Type> {
        returns::charge(self.inference, self.span)?;
        Ok(match ty {
            Type::Infer(_) | Type::Generic(_) => self.variable(ty),
            Type::List(a) => Type::List(Box::new(self.ty(a)?)),
            Type::Option(a) => Type::Option(Box::new(self.ty(a)?)),
            Type::Result(a, b) => Type::Result(Box::new(self.ty(a)?), Box::new(self.ty(b)?)),
            Type::Map(a, b) => Type::Map(Box::new(self.ty(a)?), Box::new(self.ty(b)?)),
            Type::Union(args) => crate::unions::make(self.types(args)?, Span::default())?,
            Type::Tuple(args) => Type::Tuple(self.types(args)?),
            Type::Named(name, args) => Type::Named(name.clone(), self.types(args)?),
            Type::Function(args, result) => {
                Type::Function(self.types(args)?, Box::new(self.ty(result)?))
            }
            _ => ty.clone(),
        })
    }

    /// Preserve repeated variable identities and restore only this declaration's rigid names.
    fn variable(&mut self, ty: &Type) -> Type {
        let index = self.index;
        let next = self.values.len();
        self.values
            .entry(ty.clone())
            .or_insert_with(|| {
                if let Type::Generic(name) = ty
                    && let Some(name) = name.strip_prefix(&format!("$rigid{index}:"))
                {
                    return Type::Generic(name.into());
                }
                Type::Generic(format!("$inferred{index}_{next}"))
            })
            .clone()
    }

    /// Traverse compound parameters with the same resource and variable identity state.
    fn types(&mut self, args: &[Type]) -> Checked<Vec<Type>> {
        args.iter().map(|ty| self.ty(ty)).collect()
    }
}

#[cfg(test)]
mod tests;
