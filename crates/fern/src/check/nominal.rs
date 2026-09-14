//! Nominal declarations, concrete layouts and bounded type substitution.
use super::{Checked, Inference, MAX_TYPE_DEPTH, MAX_TYPE_NODES, returns, validate_type};
use crate::{Diagnostic, Span, Type, ast, ir};
use std::collections::{BTreeSet, HashMap, HashSet};

mod foreign;
mod newtypes;
mod sets;
pub(super) use newtypes::charge_newtype_type;

pub(super) struct Registry {
    pub(super) traits: super::traits::Registry,
    pub(super) codec_predicate_work: std::cell::Cell<usize>,
    pub(super) codec_template_work: std::cell::Cell<usize>,
    pub(super) codec_work: std::cell::Cell<usize>,
    pub(super) codec_plans: std::cell::RefCell<HashMap<Type, std::rc::Rc<crate::json_codec::Plan>>>,
    pub(super) codec_aliases: HashMap<String, (Vec<String>, Type)>,
    newtypes: HashSet<String>,
    newtype_definitions: std::rc::Rc<HashMap<String, (Vec<String>, Type)>>,
    newtype_work: std::rc::Rc<std::cell::Cell<usize>>,
    aliases: HashSet<String>,
    pub(super) declarations: HashMap<String, ast::TypeDecl>,
    constructors: HashMap<String, (String, usize)>,
}

impl Registry {
    /// Validate all forward declarations before checking individual fields.
    pub(super) fn new(program: &ast::Program) -> Checked<Self> {
        let mut result = Self {
            traits: super::traits::Registry::new(program)?,
            codec_predicate_work: std::cell::Cell::new(0),
            codec_template_work: std::cell::Cell::new(0),
            codec_work: std::cell::Cell::new(0),
            codec_plans: Default::default(),
            codec_aliases: program
                .aliases
                .iter()
                .map(|a| (a.name.clone(), (a.parameters.clone(), a.target.clone())))
                .collect(),
            newtype_definitions: std::rc::Rc::new(
                program
                    .newtypes
                    .iter()
                    .map(|d| (d.name.clone(), (d.parameters.clone(), d.inner.clone())))
                    .collect(),
            ),
            newtype_work: std::rc::Rc::new(std::cell::Cell::new(0)),
            newtypes: program
                .newtypes
                .iter()
                .map(|decl| decl.name.clone())
                .collect(),
            aliases: program
                .aliases
                .iter()
                .map(|alias| alias.name.clone())
                .collect(),
            declarations: HashMap::new(),
            constructors: HashMap::new(),
        };
        let mut declarations = program.types.clone();
        declarations.extend(
            program
                .newtypes
                .iter()
                .map(newtypes::declaration)
                .collect::<Checked<Vec<_>>>()?,
        );
        for decl in &declarations {
            if super::reserved(&decl.name)
                || result
                    .declarations
                    .insert(decl.name.clone(), decl.clone())
                    .is_some()
            {
                return Err(Diagnostic::new(
                    decl.span,
                    "duplicate or reserved type name",
                ));
            }
        }
        result.register_set();
        result.register_foreign();
        for decl in &declarations {
            result.declaration(decl)?;
        }
        result.validate_newtypes(program)?;
        Ok(result)
    }

    /// Transparent names exist only in the type namespace and introduce no constructors.
    pub(super) fn is_alias(&self, name: &str) -> bool {
        self.aliases.contains(name)
    }

    /// Check a declaration's shape, parameters, fields and constructor namespace.
    fn declaration(&mut self, decl: &ast::TypeDecl) -> Checked<()> {
        let allowed: HashSet<_> = decl.parameters.iter().cloned().collect();
        if allowed.len() != decl.parameters.len() || decl.variants.is_empty() {
            return Err(Diagnostic::new(
                decl.span,
                "duplicate type parameter or empty type declaration",
            ));
        }
        if decl.record && decl.variants.len() != 1 {
            return Err(Diagnostic::new(
                decl.span,
                "record must have exactly one constructor",
            ));
        }
        let mut variants = HashSet::new();
        for (tag, variant) in decl.variants.iter().enumerate() {
            if !variants.insert(&variant.name) {
                return Err(Diagnostic::new(variant.span, "duplicate constructor"));
            }
            let mut fields = HashSet::new();
            for field in &variant.fields {
                self.validate(&field.ty, &allowed, field.span)?;
                if let Some(name) = &field.name {
                    if !fields.insert(name) {
                        return Err(Diagnostic::new(field.span, "duplicate record field"));
                    }
                } else if decl.record {
                    return Err(Diagnostic::new(field.span, "record fields require names"));
                }
            }
            self.add_constructor(&variant.name, &decl.name, tag, variant.span)?;
            let leaf = variant.name.rsplit('.').next().unwrap_or(&variant.name);
            let qualified = format!("{}.{leaf}", decl.name);
            if qualified != variant.name {
                self.add_constructor(&qualified, &decl.name, tag, variant.span)?;
            }
            if decl.record && variant.name != decl.name {
                self.add_constructor(&decl.name, &decl.name, tag, variant.span)?;
            }
        }
        Ok(())
    }

    /// Insert one constructor alias while rejecting ambiguous or builtin names.
    fn add_constructor(&mut self, name: &str, owner: &str, tag: usize, span: Span) -> Checked<()> {
        if super::reserved(name) {
            return Err(Diagnostic::new(span, "constructor name is reserved"));
        }
        if let Some(existing) = self.constructors.get(name) {
            if existing != &(owner.to_owned(), tag) {
                return Err(Diagnostic::new(span, "ambiguous duplicate constructor"));
            }
        } else {
            self.constructors.insert(name.into(), (owner.into(), tag));
        }
        Ok(())
    }

    /// Validate nominal arities and generic names in an already bounded annotation.
    pub(super) fn validate(&self, ty: &Type, allowed: &HashSet<String>, span: Span) -> Checked<()> {
        validate_type(ty, span)?;
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            match ty {
                Type::Named(name, args) => {
                    let decl = self
                        .declarations
                        .get(name)
                        .ok_or_else(|| Diagnostic::new(span, format!("unknown type '{name}'")))?;
                    if args.len() != decl.parameters.len() {
                        return Err(Diagnostic::new(
                            span,
                            format!(
                                "type '{name}' expects {} type arguments",
                                decl.parameters.len()
                            ),
                        ));
                    }
                    if name == "Set" {
                        super::maps::validate_key(
                            &self.representation(&args[0], span)?,
                            span,
                            true,
                        )?;
                    }
                    pending.extend(args);
                }
                Type::Generic(name) if !allowed.contains(name) => {
                    return Err(Diagnostic::new(
                        span,
                        format!("undeclared generic type '{name}'"),
                    ));
                }
                Type::Function(args, result) => {
                    pending.extend(args);
                    pending.push(result);
                }
                Type::Union(args) => {
                    crate::unions::charge(&self.newtype_work, ty, span)?;
                    pending.extend(args);
                }
                Type::Tuple(args) => pending.extend(args),
                Type::Pid(t) | Type::List(t) | Type::Option(t) => pending.push(t),
                Type::Map(a, b) => {
                    super::maps::validate_key(&self.representation(a, span)?, span, true)?;
                    pending.extend([a.as_ref(), b.as_ref()]);
                }
                Type::Result(a, b) => {
                    pending.push(a);
                    pending.push(b);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Return owned constructor metadata, allowing the caller to allocate fresh variables.
    pub(super) fn constructor(
        &self,
        name: &str,
    ) -> Option<(String, Vec<String>, usize, Vec<Type>)> {
        let (owner, tag) = self.constructors.get(name)?;
        let decl = &self.declarations[owner];
        Some((
            owner.clone(),
            decl.parameters.clone(),
            *tag,
            decl.variants[*tag]
                .fields
                .iter()
                .map(|f| f.ty.clone())
                .collect(),
        ))
    }

    /// Instantiate one nominal layout without recursively expanding referenced types.
    pub(super) fn layout(&self, ty: &Type, span: Span) -> Checked<ir::TypeLayout> {
        validate_layout_type(ty, span)?;
        let Type::Named(name, args) = ty else {
            return Err(Diagnostic::new(span, "field access requires a record type"));
        };
        let decl = self
            .declarations
            .get(name)
            .ok_or_else(|| Diagnostic::new(span, format!("unknown type '{name}'")))?;
        if args.len() != decl.parameters.len() {
            return Err(Diagnostic::new(span, "wrong nominal type argument count"));
        }
        let substitutions = decl
            .parameters
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let variants = decl
            .variants
            .iter()
            .map(|v| {
                v.fields
                    .iter()
                    .map(|f| substitute(&f.ty, &substitutions))
                    .collect::<Checked<Vec<_>>>()
            })
            .collect::<Checked<Vec<_>>>()?;
        let fields = if decl.record {
            decl.variants[0]
                .fields
                .iter()
                .map(|f| f.name.clone().unwrap_or_default())
                .collect()
        } else {
            Vec::new()
        };
        Ok(ir::TypeLayout {
            storage: if self.is_newtype(ty) {
                ir::LayoutStorage::Unboxed
            } else {
                ir::LayoutStorage::Tagged
            },
            ty: ty.clone(),
            variants,
            variant_names: if decl.record || self.is_newtype(ty) {
                Vec::new()
            } else {
                decl.variants
                    .iter()
                    .map(|v| v.name.rsplit('.').next().unwrap().to_owned())
                    .collect()
            },
            fields,
        })
    }

    /// Instantiate requirement fields incrementally, charging input and expanded substitutions.
    pub(super) fn requirement_fields(
        &self,
        ty: &Type,
        inference: &super::Inference,
        span: Span,
    ) -> Checked<Vec<Type>> {
        validate_layout_type(ty, span)?;
        let Type::Named(name, args) = ty else {
            return Err(Diagnostic::new(
                span,
                "nominal requirements require a named type",
            ));
        };
        let decl = self
            .declarations
            .get(name)
            .ok_or_else(|| Diagnostic::new(span, "unknown nominal requirement type"))?;
        if args.len() != decl.parameters.len() {
            return Err(Diagnostic::new(span, "wrong nominal type argument count"));
        }
        let values = decl
            .parameters
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let mut fields = Vec::new();
        for field in decl.variants.iter().flat_map(|variant| &variant.fields) {
            super::returns::charge_output(inference, &field.ty, span)?;
            let ty = substitute(&field.ty, &values)?;
            super::returns::charge_output(inference, &ty, span)?;
            fields.push(ty);
        }
        Ok(fields)
    }

    /// Require an identified nominal record before any field-level update constraints.
    pub(super) fn record_shape(&self, ty: &Type, span: Span) -> Checked<()> {
        if matches!(ty, Type::Named(name, _) if name == "Ptr") {
            return Err(Diagnostic::new(
                span,
                "Ptr storage is private; use the Ptr APIs",
            ));
        }
        let Type::Named(name, args) = ty else {
            return Err(Diagnostic::new(span, "field access requires a record type"));
        };
        let decl = self
            .declarations
            .get(name)
            .ok_or_else(|| Diagnostic::new(span, "unknown record type"))?;
        if !decl.record || decl.parameters.len() != args.len() {
            return Err(Diagnostic::new(span, "field access requires a record type"));
        }
        Ok(())
    }

    /// Resolve only the requested field while charging names and generic substitutions.
    pub(super) fn project_field(
        &self,
        ty: &Type,
        name: &str,
        inference: &Inference,
        span: Span,
    ) -> Checked<(usize, Type)> {
        if matches!(ty, Type::Named(owner, _) if matches!(owner.as_str(), "Set" | "Ptr")) {
            return Err(Diagnostic::new(
                span,
                "collection or pointer storage is private; use its public APIs",
            ));
        }
        if self.is_newtype(ty) {
            if name != "0" {
                return Err(Diagnostic::new(span, "newtype field must be .0"));
            }
            let inner = self.newtype_inner(ty, span)?;
            returns::charge_output(inference, &inner, span)?;
            return Ok((0, inner));
        }
        self.record_shape(ty, span)?;
        let Type::Named(owner, args) = ty else {
            unreachable!("validated record shape")
        };
        let decl = &self.declarations[owner];
        for arg in args {
            returns::charge_output(inference, arg, span)?;
        }
        let substitutions = decl
            .parameters
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        for (index, field) in decl.variants[0].fields.iter().enumerate() {
            for _ in 0..field.name.as_ref().map_or(0, String::len) + 1 {
                returns::charge(inference, span)?;
            }
            if field.name.as_deref() != Some(name) {
                continue;
            }
            returns::charge_output(inference, &field.ty, span)?;
            let result = substitute(&field.ty, &substitutions)?;
            returns::charge_output(inference, &result, span)?;
            return Ok((index, result));
        }
        Err(Diagnostic::new(
            span,
            format!("unknown record field '{name}'"),
        ))
    }

    /// Resolve constructor payload types for builtin and nominal sums.
    pub(super) fn variants(&self, ty: &Type, span: Span) -> Checked<Vec<Vec<Type>>> {
        match ty {
            Type::Union(members) => Ok(members.iter().map(|ty| vec![ty.clone()]).collect()),
            Type::Tuple(fields) => Ok(vec![fields.clone()]),
            Type::Option(a) => Ok(vec![vec![(**a).clone()], vec![]]),
            Type::Result(a, b) => Ok(vec![vec![(**a).clone()], vec![(**b).clone()]]),
            Type::Named(..) => Ok(self.layout(ty, span)?.variants),
            _ => Err(Diagnostic::new(
                span,
                "constructor pattern requires a sum or record type",
            )),
        }
    }

    /// Find hidden Result values while terminating on ordinary recursive nominal types.
    pub(super) fn contains_result(&self, ty: &Type) -> Checked<bool> {
        let mut pending = vec![ty.clone()];
        let mut seen = HashSet::new();
        while let Some(ty) = pending.pop() {
            if seen.len() >= MAX_TYPE_NODES {
                return Err(Diagnostic::new(
                    Span::default(),
                    "type expansion limit exceeded",
                ));
            }
            if !seen.insert(ty.clone()) {
                continue;
            }
            match ty {
                Type::Result(..) => return Ok(true),
                Type::Map(key, value) => {
                    pending.push(*key);
                    pending.push(*value);
                }
                Type::Union(fields) | Type::Tuple(fields) => pending.extend(fields),
                Type::List(a) | Type::Option(a) => pending.push(*a),
                Type::Named(..) => pending.extend(
                    self.layout(&ty, Span::default())?
                        .variants
                        .into_iter()
                        .flatten(),
                ),
                _ => {}
            }
        }
        Ok(false)
    }

    /// Collect every concrete layout reachable from emitted function types and values.
    pub(super) fn layouts(&self, functions: &[ir::Function]) -> Checked<Vec<ir::TypeLayout>> {
        let mut pending = Vec::new();
        for function in functions {
            pending.push(function.return_type.clone());
            pending.extend(function.mailbox.iter().cloned());
            pending.extend(function.captures.iter().map(|p| p.ty.clone()));
            pending.extend(function.params.iter().map(|p| p.ty.clone()));
            let mut expressions = vec![&function.body];
            while let Some(expr) = expressions.pop() {
                pending.push(expr.ty.clone());
                expressions.extend(children(expr));
            }
        }
        let mut seen = HashSet::new();
        let mut layouts = Vec::new();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty.clone()) {
                continue;
            }
            if seen.len() > MAX_TYPE_NODES {
                return Err(Diagnostic::new(
                    Span::default(),
                    "concrete type layout expansion limit exceeded",
                ));
            }
            match &ty {
                Type::Named(_, args) => {
                    let layout = self.layout(&ty, Span::default())?;
                    pending.extend(args.iter().cloned());
                    pending.extend(layout.variants.iter().flatten().cloned());
                    layouts.push(layout);
                }
                Type::Function(args, result) => {
                    pending.extend(args.iter().cloned());
                    pending.push((**result).clone());
                }
                Type::Union(fields) | Type::Tuple(fields) => pending.extend(fields.iter().cloned()),
                Type::Pid(a) | Type::List(a) | Type::Option(a) => pending.push((**a).clone()),
                Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                    pending.push((**a).clone());
                    pending.push((**b).clone());
                }
                _ => {}
            }
        }
        Ok(layouts)
    }
}

/// Share complete typed-IR traversal with publication and executable boundary validation.
pub(super) fn children(expr: &ir::Expr) -> Vec<&ir::Expr> {
    ir::children(expr)
}

/// Collect generic parameter names in deterministic order from bounded annotations.
pub(super) fn generics(types: impl IntoIterator<Item = Type>) -> Vec<String> {
    let mut names = BTreeSet::new();
    let mut pending: Vec<_> = types.into_iter().collect();
    while let Some(ty) = pending.pop() {
        match ty {
            Type::Generic(n) => {
                names.insert(n);
            }
            Type::Function(args, result) => {
                pending.extend(args);
                pending.push(*result);
            }
            Type::Union(args) | Type::Tuple(args) | Type::Named(_, args) => pending.extend(args),
            Type::Pid(a) | Type::List(a) | Type::Option(a) => pending.push(*a),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.push(*a);
                pending.push(*b);
            }
            _ => {}
        }
    }
    names.into_iter().collect()
}

/// Substitute generic annotations with a strict bound on the expanded result.
pub(super) fn substitute(ty: &Type, values: &HashMap<String, Type>) -> Checked<Type> {
    let mut budget = MAX_TYPE_NODES;
    substitute_inner(ty, values, 0, &mut budget, true)
}

/// Charge every expanded type node, including repeated generic substitutions.
fn substitute_inner(
    ty: &Type,
    values: &HashMap<String, Type>,
    depth: usize,
    budget: &mut usize,
    expand: bool,
) -> Checked<Type> {
    if depth >= MAX_TYPE_DEPTH || *budget == 0 {
        return Err(Diagnostic::new(
            Span::default(),
            "generic type expansion limit exceeded",
        ));
    }
    *budget -= 1;
    Ok(match ty {
        Type::Generic(n) if expand => match values.get(n) {
            Some(value) => substitute_inner(value, values, depth, budget, false)?,
            None => ty.clone(),
        },
        Type::Function(args, result) => Type::Function(
            substitute_fields(args, values, depth, budget, expand)?,
            Box::new(substitute_inner(result, values, depth + 1, budget, expand)?),
        ),
        Type::Union(args) => crate::unions::make(
            substitute_fields(args, values, depth, budget, expand)?,
            Span::default(),
        )?,
        Type::Tuple(args) => Type::Tuple(substitute_fields(args, values, depth, budget, expand)?),
        Type::Named(n, args) => Type::Named(
            n.clone(),
            substitute_fields(args, values, depth, budget, expand)?,
        ),
        Type::Pid(a) => Type::Pid(Box::new(substitute_inner(
            a,
            values,
            depth + 1,
            budget,
            expand,
        )?)),
        Type::ActorFunction(a, b) => Type::ActorFunction(
            Box::new(substitute_inner(a, values, depth + 1, budget, expand)?),
            Box::new(substitute_inner(b, values, depth + 1, budget, expand)?),
        ),
        Type::List(a) => Type::List(Box::new(substitute_inner(
            a,
            values,
            depth + 1,
            budget,
            expand,
        )?)),
        Type::Option(a) => Type::Option(Box::new(substitute_inner(
            a,
            values,
            depth + 1,
            budget,
            expand,
        )?)),
        Type::Map(a, b) => Type::Map(
            Box::new(substitute_inner(a, values, depth + 1, budget, expand)?),
            Box::new(substitute_inner(b, values, depth + 1, budget, expand)?),
        ),
        Type::Result(a, b) => Type::Result(
            Box::new(substitute_inner(a, values, depth + 1, budget, expand)?),
            Box::new(substitute_inner(b, values, depth + 1, budget, expand)?),
        ),
        _ => ty.clone(),
    })
}

/// Substitute ordered fields without resetting the enclosing expansion budget.
fn substitute_fields(
    args: &[Type],
    values: &HashMap<String, Type>,
    depth: usize,
    budget: &mut usize,
    expand: bool,
) -> Checked<Vec<Type>> {
    args.iter()
        .map(|a| substitute_inner(a, values, depth + 1, budget, expand))
        .collect()
}

/// Infer a template's generic arguments from concrete argument and result types.
pub(super) fn capture(
    template: &Type,
    actual: &Type,
    values: &mut HashMap<String, Type>,
    depth: usize,
) -> Checked<()> {
    if depth >= MAX_TYPE_DEPTH {
        return Err(Diagnostic::new(
            Span::default(),
            "generic specialization type depth exceeded",
        ));
    }
    if matches!(template, Type::Union(_)) {
        return super::unions::capture(template, actual, values, depth);
    }
    match (template, actual) {
        (Type::Generic(n), ty) => {
            if let Some(previous) = values.insert(n.clone(), ty.clone())
                && previous != *ty
            {
                return Err(Diagnostic::new(
                    Span::default(),
                    "inconsistent generic specialization",
                ));
            }
        }
        (Type::Named(n, a), Type::Named(m, b)) if n == m && a.len() == b.len() => {
            for (a, b) in a.iter().zip(b) {
                capture(a, b, values, depth + 1)?;
            }
        }
        (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
            for (a, b) in a.iter().zip(b) {
                capture(a, b, values, depth + 1)?;
            }
        }
        (Type::Function(a, ar), Type::Function(b, br)) if a.len() == b.len() => {
            for (a, b) in a.iter().zip(b) {
                capture(a, b, values, depth + 1)?;
            }
            capture(ar, br, values, depth + 1)?;
        }
        (Type::Pid(a), Type::Pid(b))
        | (Type::List(a), Type::List(b))
        | (Type::Option(a), Type::Option(b)) => capture(a, b, values, depth + 1)?,
        (Type::ActorFunction(a, b), Type::ActorFunction(c, d))
        | (Type::Result(a, b), Type::Result(c, d))
        | (Type::Map(a, b), Type::Map(c, d)) => {
            capture(a, c, values, depth + 1)?;
            capture(b, d, values, depth + 1)?;
        }
        _ if template == actual => {}
        _ => {
            return Err(Diagnostic::new(
                Span::default(),
                "invalid concrete generic specialization",
            ));
        }
    }
    Ok(())
}

/// Bound concrete/inferred layout arguments before substituting recursive fields.
fn validate_layout_type(ty: &Type, span: Span) -> Checked<()> {
    let mut pending = vec![(ty, 0)];
    let mut nodes = 0;
    while let Some((ty, depth)) = pending.pop() {
        nodes += 1;
        if depth >= MAX_TYPE_DEPTH || nodes > MAX_TYPE_NODES {
            return Err(Diagnostic::new(
                span,
                "nominal type expansion limit exceeded",
            ));
        }
        match ty {
            Type::Union(args) | Type::Tuple(args) | Type::Named(_, args) => {
                pending.extend(args.iter().map(|t| (t, depth + 1)))
            }
            Type::Pid(a) | Type::List(a) | Type::Option(a) => pending.push((a, depth + 1)),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                pending.push((a, depth + 1));
                pending.push((b, depth + 1));
            }
            _ => {}
        }
    }
    Ok(())
}
