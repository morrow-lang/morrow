//! Bounded typed JSON derivation and concrete plan construction.
use super::*;
mod execute;
mod predicates;
mod profiles;
mod sums;
pub(super) use predicates::{is_json, require, retention};
mod plan;
#[cfg(test)]
use plan::Plans;
use plan::{Kind, Plan};
#[cfg(test)]
mod tests;

const WORK_LIMIT: usize = 400_000;
const PLAN_LIMIT: usize = 4096;

struct Planner<'a> {
    declarations: &'a HashMap<String, ast::TypeDecl>,
    registry: &'a nominal::Registry,
    entries: Vec<Plan>,
    cached: HashMap<Type, plan::Id>,
    work: usize,
    symbolic: bool,
}
impl<'a> Planner<'a> {
    /// Borrow declarations after ordinary source preflight and nominal validation.
    fn new(_program: &'a ast::Program, registry: &'a nominal::Registry) -> Self {
        Self {
            declarations: &registry.declarations,
            registry,
            entries: Vec::new(),
            cached: HashMap::new(),
            work: 0,
            symbolic: false,
        }
    }
    /// Charge before copying type nodes or source-owned names into a plan.
    fn charge(&mut self, amount: usize, span: Span) -> Checked<()> {
        self.work = self.work.saturating_add(amount);
        if self.work > WORK_LIMIT {
            return Err(Diagnostic::new(span, "JSON codec plan work limit exceeded"));
        }
        Ok(())
    }
    /// Validate a whole candidate before recursive hashing, cloning or cache lookup.
    fn type_work(&mut self, ty: &Type, span: Span) -> Checked<()> {
        crate::unions::bound(ty, span)?;
        if !self.symbolic {
            validate_type_structure(ty, span, true)?;
        }
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            self.charge(1, span)?;
            match ty {
                Type::Named(name, args) => {
                    self.charge(name.len(), span)?;
                    pending.extend(args);
                }
                Type::Generic(name) => self.charge(name.len(), span)?,
                Type::Union(args) | Type::Tuple(args) => pending.extend(args),
                Type::Function(args, result) => {
                    pending.extend(args);
                    pending.push(result);
                }
                Type::ChildKey(a) | Type::RootFunction(a) | Type::List(a) | Type::Option(a) => pending.push(a),
                Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                    pending.extend([a.as_ref(), b.as_ref()])
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Reserve exact type identities before traversal so regular cycles close without expansion.
    fn plan(&mut self, ty: &Type, span: Span, depth: usize) -> Checked<plan::Id> {
        self.type_work(ty, span)?;
        if let Some(id) = self.cached.get(ty) {
            return Ok(*id);
        }
        if depth >= MAX_TYPE_DEPTH || self.entries.len() >= PLAN_LIMIT {
            return Err(Diagnostic::new(
                span,
                "JSON codec type depth or plan count limit exceeded",
            ));
        }
        let id = plan::Id(self.entries.len());
        self.entries.push(Plan {
            ty: ty.clone(),
            kind: Kind::Pending,
        });
        self.cached.insert(ty.clone(), id);
        let kind = self.kind(ty, span, depth + 1)?;
        self.entries[id.0].kind = kind;
        Ok(id)
    }
    /// Pending slots never leave construction; strict cycles must admit a finite value.
    fn finite(&mut self, span: Span) -> Checked<()> {
        let mut proof =
            crate::json_codec::finite::Proof::new(self.entries.len(), &mut self.work, span)?;
        for (id, entry) in self.entries.iter().enumerate() {
            match &entry.kind {
                Kind::Pending => return Err(Diagnostic::new(span, "incomplete JSON codec plan")),
                Kind::Union(children) => {
                    proof.disjunction(id)?;
                    for child in children {
                        let product = proof.alternative(id)?;
                        proof.edge(product, child.0)?;
                    }
                }
                Kind::Sum(variants) => {
                    proof.disjunction(id)?;
                    for variant in variants {
                        let product = proof.alternative(id)?;
                        for child in &variant.fields {
                            proof.edge(product, child.0)?;
                        }
                    }
                }
                Kind::Newtype(child) => proof.edge(id, child.0)?,
                Kind::Tuple(children) => {
                    for child in children {
                        proof.edge(id, child.0)?;
                    }
                }
                Kind::Record(fields) => {
                    for field in fields {
                        proof.edge(id, field.codec.0)?;
                    }
                }
                _ => {}
            }
        }
        proof.finish()?;
        profiles::validate(&self.entries, &mut self.work, span)
    }
    /// Wire nullability is a structural property, never inferred from a current runtime value.
    fn nullable(&mut self, id: plan::Id, span: Span) -> Checked<bool> {
        if matches!(self.entries[id.0].kind, Kind::Custom { .. }) {
            return Ok(true);
        }
        // The shared representation walker charges every layer before cloning/substitution.
        let ty = self.registry.representation(&self.entries[id.0].ty, span)?;
        self.type_work(&ty, span)?;
        Ok(matches!(
            ty,
            Type::Unit | Type::Native(runtime::NativeType::JsonValue) | Type::Option(_)
        ))
    }
    /// Admit only the explicit wire domains; unsupported values never gain a fallback codec.
    fn kind(&mut self, ty: &Type, span: Span, depth: usize) -> Checked<Kind> {
        let kind = match ty {
            Type::Int => Kind::Int,
            Type::Float => Kind::Float,
            Type::Bool => Kind::Bool,
            Type::String => Kind::String,
            Type::Unit => Kind::Unit,
            Type::Native(runtime::NativeType::JsonValue) => Kind::Dynamic,
            Type::List(item) => Kind::List(self.plan(item, span, depth)?),
            Type::Option(item) => {
                let child = self.plan(item, span, depth)?;
                if self.nullable(child, span)? {
                    return Err(Diagnostic::new(
                        span,
                        "Option payload can encode null; transparent JSON Option would be ambiguous",
                    ));
                }
                Kind::Option(child)
            }
            Type::Union(items) => Kind::Union(
                items
                    .iter()
                    .map(|t| self.plan(t, span, depth))
                    .collect::<Checked<_>>()?,
            ),
            Type::Tuple(items) => Kind::Tuple(
                items
                    .iter()
                    .map(|t| self.plan(t, span, depth))
                    .collect::<Checked<_>>()?,
            ),
            Type::Map(key, item) if self.string_key(key, span)? => {
                Kind::Map(self.plan(item, span, depth)?)
            }
            Type::Map(_, _) => {
                return Err(Diagnostic::new(
                    span,
                    "JSON object keys must have type String",
                ));
            }
            Type::Named(name, _) => return self.record(ty, name, span, depth),
            Type::Generic(_) | Type::Infer(_) if self.symbolic => Kind::Parameter,
            _ => return Err(Diagnostic::new(span, unsupported(ty))),
        };
        Ok(kind)
    }
    /// Keep symbolic key identities only when String is still possible; concrete keys remain exact.
    fn string_key(&mut self, ty: &Type, span: Span) -> Checked<bool> {
        self.type_work(ty, span)?;
        match ty {
            Type::String => Ok(true),
            Type::Generic(_) | Type::Infer(_) => Ok(self.symbolic),
            _ => Ok(false),
        }
    }
    /// Resolve checked record storage while rejecting declaration cycles before child expansion.
    fn record(&mut self, ty: &Type, name: &str, span: Span, depth: usize) -> Checked<Kind> {
        if let Some((encode, decode)) = self.registry.traits.custom_json(ty, span)? {
            return Ok(Kind::Custom { encode, decode });
        }
        let declaration = self
            .declarations
            .get(name)
            .ok_or_else(|| Diagnostic::new(span, "unknown nominal JSON codec target"))?;
        if !declaration.derives.iter().any(|d| d.name == "Json") {
            return Err(Diagnostic::new(
                span,
                format!("{name} has no Json codec; add derive(Json)"),
            ));
        }
        if self.registry.is_newtype(ty) {
            self.layout_work(ty, declaration, span)?;
            let inner = self.registry.newtype_inner(ty, span)?;
            return Ok(Kind::Newtype(self.plan(&inner, span, depth)?));
        }
        if !declaration.record {
            return self.sum(ty, declaration, span, depth);
        }
        self.fields(ty, declaration, span, depth).map(Kind::Record)
    }

    /// Reserve all substituted field nodes before nominal layout allocates any copied payload.
    fn layout_work(&mut self, ty: &Type, decl: &ast::TypeDecl, span: Span) -> Checked<()> {
        let Type::Named(_, arguments) = ty else {
            return Err(Diagnostic::new(span, "codec record requires nominal type"));
        };
        if arguments.len() != decl.parameters.len() {
            return Err(Diagnostic::new(span, "wrong nominal type argument count"));
        }
        for variant in &decl.variants {
            self.charge(variant.name.len() + 1, variant.span)?;
        }
        for field in decl.variants.iter().flat_map(|v| &v.fields) {
            self.charge(field.name.as_ref().map_or(0, String::len) + 1, field.span)?;
            let mut pending = vec![&field.ty];
            while let Some(ty) = pending.pop() {
                self.charge(1, field.span)?;
                match ty {
                    Type::Generic(name) => {
                        let index =
                            decl.parameters
                                .iter()
                                .position(|p| p == name)
                                .ok_or_else(|| {
                                    Diagnostic::new(field.span, "undeclared codec type parameter")
                                })?;
                        self.type_work(&arguments[index], field.span)?;
                    }
                    Type::Named(name, args) => {
                        self.charge(name.len(), field.span)?;
                        pending.extend(args);
                    }
                    Type::Union(args) | Type::Tuple(args) => pending.extend(args),
                    Type::Function(args, result) => {
                        pending.extend(args);
                        pending.push(result);
                    }
                    Type::ChildKey(a) | Type::RootFunction(a) | Type::List(a) | Type::Option(a) => pending.push(a),
                    Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                        pending.extend([a.as_ref(), b.as_ref()])
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Keep declaration order and the exact nominal field types after parameter substitution.
    fn fields(
        &mut self,
        ty: &Type,
        decl: &ast::TypeDecl,
        span: Span,
        depth: usize,
    ) -> Checked<Vec<plan::Field>> {
        self.layout_work(ty, decl, span)?;
        let layout = self.registry.layout(ty, span)?;
        let mut fields = Vec::new();
        for (index, (name, ty)) in layout.fields.iter().zip(&layout.variants[0]).enumerate() {
            let origin = decl.variants[0].fields[index].span;
            self.charge(name.len() + 1, origin)?;
            let codec = self.plan(ty, origin, depth)?;
            fields.push(plan::Field {
                name: name.clone(),
                index,
                codec,
                optional: matches!(ty, Type::Option(_)),
            });
        }
        Ok(fields)
    }
}

/// All requested derivations are checked, including unused declarations and concrete generic fields.
pub(super) fn validate(program: &ast::Program, registry: &nominal::Registry) -> Checked<()> {
    let mut planner = Planner::new(program, registry);
    planner.symbolic = true;
    let declarations = program.types.iter().chain(
        program
            .newtypes
            .iter()
            .map(|decl| &registry.declarations[&decl.name]),
    );
    for declaration in declarations {
        if declaration.derives.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        for derive in &declaration.derives {
            if !matches!(
                derive.name.as_str(),
                "Json" | "Show" | "Eq" | "Ord" | "Clone"
            ) {
                return Err(Diagnostic::new(
                    derive.span,
                    format!("unsupported derive trait '{}'", derive.name),
                ));
            }
            if !seen.insert(&derive.name) {
                return Err(Diagnostic::new(derive.span, "duplicate derive trait"));
            }
        }
        let ty = Type::Named(
            declaration.name.clone(),
            declaration
                .parameters
                .iter()
                .cloned()
                .map(Type::Generic)
                .collect(),
        );
        if declaration.derives.iter().any(|d| d.name == "Json") {
            planner.plan(&ty, declaration.span, 0)?;
        }
    }
    planner.finite(Span::default())?;
    registry.codec_work.set(planner.work);
    Ok(())
}
/// Describe a rejected domain without leaking internal inferred or rigid generic identifiers.
fn unsupported(ty: &Type) -> &'static str {
    match ty {
        Type::Result(_, _) => "JSON codecs cannot discard or conceal an unhandled Result value",
        Type::Function(_, _) => "function values are not JSON encodable",
        Type::Infer(_) | Type::Generic(_) => {
            "JSON codec requires a concrete type in this checkpoint"
        }
        Type::Union(_) => "union JSON codecs are not supported in this checkpoint",
        Type::Range => "Range has no Json codec",
        _ => "type has no Json codec",
    }
}
