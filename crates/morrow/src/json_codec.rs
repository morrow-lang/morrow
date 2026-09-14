//! Concrete, finite JSON wire plans shared by native and interactive execution.
use crate::{Diagnostic, Span, Type, ir, runtime::NativeType};
use std::collections::HashMap;

pub(crate) mod finite;
mod profile_plan;
pub(crate) mod profiles;
pub(crate) mod sums;

const MAX_ENTRIES: usize = 4096;
const MAX_WORK: usize = 400_000;

/// Field position is both a checked native storage slot and a source wire name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub index: usize,
    pub codec: usize,
    pub optional: bool,
}
/// Source tag spelling is checked independently against nominal layout metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub wire_tag: String,
    pub fields: Vec<usize>,
}
/// Child indices address a validated finite graph; symbolic parameters have no wire form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Bool,
    String,
    Unit,
    Dynamic,
    Custom { encode: Callback, decode: Callback },
    List(usize),
    Newtype(usize),
    Option(usize),
    Tuple(Vec<usize>),
    Map(usize),
    Record(Vec<Field>),
    Sum(Vec<Variant>),
    Union(Vec<usize>),
}
/// Source callbacks exist only during checking; executable plans carry validated function IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callback {
    Source(String),
    Function(ir::FunctionId),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub ty: Type,
    pub kind: Kind,
}
/// Public callers must validate every entry, including entries not reachable from the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub root: usize,
    pub entries: Vec<Entry>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Encode,
    Decode,
}

struct Validation {
    work: usize,
    span: Span,
}
impl Validation {
    /// Reserve all upcoming units before traversal, copying, lookup or allocation.
    fn charge(&mut self, amount: usize) -> Result<(), Diagnostic> {
        self.work = self.work.saturating_add(amount);
        if self.work > MAX_WORK {
            return Err(self.error("JSON codec plan work limit exceeded"));
        }
        Ok(())
    }
    fn error(&self, message: &str) -> Diagnostic {
        Diagnostic::new(self.span, message)
    }
    /// Audit borrowed types before hashing or recursive equality and bound the traversal queue.
    fn ty(&mut self, ty: &Type) -> Result<(), Diagnostic> {
        crate::unions::bound(ty, self.span)?;
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            self.charge(1)?;
            let children = match ty {
                Type::Named(name, args) => {
                    self.charge(name.len())?;
                    args.as_slice()
                }
                Type::Tuple(args) | Type::Union(args) => args.as_slice(),
                Type::List(a) | Type::Option(a) => std::slice::from_ref(a.as_ref()),
                Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                    self.charge(2)?;
                    pending.extend([a.as_ref(), b.as_ref()]);
                    continue;
                }
                Type::Function(args, result) => {
                    self.charge(args.len() + 1)?;
                    pending.extend(args);
                    pending.push(result);
                    continue;
                }
                Type::Int
                | Type::Float
                | Type::Bool
                | Type::String
                | Type::Unit
                | Type::Range
                | Type::Native(_) => &[],
                _ => {
                    return Err(
                        self.error("JSON codec plan contains a nonconcrete or unsupported type")
                    );
                }
            };
            self.charge(children.len())?;
            pending.extend(children);
        }
        Ok(())
    }
    /// Include unused storage and metadata in the same aggregate accounting pass.
    fn bounds(&mut self, plan: &Plan, layouts: &[ir::TypeLayout]) -> Result<(), Diagnostic> {
        if plan.entries.is_empty()
            || plan.entries.len() > MAX_ENTRIES
            || plan.root >= plan.entries.len()
            || layouts.len() > MAX_ENTRIES
        {
            return Err(self.error("invalid JSON codec plan size or root"));
        }
        for entry in &plan.entries {
            self.ty(&entry.ty)?;
            match &entry.kind {
                Kind::Tuple(children) | Kind::Union(children) => self.charge(children.len())?,
                Kind::Sum(variants) => {
                    self.charge(variants.len())?;
                    for variant in variants {
                        self.charge(variant.wire_tag.len() + variant.fields.len())?;
                    }
                }
                Kind::Record(fields) => {
                    self.charge(fields.len())?;
                    for field in fields {
                        self.charge(field.name.len())?;
                    }
                }
                _ => self.charge(1)?,
            }
        }
        // Unrelated nominal layouts may contain other legitimate language types.
        for layout in layouts {
            crate::unions::bound(&layout.ty, self.span)?;
            self.charge(type_size(&layout.ty))?;
            self.charge(layout.fields.len() + layout.variants.len())?;
            self.charge(layout.variant_names.len())?;
            for name in layout.fields.iter().chain(&layout.variant_names) {
                self.charge(name.len())?;
            }
            for fields in &layout.variants {
                self.charge(fields.len())?;
                for ty in fields {
                    crate::unions::bound(ty, self.span)?;
                    self.charge(type_size(ty))?;
                }
            }
        }
        Ok(())
    }
}
impl Plan {
    /// Validate concrete types, all child references, nullability and exact nominal storage.
    /// No preconditions: malformed public plans return a diagnostic without indexing unchecked slots.
    pub fn validate(&self, layouts: &[ir::TypeLayout], span: Span) -> Result<(), Diagnostic> {
        self.validate_budget(layouts, span, &mut 0)
    }
    /// Continue a single executable-program allowance across distinct concrete graphs.
    fn validate_budget(
        &self,
        layouts: &[ir::TypeLayout],
        span: Span,
        work: &mut usize,
    ) -> Result<(), Diagnostic> {
        let mut audit = Validation { work: *work, span };
        let result = self.validate_inner(layouts, &mut audit);
        *work = audit.work;
        result
    }
    fn validate_inner(
        &self,
        layouts: &[ir::TypeLayout],
        audit: &mut Validation,
    ) -> Result<(), Diagnostic> {
        audit.bounds(self, layouts)?;
        let mut storage = HashMap::new();
        for layout in layouts {
            if storage.insert(&layout.ty, layout).is_some() {
                return Err(audit.error("duplicate JSON codec nominal storage identity"));
            }
        }
        for (index, entry) in self.entries.iter().enumerate() {
            self.entry(index, entry, &storage, audit)?;
        }
        self.finite(audit)
    }
    /// All strict components must reach finite constructors, including inactive components.
    fn finite(&self, audit: &mut Validation) -> Result<(), Diagnostic> {
        let mut proof = finite::Proof::new(self.entries.len(), &mut audit.work, audit.span)?;
        for (id, entry) in self.entries.iter().enumerate() {
            match &entry.kind {
                Kind::Union(children) => {
                    proof.disjunction(id)?;
                    for child in children {
                        let product = proof.alternative(id)?;
                        proof.edge(product, *child)?;
                    }
                }
                Kind::Sum(variants) => {
                    proof.disjunction(id)?;
                    for variant in variants {
                        let product = proof.alternative(id)?;
                        for child in &variant.fields {
                            proof.edge(product, *child)?;
                        }
                    }
                }
                Kind::Newtype(child) => proof.edge(id, *child)?,
                Kind::Tuple(children) => {
                    for child in children {
                        proof.edge(id, *child)?;
                    }
                }
                Kind::Record(fields) => {
                    for field in fields {
                        proof.edge(id, field.codec)?;
                    }
                }
                _ => {}
            }
        }
        proof.finish()?;
        self.profiles(audit)
    }
    /// Exact indexed references permit regular cycles without trusting source derivations.
    fn child(
        &self,
        child: usize,
        _parent: usize,
        audit: &mut Validation,
    ) -> Result<&Entry, Diagnostic> {
        let entry = self
            .entries
            .get(child)
            .ok_or_else(|| audit.error("invalid JSON codec child slot"))?;
        audit.charge(type_size(&entry.ty))?;
        Ok(entry)
    }
    /// Compare semantic types only after the complete borrowed graph was bounded.
    fn entry(
        &self,
        index: usize,
        entry: &Entry,
        layouts: &HashMap<&Type, &ir::TypeLayout>,
        audit: &mut Validation,
    ) -> Result<(), Diagnostic> {
        let valid = match (&entry.kind, &entry.ty) {
            (Kind::Int, Type::Int)
            | (Kind::Float, Type::Float)
            | (Kind::Bool, Type::Bool)
            | (Kind::String, Type::String)
            | (Kind::Unit, Type::Unit)
            | (Kind::Dynamic, Type::Native(NativeType::JsonValue)) => true,
            (Kind::List(id), Type::List(ty)) => self.child(*id, index, audit)?.ty == **ty,
            (Kind::Option(id), Type::Option(ty)) => {
                let child = self.child(*id, index, audit)?;
                child.ty == **ty
            }
            (Kind::Map(id), Type::Map(key, ty)) => {
                **key == Type::String && self.child(*id, index, audit)?.ty == **ty
            }
            (Kind::Union(ids), Type::Union(types)) => {
                self.union_members(index, ids, types, audit)?
            }
            (Kind::Tuple(ids), Type::Tuple(types)) => {
                if ids.len() != types.len() {
                    false
                } else {
                    let mut same = true;
                    for (id, ty) in ids.iter().zip(types) {
                        same &= self.child(*id, index, audit)?.ty == *ty;
                    }
                    same
                }
            }
            (Kind::Sum(variants), Type::Named(..)) => {
                self.sum(index, variants, entry, layouts, audit)?
            }
            (Kind::Newtype(child), Type::Named(..)) => {
                self.newtype(index, *child, entry, layouts, audit)?
            }
            (Kind::Custom { .. }, Type::Named(..)) => layouts.contains_key(&entry.ty),
            (Kind::Record(fields), Type::Named(..)) => {
                self.record(index, fields, entry, layouts, audit)?
            }
            _ => false,
        };
        if !valid {
            return Err(audit.error("JSON codec kind does not match its concrete storage type"));
        }
        Ok(())
    }
    /// Follow unboxed wire identity without accepting a forged tagged or mismatched layout.
    fn newtype(
        &self,
        index: usize,
        child: usize,
        entry: &Entry,
        layouts: &HashMap<&Type, &ir::TypeLayout>,
        audit: &mut Validation,
    ) -> Result<bool, Diagnostic> {
        let child = self.child(child, index, audit)?;
        let layout = layouts
            .get(&entry.ty)
            .ok_or_else(|| audit.error("JSON newtype codec is missing its checked layout"))?;
        Ok(layout.storage == ir::LayoutStorage::Unboxed
            && layout.fields.is_empty()
            && layout.variant_names.is_empty()
            && layout.variants.len() == 1
            && layout.variants[0].len() == 1
            && layout.variants[0][0] == child.ty)
    }
    /// Every field must name its exact tagged-record slot; aliases and unboxed newtypes cannot fabricate it.
    fn record(
        &self,
        index: usize,
        fields: &[Field],
        entry: &Entry,
        layouts: &HashMap<&Type, &ir::TypeLayout>,
        audit: &mut Validation,
    ) -> Result<bool, Diagnostic> {
        let Some(layout) = layouts.get(&entry.ty) else {
            return Err(audit.error("JSON codec record is missing its checked layout"));
        };
        if layout.storage != ir::LayoutStorage::Tagged
            || !layout.variant_names.is_empty()
            || layout.variants.len() != 1
            || layout.fields.len() != fields.len()
            || layout.variants[0].len() != fields.len()
        {
            return Ok(false);
        }
        let mut names = std::collections::HashSet::new();
        for (position, field) in fields.iter().enumerate() {
            let child = self.child(field.codec, index, audit)?;
            if field.name.contains('\0')
                || field.name.is_empty()
                || field.index != position
                || field.name != layout.fields[position]
                || !names.insert(&field.name)
                || child.ty != layout.variants[0][position]
                || field.optional != matches!(child.ty, Type::Option(_))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
/// Count a previously bounded type including names before native-layout indexing or equality.
fn type_size(ty: &Type) -> usize {
    let mut total = 0usize;
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        total = total.saturating_add(1);
        match ty {
            Type::Named(name, args) => {
                total = total.saturating_add(name.len());
                pending.extend(args);
            }
            Type::Generic(name) => total = total.saturating_add(name.len()),
            Type::Union(args) | Type::Tuple(args) => pending.extend(args),
            Type::Function(args, result) => {
                pending.extend(args);
                pending.push(result);
            }
            Type::List(a) | Type::Option(a) => pending.push(a),
            Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                pending.extend([a.as_ref(), b.as_ref()])
            }
            _ => {}
        }
    }
    total
}

/// Validate every executable codec, including inactive bodies, before backend or REPL execution.
pub(crate) fn validate_program(program: &ir::Program) -> Result<(), Diagnostic> {
    let mut pending: Vec<_> = program.functions.iter().map(|f| &f.body).collect();
    let mut seen = std::collections::HashSet::new();
    let mut count = 0usize;
    let mut work = 0usize;
    while let Some(expr) = pending.pop() {
        count += 1;
        if count + pending.len() > 200_000 {
            return Err(Diagnostic::new(
                expr.span,
                "JSON codec executable work limit exceeded",
            ));
        }
        if let ir::ExprKind::JsonCodec {
            direction,
            input,
            plan,
        } = &expr.kind
        {
            if seen.insert(std::rc::Rc::as_ptr(plan) as usize) {
                plan.validate_budget(&program.types, expr.span, &mut work)?;
                for entry in &plan.entries {
                    if let Kind::Custom { encode, decode } = &entry.kind {
                        for (callback, input, output) in [
                            (encode, &entry.ty, Type::Native(NativeType::JsonValue)),
                            (
                                decode,
                                &Type::Native(NativeType::JsonValue),
                                entry.ty.clone(),
                            ),
                        ] {
                            work = work.saturating_add(program.functions.len());
                            if work > MAX_WORK {
                                return Err(Diagnostic::new(
                                    expr.span,
                                    "JSON callback validation work limit exceeded",
                                ));
                            }
                            let Callback::Function(id) = callback else {
                                return Err(Diagnostic::new(
                                    expr.span,
                                    "unresolved JSON callback in executable plan",
                                ));
                            };
                            let function =
                                program.functions.iter().find(|f| f.id == *id).ok_or_else(
                                    || Diagnostic::new(expr.span, "missing JSON callback function"),
                                )?;
                            let expected = Type::Result(
                                Box::new(output),
                                Box::new(Type::Native(NativeType::JsonError)),
                            );
                            if function.mailbox.is_some()
                                || !function.captures.is_empty()
                                || function.params.len() != 1
                                || function.params[0].ty != *input
                                || function.return_type != expected
                            {
                                return Err(Diagnostic::new(
                                    expr.span,
                                    "JSON callback has invalid concrete signature",
                                ));
                            }
                        }
                    }
                }
            }
            crate::unions::bound(&input.ty, expr.span)?;
            crate::unions::bound(&expr.ty, expr.span)?;
            let root = &plan.entries[plan.root].ty;
            work =
                work.saturating_add(type_size(root) + type_size(&input.ty) + type_size(&expr.ty));
            if work > MAX_WORK {
                return Err(Diagnostic::new(
                    expr.span,
                    "JSON codec program work limit exceeded",
                ));
            }
            let (argument, output) = match direction {
                Direction::Encode => (root.clone(), Type::String),
                Direction::Decode => (Type::String, root.clone()),
            };
            let result = Type::Result(
                Box::new(output),
                Box::new(Type::Native(NativeType::JsonError)),
            );
            if input.ty != argument || expr.ty != result {
                return Err(Diagnostic::new(
                    expr.span,
                    "JSON codec input or result does not match its concrete plan",
                ));
            }
        }
        let children = ir::children(expr);
        if children.len() > 200_000usize.saturating_sub(count + pending.len()) {
            return Err(Diagnostic::new(
                expr.span,
                "JSON codec executable work limit exceeded",
            ));
        }
        pending.extend(children);
    }
    Ok(())
}
