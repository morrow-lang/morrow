//! Optional source facts collected only from finalized, fully validated source functions.
//! Source spans are snapshot-local identities; backend IDs and inference probes never escape.
use super::*;

/// Exact current-source occurrence and optional lexical/global declaration selected by the editor.
#[derive(Clone, Debug)]
pub struct Query {
    pub occurrence: Span,
    pub binding: Option<Span>,
    pub function: Option<String>,
}

/// An intrinsic restriction in checker-owned language, separate from source type syntax.
#[derive(Clone, Debug)]
pub struct Requirement {
    pub ty: Type,
    pub message: String,
}

/// One reusable source function scheme, regardless of concrete callers or clause lowering.
#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub name: String,
    pub origin: Span,
    pub parameters: Vec<Type>,
    pub result: Type,
    pub generics: Vec<String>,
    pub requirements: Vec<Requirement>,
    pub clauses: usize,
}

/// A checked record field, tuple slot or supported receiver method.
#[derive(Clone, Debug)]
pub struct Member {
    pub name: String,
    pub ty: Type,
    pub origin: Option<Span>,
}

/// Final selected facts only; absence means no justified source fact was found.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub function: Option<FunctionInfo>,
    pub context: Option<FunctionInfo>,
    pub value: Option<Type>,
    pub receiver: Option<Type>,
    pub members: Vec<Member>,
}

/// Analyze one original-source selection after the complete ordinary check succeeds.
/// Caller-created AST/query inputs are validated; metadata has an independent 16k-node budget.
pub fn analyze(source: &ast::Program, query: Query) -> Checked<Facts> {
    if !valid_span(query.occurrence)
        || query.binding.is_some_and(|s| !valid_span(s))
        || query.function.as_ref().is_some_and(|s| s.len() > 65_536)
    {
        return Err(Diagnostic::new(
            query.occurrence,
            "invalid editor source selection",
        ));
    }
    super::pipeline(source, |prepared, registry, signatures| {
        analyze_prepared(source, prepared, registry, signatures, query)
    })
    .map(|(_, facts)| facts)
}

/// Publish reusable schemes for every source group after one complete library check.
/// Metadata shares a 16k-node/1MiB budget; clause groups retain their original anchors.
pub fn function_schemes(source: &ast::Program) -> Checked<Vec<FunctionInfo>> {
    super::pipeline(source, |_, _, signatures| {
        let mut budget = Budget::default();
        let mut result = Vec::new();
        let mut index = 0;
        while index < source.functions.len() {
            let first = &source.functions[index];
            let mut end = index + 1;
            while end < source.functions.len() && source.functions[end].name == first.name {
                end += 1;
            }
            let signature = signatures
                .get(&first.name)
                .ok_or_else(|| Diagnostic::new(first.span, "missing finalized source signature"))?;
            result.push(signature_info(first, signature, end - index, &mut budget)?);
            index = end;
        }
        Ok(result)
    })
    .map(|(_, facts)| facts)
}

/// Validate caller-supplied presentation metadata before any type/name cloning.
pub(crate) fn validate_schemes(schemes: &[FunctionInfo]) -> Checked<()> {
    if schemes.len() > 4096 {
        return Err(limit());
    }
    let mut budget = Budget::default();
    for info in schemes {
        if info.parameters.len() > 255
            || info.generics.len() > 4096
            || info.requirements.len() > 4096
            || !valid_span(info.origin)
        {
            return Err(limit());
        }
        budget.text(&info.name)?;
        for ty in info.parameters.iter().chain([&info.result]) {
            budget.ty(ty)?;
        }
        for name in &info.generics {
            budget.text(name)?;
        }
        for requirement in &info.requirements {
            budget.ty(&requirement.ty)?;
            budget.text(&requirement.message)?;
        }
    }
    Ok(())
}

/// Keep preparation and backend validation identical to ordinary compilation.
fn analyze_prepared(
    source: &ast::Program,
    prepared: &ast::Program,
    registry: &nominal::Registry,
    signatures: &HashMap<String, Signature>,
    query: Query,
) -> Checked<Facts> {
    let mut budget = Budget::default();
    let mut facts = Facts::default();
    if let Some(name) = &query.function {
        facts.function = function_info(source, signatures, name, &mut budget)?;
    }
    let selected = source.functions.iter().find(|f| {
        !f.name.starts_with('$') && contains(f.span, query.occurrence) && f.span != Span::default()
    });
    let Some(selected) = selected else {
        return Ok(facts);
    };
    let function = prepared
        .functions
        .iter()
        .find(|f| f.name == selected.name)
        .ok_or_else(|| Diagnostic::new(selected.span, "missing finalized source function"))?;
    facts.context = function_info(source, signatures, &selected.name, &mut budget)?;
    let signature = &signatures[&selected.name];
    let mut checker = final_checker(registry, signatures, signature, query);
    checker.function(function)?;
    let recorder = checker.editor.take().expect("editor recorder enabled");
    recorder.finish(&checker.inference, &mut facts, &mut budget)?;
    if let Some(receiver) = &facts.receiver {
        facts.members = members(source, registry, receiver, selected.span, &mut budget)?;
    }
    Ok(facts)
}

/// A final template environment uses published rigid variables, never SCC inference slots.
fn final_checker<'a>(
    registry: &'a nominal::Registry,
    signatures: &'a HashMap<String, Signature>,
    signature: &Signature,
    query: Query,
) -> Checker<'a> {
    Checker {
        mailbox: None,
        signatures,
        registry,
        scopes: vec![HashMap::new()],
        local_count: 0,
        expr_count: 0,
        inference: Inference {
            template: !signature.generics.is_empty(),
            probing: true,
            template_names: signature.generics.iter().cloned().collect(),
            ..Inference::default()
        },
        function_return: signature.result.clone(),
        deferred: false,
        loop_depth: 0,
        recovery: None,
        editor: Some(Recorder {
            query,
            binding: None,
            expression: None,
            receiver: None,
            budget: Budget::default(),
            error: None,
        }),
    }
}

/// Publish one source group and its closed intrinsic requirements under an aggregate budget.
fn function_info(
    source: &ast::Program,
    signatures: &HashMap<String, Signature>,
    name: &str,
    budget: &mut Budget,
) -> Checked<Option<FunctionInfo>> {
    let Some(signature) = signatures.get(name) else {
        return Ok(None);
    };
    let Some(first) = source
        .functions
        .iter()
        .find(|f| f.name == name && f.span != Span::default())
    else {
        return Ok(None);
    };
    signature_info(
        first,
        signature,
        source.functions.iter().filter(|f| f.name == name).count(),
        budget,
    )
    .map(Some)
}

/// Copy one final scheme only after charging its complete structured metadata.
fn signature_info(
    first: &ast::Function,
    signature: &Signature,
    clauses: usize,
    budget: &mut Budget,
) -> Checked<FunctionInfo> {
    let name = first.name.as_str();
    if !valid_span(first.span) {
        return Err(Diagnostic::new(
            first.span,
            "invalid editor declaration span",
        ));
    }
    budget.text(name)?;
    for ty in signature.params.iter().chain([&signature.result]) {
        budget.ty(ty)?;
    }
    for name in &signature.generics {
        budget.text(name)?;
    }
    let mut requirements = Vec::new();
    for requirement in &signature.requirements {
        let (ty, message) = requirement.editor_view();
        budget.ty(ty)?;
        budget.text(message)?;
        requirements.push(Requirement {
            ty: ty.clone(),
            message: message.into(),
        });
    }
    Ok(FunctionInfo {
        name: name.into(),
        origin: first.span,
        parameters: signature.params.clone(),
        result: signature.result.clone(),
        generics: signature.generics.clone(),
        requirements,
        clauses,
    })
}

/// Resolve members using the same nominal substitution as field access, retaining source origins.
fn members(
    source: &ast::Program,
    registry: &nominal::Registry,
    ty: &Type,
    span: Span,
    budget: &mut Budget,
) -> Checked<Vec<Member>> {
    if registry.is_newtype(ty) {
        let inner = registry.newtype_inner(ty, span)?;
        budget.ty(&inner)?;
        let origin = match ty {
            Type::Named(name, _) => source
                .newtypes
                .iter()
                .find(|d| d.name == *name)
                .map(|d| d.inner_span),
            _ => None,
        };
        return Ok(vec![Member {
            name: "0".into(),
            ty: inner,
            origin,
        }]);
    }
    match ty {
        Type::Tuple(fields) => fields
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                budget.ty(ty)?;
                Ok(Member {
                    name: index.to_string(),
                    ty: ty.clone(),
                    origin: None,
                })
            })
            .collect(),
        Type::List(inner) => {
            budget.ty(inner)?;
            let ty = Type::Function(
                Vec::new(),
                Box::new(Type::List(Box::new(Type::Tuple(vec![
                    Type::Int,
                    (**inner).clone(),
                ])))),
            );
            budget.ty(&ty)?;
            Ok(vec![Member {
                name: "enumerate".into(),
                ty,
                origin: None,
            }])
        }
        Type::Named(name, args) => record_members(source, name, args, ty, budget),
        _ => Ok(Vec::new()),
    }
}

/// Instantiate record fields one at a time so a large layout cannot outrun metadata limits.
fn record_members(
    source: &ast::Program,
    name: &str,
    args: &[Type],
    ty: &Type,
    budget: &mut Budget,
) -> Checked<Vec<Member>> {
    let Some(decl) = source
        .types
        .iter()
        .find(|decl| decl.name == name && decl.record)
    else {
        return Ok(Vec::new());
    };
    budget.ty(ty)?;
    let substitutions = decl
        .parameters
        .iter()
        .cloned()
        .zip(args.iter().cloned())
        .collect();
    let mut result = Vec::new();
    for field in &decl.variants[0].fields {
        budget.ty(&field.ty)?;
        let ty = nominal::substitute(&field.ty, &substitutions)?;
        budget.ty(&ty)?;
        let name = field.name.as_deref().unwrap_or("");
        budget.text(name)?;
        if !valid_span(field.span) {
            return Err(Diagnostic::new(field.span, "invalid editor field span"));
        }
        result.push(Member {
            name: name.into(),
            ty: ty.clone(),
            origin: Some(field.span),
        });
    }
    Ok(result)
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    /// Charge bounded type traversal before cloning any caller-dependent graph.
    fn ty(&mut self, ty: &Type) -> Checked<()> {
        let mut stack = vec![(ty, 0)];
        while let Some((ty, depth)) = stack.pop() {
            self.nodes += 1;
            if self.nodes > 16_384 || depth >= 128 {
                return Err(limit());
            }
            match ty {
                Type::Pid(t) | Type::List(t) | Type::Option(t) => stack.push((t, depth + 1)),
                Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                    stack.push((a, depth + 1));
                    stack.push((b, depth + 1));
                }
                Type::Function(args, result) => {
                    if args.len() > 4096 {
                        return Err(limit());
                    }
                    stack.extend(args.iter().map(|a| (a, depth + 1)));
                    stack.push((result, depth + 1));
                }
                Type::Union(args) | Type::Tuple(args) | Type::Named(_, args) => {
                    if args.len() > 4096 {
                        return Err(limit());
                    }
                    stack.extend(args.iter().map(|a| (a, depth + 1)));
                }
                _ => {}
            }
            if let Type::Generic(name) | Type::Named(name, _) = ty {
                self.text(name)?;
            }
        }
        Ok(())
    }
    /// Bound all retained names/requirements separately from typed node count.
    fn text(&mut self, text: &str) -> Checked<()> {
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > 1_048_576 {
            Err(limit())
        } else {
            Ok(())
        }
    }
}

pub(super) struct Recorder {
    query: Query,
    binding: Option<(Span, Type)>,
    expression: Option<(Span, Type)>,
    receiver: Option<Type>,
    budget: Budget,
    error: Option<Diagnostic>,
}
impl Recorder {
    /// Select source binding origins before lowering erases names and pattern spans.
    fn binding(&mut self, name: &str, ty: &Type, span: Span) {
        if name.starts_with('$')
            || name == "_"
            || !self.query.binding.is_some_and(|s| contains(span, s))
        {
            return;
        }
        if !valid_span(span) {
            self.error = Some(limit());
            return;
        }
        if let Err(error) = self.budget.ty(ty) {
            self.error = Some(error);
            return;
        }
        if self
            .binding
            .as_ref()
            .is_some_and(|(old, _)| old.end - old.start <= span.end - span.start)
        {
            return;
        }
        self.binding = Some((span, ty.clone()));
    }
    /// Charge callable argument types before constructing their aggregate public signature.
    fn precharge(&mut self, kind: &ir::ExprKind, ty: &Type) -> Checked<()> {
        self.budget.ty(ty)?;
        match kind {
            ir::ExprKind::Call { args, .. } => {
                for arg in args {
                    self.budget.ty(&arg.ty)?;
                }
            }
            ir::ExprKind::Invoke { callee, .. } => self.budget.ty(&callee.ty)?,
            ir::ExprKind::Field { value, .. } | ir::ExprKind::Unwrap(value) => {
                self.budget.ty(&value.ty)?
            }
            _ => {}
        }
        Ok(())
    }
    /// Retain the smallest explicit source value range, never synthesized outer control nodes.
    fn value(&mut self, span: Span, ty: Type, receiver: Option<Type>) {
        if !valid_span(span) {
            self.error = Some(limit());
            return;
        }
        if self
            .expression
            .as_ref()
            .is_some_and(|(old, _)| old.end - old.start < span.end - span.start)
        {
            return;
        }
        if let Err(error) = self
            .budget
            .ty(&ty)
            .and_then(|_| receiver.as_ref().map_or(Ok(()), |r| self.budget.ty(r)))
        {
            self.error = Some(error);
            return;
        }
        self.expression = Some((span, ty));
        self.receiver = receiver;
    }
    /// Resolve all selected candidates in the finalized source environment before publication.
    fn finish(self, inference: &Inference, facts: &mut Facts, budget: &mut Budget) -> Checked<()> {
        if let Some(error) = self.error {
            return Err(error);
        }
        facts.value = self
            .binding
            .map(|(_, ty)| ty)
            .or_else(|| self.expression.map(|(_, ty)| ty))
            .map(|ty| inference.concrete(&ty, self.query.occurrence))
            .transpose()?;
        facts.receiver = self
            .receiver
            .map(|ty| inference.concrete(&ty, self.query.occurrence))
            .transpose()?;
        for ty in facts.value.iter().chain(facts.receiver.iter()) {
            budget.ty(ty)?;
        }
        Ok(())
    }
}

impl Checker<'_> {
    /// Ordinary and provisional checking pay no source-fact allocation cost.
    pub(super) fn bind_source(&mut self, name: &str, ty: Type, span: Span) -> ir::LocalId {
        if let Some(editor) = &mut self.editor {
            editor.binding(name, &ty, span);
        }
        self.bind(name, ty)
    }
    /// Observe original names/fields/calls only, excluding lowered control scaffolding.
    pub(super) fn observe_source(&mut self, source: &ast::Expr, kind: &ir::ExprKind, ty: &Type) {
        let Some(editor) = &self.editor else {
            return;
        };
        let selected = editor.query.occurrence;
        let global = match &source.kind {
            ast::ExprKind::GlobalName { resolved, .. }
            | ast::ExprKind::GlobalCall { resolved, .. } => {
                editor.query.function.as_deref() == Some(resolved.as_str())
            }
            ast::ExprKind::Name(name) | ast::ExprKind::Call { name, .. } => {
                editor.query.function.as_deref() == Some(name.as_str())
            }
            _ => false,
        };
        if !contains(source.span, selected) {
            return;
        }
        if let Err(error) = self
            .editor
            .as_mut()
            .expect("selected recorder")
            .precharge(kind, ty)
        {
            self.editor.as_mut().expect("selected recorder").error = Some(error);
            return;
        }
        let candidate = self.source_candidate(source, kind, ty, selected, global);
        if let Some((value, receiver)) = candidate
            && let Some(editor) = &mut self.editor
        {
            editor.value(source.span, value, receiver);
        }
    }
    /// Separate source-role selection from recording and final inference resolution.
    fn source_candidate(
        &self,
        source: &ast::Expr,
        kind: &ir::ExprKind,
        ty: &Type,
        selected: Span,
        global: bool,
    ) -> Option<(Type, Option<Type>)> {
        let method = match &source.kind {
            ast::ExprKind::Call { name, .. } => {
                name.ends_with(".enumerate")
                    && self.local(name.split('.').next().unwrap_or("")).is_some()
            }
            ast::ExprKind::Apply { callee, .. } => {
                matches!(&callee.kind,ast::ExprKind::Field {name,..} if name=="enumerate")
            }
            _ => false,
        };
        if global {
            let value = if matches!(
                source.kind,
                ast::ExprKind::Call { .. } | ast::ExprKind::GlobalCall { .. }
            ) {
                callable(kind, ty).1
            } else {
                ty.clone()
            };
            Some((value, None))
        } else if method {
            match kind {
                ir::ExprKind::Call {
                    target: ir::CallTarget::Builtin(ir::Builtin::ListEnumerate),
                    args,
                } => args.first().map(|receiver| {
                    (
                        Type::Function(Vec::new(), Box::new(ty.clone())),
                        Some(receiver.ty.clone()),
                    )
                }),
                _ => None,
            }
        } else {
            match &source.kind {
                ast::ExprKind::Name(name) | ast::ExprKind::GlobalName { name, .. } => {
                    named_value(name, source.span, selected, kind, ty)
                }
                ast::ExprKind::Int(_) | ast::ExprKind::Float(_) | ast::ExprKind::Bool(_) => {
                    Some((ty.clone(), None))
                }
                ast::ExprKind::Call { name, .. } | ast::ExprKind::GlobalCall { name, .. } => {
                    if selected.start - source.span.start >= name.len() {
                        return None;
                    }
                    let (kind, ty) = callable(kind, ty);
                    named_value(name, source.span, selected, kind, &ty)
                }
                ast::ExprKind::Field { value, .. } if selected.start >= value.span.end => {
                    Some(field_value(kind, ty))
                }
                _ => None,
            }
        }
    }
}

/// Recover the source callable's instantiated signature before backend target rewriting.
fn callable<'a>(kind: &'a ir::ExprKind, result: &Type) -> (&'a ir::ExprKind, Type) {
    match kind {
        ir::ExprKind::Call { args, .. } => (
            kind,
            Type::Function(
                args.iter().map(|a| a.ty.clone()).collect(),
                Box::new(result.clone()),
            ),
        ),
        ir::ExprKind::Invoke { callee, .. } => (&callee.kind, callee.ty.clone()),
        _ => (kind, result.clone()),
    }
}

/// Select a qualified local member without confusing the root binder or sibling fields.
fn named_value(
    name: &str,
    span: Span,
    selected: Span,
    kind: &ir::ExprKind,
    ty: &Type,
) -> Option<(Type, Option<Type>)> {
    let parts: Vec<_> = name.split('.').take(129).collect();
    if parts.len() > 128 {
        return None;
    }
    let mut offset = span.start;
    let selected_part = parts.iter().position(|part| {
        let matches = selected.start >= offset && selected.end <= offset + part.len();
        offset += part.len() + 1;
        matches
    })?;
    let mut kind = kind;
    let mut ty = ty;
    for _ in selected_part + 1..parts.len() {
        if let ir::ExprKind::Field { value, .. } | ir::ExprKind::Unwrap(value) = kind {
            kind = &value.kind;
            ty = &value.ty;
        } else {
            break;
        }
    }
    Some(field_value(kind, ty))
}

/// The checked field receiver supplies layout evidence independently of its selected value.
fn field_value(kind: &ir::ExprKind, ty: &Type) -> (Type, Option<Type>) {
    let receiver = match kind {
        ir::ExprKind::Field { value, .. } | ir::ExprKind::Unwrap(value) => Some(value.ty.clone()),
        _ => None,
    };
    (ty.clone(), receiver)
}

/// Query spans must be contained within one original source construct.
fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end && inner.start <= inner.end
}
/// Report exhausted metadata work without emitting a partial misleading type.
fn limit() -> Diagnostic {
    Diagnostic::new(Span::default(), "editor type metadata limit exceeded")
}

/// Source graphs fit below sixteen MiB including per-file offset separators.
fn valid_span(span: Span) -> bool {
    span.start <= span.end && span.end <= 16 * 1024 * 1024
}

/// Publish only independently concrete receiver members from the editor proof checker.
pub(super) fn receiver_facts(
    source: &ast::Program,
    registry: &nominal::Registry,
    ty: Type,
    span: Span,
) -> Checked<Facts> {
    let mut budget = Budget::default();
    budget.ty(&ty)?;
    let members = members(source, registry, &ty, span, &mut budget)?;
    Ok(Facts {
        receiver: Some(ty),
        members,
        ..Facts::default()
    })
}
