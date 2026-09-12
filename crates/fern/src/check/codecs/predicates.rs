//! Conditional wire capabilities retain variables without fabricating concrete witnesses.
use super::super::schemes::{Capability, Requirement};
use super::*;

/// One registry allowance covers every predicate query and every recursively expanded field.
pub(in crate::check) fn require(
    inference: &Inference,
    registry: &nominal::Registry,
    capability: Capability,
    ty: &Type,
    span: Span,
) -> Checked<()> {
    let empty = ast::Program::default();
    let mut proof = Planner::new(&empty, registry);
    proof.symbolic = true;
    proof.work = registry.codec_predicate_work.get();
    let result = walk(&mut proof, inference, capability, ty, span);
    registry.codec_predicate_work.set(proof.work);
    result
}

/// Cache exact instantiated predicates only after paying for their complete key.
fn walk(
    proof: &mut Planner<'_>,
    inference: &Inference,
    capability: Capability,
    ty: &Type,
    span: Span,
) -> Checked<()> {
    proof.type_work(ty, span)?;
    let mut pending = vec![(capability, ty.clone())];
    let mut seen = HashSet::new();
    while let Some((capability, ty)) = pending.pop() {
        proof.type_work(&ty, span)?;
        let ty = inference.resolve(&ty, span)?;
        proof.type_work(&ty, span)?;
        returns::charge_output(inference, &ty, span)?;
        if !seen.insert((capability, ty.clone())) {
            continue;
        }
        if union(proof, inference, capability, &ty, span)? {
            if let Type::Union(children) = &ty {
                for child in children {
                    proof.type_work(child, span)?;
                    pending.push((capability, child.clone()));
                }
            }
            continue;
        }
        if variable(proof, inference, capability, &ty, span)? {
            continue;
        }
        for (cap, child) in children(proof, capability, &ty, span)? {
            proof.type_work(&child, span)?;
            pending.push((cap, child));
        }
    }
    Ok(())
}

/// Check every known component now, while preserving symbolic union disjointness as one equation.
fn union(
    proof: &mut Planner<'_>,
    inference: &Inference,
    cap: Capability,
    ty: &Type,
    span: Span,
) -> Checked<bool> {
    let Type::Union(_) = ty else {
        return Ok(false);
    };
    if cap == Capability::JsonStringKey {
        return Err(Diagnostic::new(
            span,
            "JSON object keys must have type String",
        ));
    }
    proof.plan(ty, span, 0)?;
    proof.finite(span)?;
    let symbolic = returns::has_infer(ty) || !nominal::generics([ty.clone()]).is_empty();
    if symbolic {
        for existing in inference.requirements.borrow().iter() {
            proof.type_work(&existing.ty, span)?;
        }
        schemes::retain_requirement(
            &mut inference.requirements.borrow_mut(),
            Requirement {
                capability: cap,
                ty: ty.clone(),
                span,
            },
            inference,
        )?;
    }
    Ok(true)
}

/// Only declared universals or whole-signature existential slots can retain a requirement.
fn variable(
    proof: &mut Planner<'_>,
    inference: &Inference,
    capability: Capability,
    ty: &Type,
    span: Span,
) -> Checked<bool> {
    let allowed = (inference.whole_signature && matches!(ty, Type::Infer(_)))
        || inference.template
            && matches!(ty, Type::Generic(n) if inference.template_names.contains(n));
    if !allowed {
        return Ok(false);
    }
    for existing in inference.requirements.borrow().iter() {
        proof.type_work(&existing.ty, span)?;
    }
    schemes::retain_requirement(
        &mut inference.requirements.borrow_mut(),
        Requirement {
            capability,
            ty: ty.clone(),
            span,
        },
        inference,
    )?;
    Ok(true)
}

/// Decompose actual storage, so phantom parameters do not acquire unnecessary predicates.
fn children(
    proof: &mut Planner<'_>,
    cap: Capability,
    ty: &Type,
    span: Span,
) -> Checked<Vec<(Capability, Type)>> {
    use Capability::{Json, JsonNonNull, JsonStringKey};
    if cap == JsonStringKey {
        return if *ty == Type::String {
            Ok(Vec::new())
        } else {
            Err(Diagnostic::new(
                span,
                "JSON object keys must have type String",
            ))
        };
    }
    if cap == JsonNonNull
        && matches!(
            ty,
            Type::Unit | Type::Option(_) | Type::Native(runtime::NativeType::JsonValue)
        )
    {
        return Err(Diagnostic::new(
            span,
            "Option payload can encode null; transparent JSON Option would be ambiguous",
        ));
    }
    Ok(match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Unit
        | Type::Native(runtime::NativeType::JsonValue) => Vec::new(),
        Type::List(a) => vec![(Json, *a.clone())],
        Type::Option(a) => vec![(JsonNonNull, *a.clone())],
        Type::Map(k, v) => vec![(JsonStringKey, *k.clone()), (Json, *v.clone())],
        Type::Tuple(items) => items.iter().cloned().map(|t| (Json, t)).collect(),
        Type::Named(name, _) => return nominal_children(proof, cap, ty, name, span),
        _ => return Err(Diagnostic::new(span, unsupported(ty))),
    })
}

/// Precharge substitution before allocating expanded fields; every sibling remains in the queue.
fn nominal_children(
    proof: &mut Planner<'_>,
    cap: Capability,
    ty: &Type,
    name: &str,
    span: Span,
) -> Checked<Vec<(Capability, Type)>> {
    let decl = proof
        .declarations
        .get(name)
        .ok_or_else(|| Diagnostic::new(span, "unknown nominal JSON codec target"))?;
    if !decl.derives.iter().any(|d| d.name == "Json") {
        return Err(Diagnostic::new(
            span,
            format!("{name} has no Json codec; add derive(Json)"),
        ));
    }
    let newtype = proof.registry.is_newtype(ty);
    proof.layout_work(ty, decl, span)?;
    let layout = proof.registry.layout(ty, span)?;
    let cap = if newtype { cap } else { Capability::Json };
    Ok(layout
        .variants
        .into_iter()
        .flatten()
        .map(|t| (cap, t))
        .collect())
}

/// Charge propagated predicate comparisons independently of the existing C1 node allowance.
pub(in crate::check) fn retention(
    registry: &nominal::Registry,
    candidate: &Requirement,
    existing: &[Requirement],
) -> Checked<()> {
    if !is_json(candidate.capability) {
        return Ok(());
    }
    let empty = ast::Program::default();
    let mut proof = Planner::new(&empty, registry);
    proof.symbolic = true;
    proof.work = registry.codec_predicate_work.get();
    let result = (|| {
        proof.type_work(&candidate.ty, candidate.span)?;
        for requirement in existing {
            proof.type_work(&requirement.ty, candidate.span)?;
        }
        Ok(())
    })();
    registry.codec_predicate_work.set(proof.work);
    result
}
/// Central discriminator keeps Json predicates out of fixed scalar capability discharge.
pub(in crate::check) fn is_json(capability: Capability) -> bool {
    matches!(
        capability,
        Capability::Json | Capability::JsonNonNull | Capability::JsonStringKey
    )
}
