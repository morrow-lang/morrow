//! Coherent static traits share generic requirement propagation and monomorphization.
use super::*;
mod derive;
pub(super) use derive::expand;

#[derive(Default)]
pub(super) struct Registry {
    declarations: Vec<ast::TraitDecl>,
    names: HashMap<String, usize>,
    implementations: Vec<ast::Implementation>,
    pub(super) methods: HashMap<String, (usize, String)>,
    work: std::cell::Cell<usize>,
    resolving: std::cell::RefCell<HashSet<(usize, Type)>>,
    source_signatures: HashMap<String, (Vec<Type>, Type)>,
    inferred: std::cell::RefCell<HashMap<String, Vec<schemes::Requirement>>>,
}

impl Registry {
    pub(super) fn name(&self, id: usize) -> Option<&str> {
        self.declarations.get(id).map(|d| d.name.as_str())
    }
    fn charge(&self, amount: usize, span: Span) -> Checked<()> {
        let work = self.work.get().saturating_add(amount);
        if work > 400_000 {
            return Err(Diagnostic::new(
                span,
                "trait resolution work limit exceeded",
            ));
        }
        self.work.set(work);
        Ok(())
    }
    pub(super) fn new(program: &ast::Program) -> Checked<Self> {
        let mut registry = Self::default();
        for declaration in &program.traits {
            if registry
                .names
                .insert(declaration.name.clone(), registry.declarations.len())
                .is_some()
            {
                return Err(Diagnostic::new(
                    declaration.span,
                    "duplicate trait declaration",
                ));
            }
            let id = registry.declarations.len();
            let mut methods = HashSet::new();
            for method in &declaration.methods {
                if !methods.insert(&method.name)
                    || registry
                        .methods
                        .insert(method.function.clone(), (id, method.name.clone()))
                        .is_some()
                {
                    return Err(Diagnostic::new(declaration.span, "duplicate trait method"));
                }
            }
            registry.declarations.push(declaration.clone());
        }
        registry.implementations = program.implementations.clone();
        for implementation in &registry.implementations {
            if implementation.bound.name == "Json"
                && !matches!(implementation.bound.ty, Type::Named(..))
            {
                return Err(Diagnostic::new(
                    implementation.span,
                    "custom Json implementations require a nominal type",
                ));
            }
        }
        for function in &program.functions {
            if let Some(result) = &function.return_type
                && function.params.iter().all(|p| p.annotation.is_some())
            {
                registry.source_signatures.insert(
                    function.name.clone(),
                    (
                        function
                            .params
                            .iter()
                            .filter_map(|p| p.annotation.clone())
                            .collect(),
                        result.clone(),
                    ),
                );
            }
        }
        Ok(registry)
    }

    /// Publish each checked method's complete conditional requirements in its source type names.
    /// This includes inferred intrinsic requirements, not just the written where clause.
    pub(super) fn complete_requirements(
        &self,
        signatures: &HashMap<String, Signature>,
    ) -> Checked<()> {
        let names: HashSet<_> = self
            .implementations
            .iter()
            .flat_map(|i| i.methods.iter().map(|(_, f)| f))
            .chain(
                self.declarations
                    .iter()
                    .flat_map(|d| &d.methods)
                    .filter_map(|m| m.default.as_ref()),
            )
            .collect();
        let mut inferred = HashMap::new();
        for name in names {
            let signature = &signatures[name];
            let Some((params, result)) = self.source_signatures.get(name) else {
                continue;
            };
            let mut values = HashMap::new();
            unions::capture_pairs(
                &signature
                    .params
                    .iter()
                    .zip(params)
                    .chain([(&signature.result, result)])
                    .collect::<Vec<_>>(),
                &mut values,
            )?;
            let requirements = signature
                .requirements
                .iter()
                .map(|r| {
                    self.charge(1, r.span)?;
                    Ok(schemes::Requirement {
                        capability: r.capability,
                        ty: nominal::substitute(&r.ty, &values)?,
                        span: r.span,
                    })
                })
                .collect::<Checked<Vec<_>>>()?;
            inferred.insert(name.clone(), requirements);
        }
        *self.inferred.borrow_mut() = inferred;
        Ok(())
    }

    pub(super) fn requirements(
        &self,
        function: &ast::Function,
    ) -> Checked<Vec<schemes::Requirement>> {
        let allowed: HashSet<_> = nominal::generics(
            function
                .params
                .iter()
                .filter_map(|p| p.annotation.clone())
                .chain(function.return_type.clone()),
        )
        .into_iter()
        .collect();
        let mut seen = HashSet::new();
        function
            .constraints
            .iter()
            .map(|bound| {
                let id = *self.names.get(&bound.name).ok_or_else(|| {
                    Diagnostic::new(bound.span, format!("unknown trait '{}'", bound.name))
                })?;
                if !seen.insert((id, bound.ty.clone())) {
                    return Err(Diagnostic::new(bound.span, "duplicate trait constraint"));
                }
                if nominal::generics([bound.ty.clone()])
                    .iter()
                    .any(|name| !allowed.contains(name))
                {
                    return Err(Diagnostic::new(
                        bound.span,
                        "trait constraint contains an undeclared type parameter",
                    ));
                }
                Ok(schemes::Requirement {
                    capability: schemes::Capability::Trait(id),
                    ty: bound.ty.clone(),
                    span: bound.span,
                })
            })
            .collect()
    }

    pub(super) fn validate(
        &self,
        program: &ast::Program,
        registry: &nominal::Registry,
    ) -> Checked<()> {
        let functions: HashMap<_, _> = program
            .functions
            .iter()
            .map(|f| (f.name.as_str(), f))
            .collect();
        let mut clauses: HashMap<&str, Vec<&ast::Function>> = HashMap::new();
        for function in &program.functions {
            self.charge(1, function.span)?;
            clauses.entry(&function.name).or_default().push(function);
        }
        for declaration in &self.declarations {
            self.charge(1, declaration.span)?;
            if declaration.methods.is_empty() {
                return Err(Diagnostic::new(
                    declaration.span,
                    "trait requires at least one method",
                ));
            }
            for parent in &declaration.parents {
                if !self.names.contains_key(&parent.name) {
                    return Err(Diagnostic::new(parent.span, "unknown parent trait"));
                }
                if nominal::generics([parent.ty.clone()])
                    .iter()
                    .any(|p| p != &declaration.parameter)
                {
                    return Err(Diagnostic::new(
                        parent.span,
                        "undeclared parent trait parameter",
                    ));
                }
            }
            self.parents(
                *self.names.get(&declaration.name).expect("registered trait"),
                &mut HashSet::new(),
                0,
            )?;
            for method in &declaration.methods {
                let function = functions.get(method.function.as_str()).ok_or_else(|| {
                    Diagnostic::new(declaration.span, "missing trait method signature")
                })?;
                if function.syntax != ast::FunctionSyntax::Trait
                    || !nominal::generics(
                        function
                            .params
                            .iter()
                            .filter_map(|p| p.annotation.clone())
                            .chain(function.return_type.clone()),
                    )
                    .contains(&declaration.parameter)
                {
                    return Err(Diagnostic::new(
                        function.span,
                        "trait method signature must mention its trait parameter",
                    ));
                }
            }
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            self.charge(1, implementation.span)?;
            let id = *self.names.get(&implementation.bound.name).ok_or_else(|| {
                Diagnostic::new(implementation.span, "implementation names an unknown trait")
            })?;
            let declaration = &self.declarations[id];
            if let Some((_, function)) = implementation.methods.first() {
                let owner = function.rsplit_once('.').map(|(owner, _)| owner);
                let trait_owner = declaration.name.rsplit_once('.').map(|(owner, _)| owner);
                let type_owner = if let Type::Named(name, _) = &implementation.bound.ty {
                    name.rsplit_once('.').map(|(owner, _)| owner)
                } else {
                    None
                };
                if owner.is_some() && owner != trait_owner && owner != type_owner {
                    return Err(Diagnostic::new(
                        implementation.span,
                        "an implementation must belong to the module defining its trait or type",
                    ));
                }
            }
            let allowed = nominal::generics([implementation.bound.ty.clone()])
                .into_iter()
                .collect();
            registry.validate(&implementation.bound.ty, &allowed, implementation.span)?;
            for previous in &self.implementations[..index] {
                self.charge(1, implementation.span)?;
                if previous.bound.name == implementation.bound.name
                    && overlaps(&previous.bound.ty, &implementation.bound.ty)?
                {
                    return Err(Diagnostic::new(
                        implementation.span,
                        format!(
                            "overlapping implementation of {}",
                            implementation.bound.name
                        ),
                    ));
                }
            }
            let substitutions = HashMap::from([(
                declaration.parameter.clone(),
                implementation.bound.ty.clone(),
            )]);
            let mut seen = HashSet::new();
            for (name, target) in &implementation.methods {
                if !seen.insert(name) {
                    return Err(Diagnostic::new(
                        implementation.span,
                        "duplicate implementation method",
                    ));
                }
                let method = declaration
                    .methods
                    .iter()
                    .find(|m| &m.name == name)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            implementation.span,
                            format!("unknown method '{name}' for {}", declaration.name),
                        )
                    })?;
                let expected = functions[method.function.as_str()];
                let actuals = clauses.get(target.as_str()).ok_or_else(|| {
                    Diagnostic::new(implementation.span, "missing implementation function")
                })?;
                for actual in actuals {
                    self.charge(1, actual.span)?;
                    if expected.params.len() != actual.params.len()
                        || actual.return_type.is_none()
                        || actual.params.iter().any(|p| p.annotation.is_none())
                    {
                        return Err(Diagnostic::new(
                            actual.span,
                            "implementation method requires the complete trait signature",
                        ));
                    }
                    for (expected, actual) in expected
                        .params
                        .iter()
                        .filter_map(|p| p.annotation.as_ref())
                        .chain(expected.return_type.as_ref())
                        .zip(
                            actual
                                .params
                                .iter()
                                .filter_map(|p| p.annotation.as_ref())
                                .chain(actual.return_type.as_ref()),
                        )
                    {
                        if nominal::substitute(expected, &substitutions)? != *actual {
                            return Err(Diagnostic::new(
                                implementation.span,
                                format!("method '{name}' does not match its trait signature"),
                            ));
                        }
                    }
                }
            }
            for method in &declaration.methods {
                if !seen.contains(&method.name) && method.default.is_none() {
                    return Err(Diagnostic::new(
                        implementation.span,
                        format!("implementation is missing method '{}'", method.name),
                    ));
                }
            }
            if nominal::generics([implementation.bound.ty.clone()]).is_empty() {
                self.require(
                    id,
                    &implementation.bound.ty,
                    &Inference::default(),
                    implementation.span,
                    registry,
                )?;
            }
        }
        Ok(())
    }

    fn parents(&self, id: usize, active: &mut HashSet<usize>, depth: usize) -> Checked<()> {
        let declaration = &self.declarations[id];
        self.charge(1, declaration.span)?;
        if depth >= 64 || !active.insert(id) {
            return Err(Diagnostic::new(
                declaration.span,
                "cyclic or excessively deep trait parents",
            ));
        }
        for parent in &declaration.parents {
            self.parents(self.names[&parent.name], active, depth + 1)?;
        }
        active.remove(&id);
        Ok(())
    }

    fn implementation(
        &self,
        id: usize,
        ty: &Type,
        span: Span,
    ) -> Checked<(&ast::Implementation, HashMap<String, Type>)> {
        for implementation in &self.implementations {
            self.charge(1, span)?;
            if implementation.bound.name != self.declarations[id].name {
                continue;
            }
            let mut values = HashMap::new();
            if unions::capture_pairs(&[(&implementation.bound.ty, ty)], &mut values).is_ok() {
                return Ok((implementation, values));
            }
        }
        Err(Diagnostic::new(
            span,
            format!(
                "type has no implementation of {}",
                self.declarations[id].name
            ),
        ))
    }

    pub(super) fn require(
        &self,
        id: usize,
        ty: &Type,
        inference: &Inference,
        span: Span,
        registry: &nominal::Registry,
    ) -> Checked<()> {
        if self.declarations[id].name == "Json" && self.custom_json(ty, span)?.is_none() {
            return codecs::require(inference, registry, schemes::Capability::Json, ty, span);
        }
        self.require_depth(id, ty, inference, span, 0, registry)?;
        let ty = inference.resolve(ty, span)?;
        if returns::has_infer(&ty) || !nominal::generics([ty.clone()]).is_empty() {
            return Ok(());
        }
        let active = (id, ty.clone());
        if !self.resolving.borrow_mut().insert(active.clone()) {
            return Ok(());
        }
        let result = (|| {
            let mut pending = vec![(id, ty)];
            let mut seen = HashSet::new();
            while let Some((id, ty)) = pending.pop() {
                self.charge(1, span)?;
                if !seen.insert((id, ty.clone())) {
                    continue;
                }
                self.require_depth(id, &ty, inference, span, 0, registry)?;
                if self.declarations[id].name == "Json" && self.custom_json(&ty, span)?.is_none() {
                    continue;
                }
                let (implementation, values) = self.implementation(id, &ty, span)?;
                for method in &self.declarations[id].methods {
                    let (name, values) = if let Some((_, name)) = implementation
                        .methods
                        .iter()
                        .find(|(name, _)| name == &method.name)
                    {
                        (name, values.clone())
                    } else if let Some(default) = &method.default {
                        (
                            default,
                            HashMap::from([(self.declarations[id].parameter.clone(), ty.clone())]),
                        )
                    } else {
                        continue;
                    };
                    let inferred = self.inferred.borrow();
                    for requirement in inferred.get(name).into_iter().flatten() {
                        let ty = nominal::substitute(&requirement.ty, &values)?;
                        if let schemes::Capability::Trait(id) = requirement.capability {
                            pending.push((id, ty));
                        } else if codecs::is_json(requirement.capability) {
                            codecs::require(
                                inference,
                                registry,
                                requirement.capability,
                                &ty,
                                span,
                            )?;
                        } else {
                            inference.require(requirement.capability, &ty, span)?;
                        }
                    }
                }
            }
            Ok(())
        })();
        self.resolving.borrow_mut().remove(&active);
        result
    }

    fn require_depth(
        &self,
        id: usize,
        ty: &Type,
        inference: &Inference,
        span: Span,
        depth: usize,
        registry: &nominal::Registry,
    ) -> Checked<()> {
        returns::charge(inference, span)?;
        if self.declarations[id].name == "Json" && self.custom_json(ty, span)?.is_none() {
            return codecs::require(inference, registry, schemes::Capability::Json, ty, span);
        }
        if depth >= 64 {
            return Err(Diagnostic::new(
                span,
                "trait resolution depth limit exceeded",
            ));
        }
        let ty = inference.resolve(ty, span)?;
        if (inference.template || inference.whole_signature)
            && (returns::has_infer(&ty) || !nominal::generics([ty.clone()]).is_empty())
        {
            if !matches!(ty, Type::Generic(_) | Type::Infer(_)) {
                self.implementation(id, &ty, span)?;
            }
            schemes::retain_requirement(
                &mut inference.requirements.borrow_mut(),
                schemes::Requirement {
                    capability: schemes::Capability::Trait(id),
                    ty,
                    span,
                },
                inference,
            )?;
            return Ok(());
        }
        let (implementation, values) = self.implementation(id, &ty, span)?;
        for bound in &implementation.constraints {
            let target = *self
                .names
                .get(&bound.name)
                .ok_or_else(|| Diagnostic::new(bound.span, "unknown implementation constraint"))?;
            self.require_depth(
                target,
                &nominal::substitute(&bound.ty, &values)?,
                inference,
                span,
                depth + 1,
                registry,
            )?;
        }
        let declaration = &self.declarations[id];
        let values = HashMap::from([(declaration.parameter.clone(), ty)]);
        for parent in &declaration.parents {
            self.require_depth(
                self.names[&parent.name],
                &nominal::substitute(&parent.ty, &values)?,
                inference,
                span,
                depth + 1,
                registry,
            )?;
        }
        Ok(())
    }

    /// Authored Json implementations override structural codecs only for nominal targets.
    pub(super) fn custom_json(&self, ty: &Type, span: Span) -> Checked<Option<(String, String)>> {
        if !matches!(ty, Type::Named(..)) {
            return Ok(None);
        }
        if !self.names.contains_key("Json") {
            return Ok(None);
        }
        for implementation in &self.implementations {
            self.charge(1, span)?;
            if implementation.bound.name != "Json" {
                continue;
            }
            if unions::capture_pairs(&[(&implementation.bound.ty, ty)], &mut HashMap::new())
                .is_err()
            {
                continue;
            }
            let method = |name: &str| {
                implementation
                    .methods
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, f)| f.clone())
                    .ok_or_else(|| {
                        Diagnostic::new(span, "custom Json requires to_json and from_json")
                    })
            };

            return Ok(Some((method("to_json")?, method("from_json")?)));
        }
        Ok(None)
    }
    pub(super) fn require_custom_json(
        &self,
        ty: &Type,
        inference: &Inference,
        registry: &nominal::Registry,
        span: Span,
    ) -> Checked<()> {
        let id = *self
            .names
            .get("Json")
            .ok_or_else(|| Diagnostic::new(span, "missing Json trait"))?;
        self.require(id, ty, inference, span, registry)
    }

    pub(super) fn target(
        &self,
        name: &str,
        signature: &Signature,
        params: &[Type],
        result: &Type,
        span: Span,
    ) -> Checked<Option<String>> {
        let Some((id, method)) = self.methods.get(name) else {
            return Ok(None);
        };
        let mut values = HashMap::new();
        unions::capture_pairs(
            &signature
                .params
                .iter()
                .zip(params)
                .chain([(&signature.result, result)])
                .collect::<Vec<_>>(),
            &mut values,
        )?;
        let bound = signature
            .requirements
            .iter()
            .find(|r| r.capability == schemes::Capability::Trait(*id))
            .ok_or_else(|| Diagnostic::new(span, "missing trait method requirement"))?;
        let ty = nominal::substitute(&bound.ty, &values)?;
        if self.declarations[*id].name == "Json" && self.custom_json(&ty, span)?.is_none() {
            return Ok(Some(
                match method.as_str() {
                    "to_json" => "$traits_derived_json_to",
                    "from_json" => "$traits_derived_json_from",
                    _ => return Err(Diagnostic::new(span, "unknown Json trait method")),
                }
                .into(),
            ));
        }
        let (implementation, _) = self.implementation(*id, &ty, span)?;
        let target = implementation
            .methods
            .iter()
            .find(|(n, _)| n == method)
            .map(|(_, f)| f.clone())
            .or_else(|| {
                self.declarations[*id]
                    .methods
                    .iter()
                    .find(|m| &m.name == method)
                    .and_then(|m| m.default.clone())
            })
            .ok_or_else(|| Diagnostic::new(span, "missing trait implementation method"))?;
        Ok(Some(target))
    }
}

fn overlaps(left: &Type, right: &Type) -> Checked<bool> {
    let mut inference = Inference::default();
    let mut instantiate = |ty: &Type| -> Checked<Type> {
        let values = nominal::generics([ty.clone()])
            .into_iter()
            .map(|n| (n, inference.fresh()))
            .collect();
        nominal::substitute(ty, &values)
    };
    let left = instantiate(left)?;
    let right = instantiate(right)?;
    Ok(inference
        .unify(&left, &right, Span::default(), "trait coherence")
        .is_ok())
}
