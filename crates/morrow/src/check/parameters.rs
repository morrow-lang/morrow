//! Resolve private parameter slots from complete, caller-independent clause evidence.
use super::*;
use std::borrow::Cow;

type Constraint<'a> = (&'a ast::Pattern, Type);

/// Fill only private omissions, keeping the original source and fully annotated fast path.
pub(super) fn resolve<'a>(
    program: &'a ast::Program,
    registry: &nominal::Registry,
) -> Checked<Cow<'a, ast::Program>> {
    if !program
        .functions
        .iter()
        .any(|f| f.params.iter().any(|p| p.annotation.is_none()))
    {
        return Ok(Cow::Borrowed(program));
    }
    let mut prepared = program.clone();
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
        inference: Inference {
            newtypes: registry.newtype_definitions(),
            newtype_work: registry.newtype_budget(),
            ..Inference::default()
        },
        function_return: Type::Unit,
        deferred: false,
        loop_depth: 0,
    };
    checker.inference.probing = true;
    let result = prepare_groups(program, &mut prepared, &mut checker);
    result.map_err(|error| {
        if error.message.contains("return inference work limit") {
            Diagnostic::new(
                error.span,
                "parameter inference work limit exceeded; add parameter annotations",
            )
        } else {
            error
        }
    })?;
    Ok(Cow::Owned(prepared))
}

/// Keep inference variables independent across groups while retaining one aggregate budget.
fn prepare_groups(
    source: &ast::Program,
    prepared: &mut ast::Program,
    checker: &mut Checker<'_>,
) -> Checked<()> {
    let mut start = 0;
    while start < source.functions.len() {
        let name = &source.functions[start].name;
        let mut end = start + 1;
        while end < source.functions.len() && source.functions[end].name == *name {
            end += 1;
        }
        let group = &source.functions[start..end];
        if group
            .iter()
            .any(|f| f.params.iter().any(|p| p.annotation.is_none()))
        {
            let types = infer_group(group, checker)?;
            for function in &mut prepared.functions[start..end] {
                for (param, ty) in function.params.iter_mut().zip(&types) {
                    returns::charge_output(&checker.inference, ty, param.span)?;
                    param.annotation = Some(ty.clone());
                }
            }
        }
        start = end;
    }
    Ok(())
}

/// Constrain all annotations before patterns so rigid generics never depend on clause order.
pub(super) fn infer_group(
    group: &[ast::Function],
    checker: &mut Checker<'_>,
) -> Checked<Vec<Type>> {
    let first = &group[0];
    let slots: Vec<_> = first
        .params
        .iter()
        .map(|_| checker.inference.fresh())
        .collect();
    let mut pending = Vec::new();
    for function in group {
        if function.params.len() != slots.len() || function.group_start != first.group_start {
            return Err(Diagnostic::new(
                function.span,
                "function clauses must be adjacent and have the same arity",
            ));
        }
        for (param, slot) in function.params.iter().zip(&slots) {
            returns::charge(&checker.inference, param.span)?;
            if let Some(annotation) = &param.annotation {
                checker.inference.unify(
                    slot,
                    annotation,
                    param.span,
                    "parameter clause annotation",
                )?;
            } else if function.public {
                return Err(Diagnostic::new(
                    param.span,
                    "public function parameters require type annotations",
                ));
            }
            pending.push((&param.pattern, slot.clone()));
        }
    }
    constrain(checker, pending)?;
    slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let ty = checker.inference.resolve(slot, first.params[index].span)?;
            if returns::has_infer(&ty) && !checker.inference.whole_signature {
                return Err(Diagnostic::new(
                    first.params[index].span,
                    format!(
                        "cannot infer parameter {} of '{}'; add a type annotation",
                        index + 1,
                        first.name
                    ),
                ));
            }
            if !checker.inference.whole_signature {
                validate_type(&ty, first.params[index].span)?;
            }
            Ok(ty)
        })
        .collect()
}

/// Revisit only tuple-rest obligations whose fixed arity another clause can establish.
pub(super) fn constrain(
    checker: &mut Checker<'_>,
    mut pending: Vec<Constraint<'_>>,
) -> Checked<()> {
    loop {
        let mut deferred = Vec::new();
        let mut progressed = false;
        while let Some((pattern, ty)) = pending.pop() {
            returns::charge(&checker.inference, pattern.span)?;
            if let ast::PatternKind::TupleRest { prefix, .. } = &pattern.kind {
                let shape = checker.inference.resolve(&ty, pattern.span)?;
                if matches!(shape, Type::Infer(_)) {
                    deferred.push((pattern, ty));
                    continue;
                }
                let fields = sequences::tuple_fields(&shape, pattern.span)?;
                if prefix.len() > fields.len() {
                    return Err(Diagnostic::new(
                        pattern.span,
                        "tuple parameter pattern prefix exceeds tuple arity",
                    ));
                }
                pending.extend(prefix.iter().zip(fields.iter().cloned()));
            } else {
                constrain_pattern(checker, pattern, &ty, &mut pending)?;
            }
            progressed = true;
        }
        if deferred.is_empty() {
            return Ok(());
        }
        if !progressed && checker.inference.whole_signature {
            return Ok(());
        }
        if !progressed {
            return Err(Diagnostic::new(
                deferred[0].0.span,
                "cannot infer tuple-rest parameter arity; add a tuple type annotation",
            ));
        }
        pending = deferred;
    }
}

/// Build scalar/container constraints without binding source names or consuming Results.
fn constrain_pattern<'a>(
    checker: &mut Checker<'_>,
    pattern: &'a ast::Pattern,
    ty: &Type,
    pending: &mut Vec<Constraint<'a>>,
) -> Checked<()> {
    use ast::PatternKind::*;
    let expected = match &pattern.kind {
        Typed { .. } => {
            return Err(Diagnostic::new(
                pattern.span,
                "typed parameter subpatterns require an explicit enclosing annotation",
            ));
        }
        Bind(_) | Wildcard => return Ok(()),
        Int(_) => Type::Int,
        Bool(_) => Type::Bool,
        String(_) => Type::String,
        Tuple(fields) => {
            let types: Vec<_> = fields.iter().map(|_| checker.inference.fresh()).collect();
            pending.extend(fields.iter().zip(types.iter().cloned()));
            sequences::tuple_type(types)
        }
        List { prefix, .. } => {
            let element = checker.inference.fresh();
            pending.extend(prefix.iter().map(|p| (p, element.clone())));
            Type::List(Box::new(element))
        }
        NamedConstructor { name, fields } => {
            let (expected, _, types) = checker.pattern_signature(name, pattern.span)?;
            if fields.len() != types.len() {
                return Err(Diagnostic::new(
                    pattern.span,
                    "constructor parameter pattern field arity mismatch",
                ));
            }
            for field in &types {
                returns::charge_output(&checker.inference, field, pattern.span)?;
            }
            pending.extend(fields.iter().zip(types));
            expected
        }
        Constructor { constructor, .. } => {
            let name = match constructor {
                crate::Constructor::Some => "Some",
                crate::Constructor::None => "None",
                crate::Constructor::Ok => "Ok",
                crate::Constructor::Err => "Err",
            };
            checker.pattern_signature(name, pattern.span)?.0
        }
        TupleRest { .. } => {
            unreachable!("tuple-rest constraints are handled before structural patterns")
        }
    };
    checker
        .inference
        .unify(ty, &expected, pattern.span, "parameter pattern")
}
