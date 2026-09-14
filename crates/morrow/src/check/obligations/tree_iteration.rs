//! Representative children inherit structural evidence only from complete actual parent provenance.
use super::*;
#[derive(Clone, PartialEq, Eq)]
enum Shape {
    Plain,
    Cut(usize),
    Product(Vec<Shape>),
    Unboxed(usize, Box<Shape>),
}
pub(super) struct Plan {
    function: usize,
    parameter: usize,
    shape: Shape,
}
/// Build an intersection of actual descendant evidence, without guessing any scalar or tag value.
pub(super) fn prepare(
    engine: &mut Engine<'_>,
    actual: &Value,
    ty: &Type,
    span: Span,
) -> Checked<Option<Plan>> {
    let Some((function, parameter, anchor)) = recursive_trees::context(engine) else {
        return Ok(None);
    };
    let shape = shape(engine, actual, ty, anchor, span, 0)?;
    Ok((shape != Shape::Plain).then_some(Plan {
        function,
        parameter,
        shape,
    }))
}
/// Every possible actual alternative must carry the same structural evidence.
fn shape(
    engine: &mut Engine<'_>,
    value: &Value,
    ty: &Type,
    anchor: usize,
    span: Span,
    depth: usize,
) -> Checked<Shape> {
    engine.charge(1, span)?;
    if depth >= DEPTH_LIMIT {
        return engine.unsupported(span);
    }
    if !value.complete {
        return Ok(Shape::Plain);
    }
    if let Region::Choice(choices) = &value.node.kind {
        return choices_shape(engine, choices, ty, anchor, span, depth + 1);
    }
    if let Some((layout, inner)) = unboxed(engine, ty, span)? {
        return wrapper_shape(engine, value, layout, inner, anchor, span, depth);
    }
    if let Region::RecursiveCut { layout, .. } = &value.node.kind {
        return Ok(if recursive_trees::certified(engine, value, anchor) {
            Shape::Cut(*layout)
        } else {
            Shape::Plain
        });
    }
    if matches!(ty, Type::Named(..)) && recursive_trees::certified(engine, value, anchor) {
        return Ok(Shape::Cut(gate::layout_index(
            engine.program,
            ty,
            &mut engine.work,
            span,
        )?));
    }
    match (&value.node.kind, ty) {
        (Region::Product(fields), Type::Tuple(types)) if fields.len() == types.len() => {
            engine.charge(fields.len(), span)?;
            let fields = fields
                .iter()
                .zip(types)
                .map(|(v, ty)| shape(engine, v, ty, anchor, span, depth + 1))
                .collect::<Checked<Vec<_>>>()?;
            Ok(if fields.iter().all(|s| *s == Shape::Plain) {
                Shape::Plain
            } else {
                Shape::Product(fields)
            })
        }
        _ => Ok(Shape::Plain),
    }
}
/// A whole-wrapper cut remains opaque; only its transparent stored payload needs an edge.
fn wrapper_shape(
    engine: &mut Engine<'_>,
    value: &Value,
    layout: usize,
    inner: &Type,
    anchor: usize,
    span: Span,
    depth: usize,
) -> Checked<Shape> {
    if matches!(value.node.kind, Region::RecursiveCut {layout: actual, ..} if actual == layout) {
        return Ok(if recursive_trees::certified(engine, value, anchor) {
            Shape::Cut(layout)
        } else {
            Shape::Plain
        });
    }
    let child = shape(engine, value, inner, anchor, span, depth + 1)?;
    if child == Shape::Plain {
        return Ok(Shape::Plain);
    }
    engine.charge(1, span)?;
    Ok(Shape::Unboxed(layout, Box::new(child)))
}
/// Inspect only exact validated wrapper storage; the payload retains its original value identity.
fn unboxed<'a>(
    engine: &mut Engine<'a>,
    ty: &Type,
    span: Span,
) -> Checked<Option<(usize, &'a Type)>> {
    if !matches!(ty, Type::Named(..)) {
        return Ok(None);
    }
    let index = gate::layout_index(engine.program, ty, &mut engine.work, span)?;
    let layout = &engine.program.types[index];
    if layout.storage != ir::LayoutStorage::Unboxed {
        return Ok(None);
    }
    let [fields] = layout.variants.as_slice() else {
        return engine.unsupported(span);
    };
    let [inner] = fields.as_slice() else {
        return engine.unsupported(span);
    };
    Ok(Some((index, inner)))
}
/// Inactive choices confer no evidence; every reachable choice must agree on its actual origin.
fn choices_shape(
    engine: &mut Engine<'_>,
    choices: &[(Predicate, Value)],
    ty: &Type,
    anchor: usize,
    span: Span,
    depth: usize,
) -> Checked<Shape> {
    engine.charge(choices.len(), span)?;
    let mut found = None;
    for (guard, value) in choices {
        if engine
            .predicates
            .and(engine.path, *guard, &mut engine.work, span)?
            == Predicate::FALSE
        {
            continue;
        }
        let next = shape(engine, value, ty, anchor, span, depth)?;
        if found.as_ref().is_some_and(|old| *old != next) {
            return Ok(Shape::Plain);
        }
        found = Some(next);
    }
    Ok(found.unwrap_or(Shape::Plain))
}
/// A new proof namespace prevents captures or copied source node IDs from inheriting descent.
pub(super) fn input(
    engine: &mut Engine<'_>,
    ty: &Type,
    plan: Option<&Plan>,
    span: Span,
) -> Checked<Value> {
    let Some(plan) = plan else {
        return engine.fresh(ty, Some(0), span, 0);
    };
    let anchor = engine.node(Region::Empty, span)?.node.id;
    engine.tree_context = Some((plan.function, plan.parameter, anchor));
    build(engine, ty, &plan.shape, anchor, span, 0)
}
/// Only certified typed components become cuts; all other regions are fresh independent shapes.
fn build(
    engine: &mut Engine<'_>,
    ty: &Type,
    shape: &Shape,
    anchor: usize,
    span: Span,
    depth: usize,
) -> Checked<Value> {
    engine.charge(1, span)?;
    if depth >= DEPTH_LIMIT {
        return engine.unsupported(span);
    }
    match (shape, ty) {
        (Shape::Plain, _) => engine.fresh(ty, Some(0), span, depth),
        (Shape::Cut(layout), Type::Named(..)) => {
            if gate::layout_index(engine.program, ty, &mut engine.work, span)? != *layout {
                return engine.unsupported(span);
            }
            engine.recursive_cut(*layout, anchor, Some(0), span)
        }
        (Shape::Unboxed(expected, shape), _) => {
            let Some((actual, inner)) = unboxed(engine, ty, span)? else {
                return engine.unsupported(span);
            };
            if actual != *expected {
                return engine.unsupported(span);
            }
            build(engine, inner, shape, anchor, span, depth + 1)
        }
        (Shape::Product(shapes), Type::Tuple(types)) if shapes.len() == types.len() => {
            engine.charge(types.len(), span)?;
            let values = types
                .iter()
                .zip(shapes)
                .map(|(ty, shape)| build(engine, ty, shape, anchor, span, depth + 1))
                .collect::<Checked<_>>()?;
            engine.node(Region::Product(values), span)
        }
        _ => engine.unsupported(span),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn program() -> ir::Program {
        ir::Program {
            types: vec![ir::TypeLayout {
                ty: Type::Named("Tree".into(), vec![]),
                storage: ir::LayoutStorage::Tagged,
                variants: vec![vec![]],
                fields: vec![],
                variant_names: vec!["Empty".into()],
            }],
            functions: vec![],
        }
    }
    #[test]
    fn representatives_require_exact_complete_descendant_evidence() {
        let program = program();
        let mut engine = Engine::new(&program);
        let span = Span::default();
        engine.tree_context = Some((7, 0, 42));
        let child = engine.recursive_cut(0, 42, Some(0), span).unwrap();
        let other = engine.recursive_cut(0, 43, Some(1), span).unwrap();
        assert!(
            prepare(
                &mut engine,
                &child,
                &Type::Named("Tree".into(), vec![]),
                span
            )
            .unwrap()
            .is_some()
        );
        assert!(
            prepare(
                &mut engine,
                &child.partial(),
                &Type::Named("Tree".into(), vec![]),
                span
            )
            .unwrap()
            .is_none()
        );
        assert!(
            prepare(
                &mut engine,
                &other,
                &Type::Named("Tree".into(), vec![]),
                span
            )
            .unwrap()
            .is_none()
        );
        let mixed = engine
            .node(
                Region::Choice(vec![
                    (Predicate::TRUE, child.clone()),
                    (Predicate::TRUE, other.clone()),
                ]),
                span,
            )
            .unwrap();
        assert!(
            prepare(
                &mut engine,
                &mixed,
                &Type::Named("Tree".into(), vec![]),
                span
            )
            .unwrap()
            .is_none()
        );
        let inactive = engine
            .node(
                Region::Choice(vec![(Predicate::TRUE, child), (Predicate::FALSE, other)]),
                span,
            )
            .unwrap();
        assert!(
            prepare(
                &mut engine,
                &inactive,
                &Type::Named("Tree".into(), vec![]),
                span
            )
            .unwrap()
            .is_some()
        );
    }
    #[test]
    fn representative_shape_uses_the_shared_budget_before_allocation() {
        let program = program();
        let mut engine = Engine::new(&program);
        let span = Span::default();
        engine.tree_context = Some((7, 0, 42));
        let child = engine.recursive_cut(0, 42, Some(0), span).unwrap();
        engine.work = WORK_LIMIT;
        let error = prepare(
            &mut engine,
            &child,
            &Type::Named("Tree".into(), vec![]),
            span,
        )
        .err()
        .expect("bounded proof");
        assert!(error.message.contains("work limit"));
    }
    /// Equal payloads do not permit substituting a different nominal wrapper in a proof plan.
    #[test]
    fn unboxed_representatives_validate_exact_wrappers_and_layout_shape() {
        let mut program = program();
        for name in ["Wrapped", "Other"] {
            program.types.push(ir::TypeLayout {
                ty: Type::Named(name.into(), vec![]),
                storage: ir::LayoutStorage::Unboxed,
                variants: vec![vec![Type::Named("Tree".into(), vec![])]],
                fields: vec![],
                variant_names: vec![],
            });
        }
        let span = Span::default();
        let mut engine = Engine::new(&program);
        engine.tree_context = Some((7, 0, 42));
        let child = engine.recursive_cut(0, 42, Some(0), span).unwrap();
        let wrapped = Type::Named("Wrapped".into(), vec![]);
        let plan = prepare(&mut engine, &child, &wrapped, span)
            .unwrap()
            .unwrap();
        let mut proof = Engine::new(&program);
        let value = input(&mut proof, &wrapped, Some(&plan), span).unwrap();
        assert!(matches!(
            value.node.kind,
            Region::RecursiveCut { layout: 0, .. }
        ));
        assert!(
            input(
                &mut proof,
                &Type::Named("Other".into(), vec![]),
                Some(&plan),
                span
            )
            .is_err()
        );
        program.types[1].variants[0].push(Type::Int);
        let mut invalid = Engine::new(&program);
        invalid.tree_context = Some((7, 0, 42));
        assert!(prepare(&mut invalid, &child, &wrapped, span).is_err());
    }
    /// Flat source identities cannot hide an unbounded chain of transparent storage edges.
    #[test]
    fn wrapper_chains_share_the_existing_depth_budget() {
        let mut program = program();
        let mut inner = Type::Named("Tree".into(), vec![]);
        for index in 0..129 {
            let ty = Type::Named(format!("Wrapper{index}"), vec![]);
            program.types.push(ir::TypeLayout {
                ty: ty.clone(),
                storage: ir::LayoutStorage::Unboxed,
                variants: vec![vec![inner]],
                fields: vec![],
                variant_names: vec![],
            });
            inner = ty;
        }
        let mut engine = Engine::new(&program);
        engine.tree_context = Some((7, 0, 42));
        let span = Span::default();
        let child = engine.recursive_cut(0, 42, Some(0), span).unwrap();
        assert!(prepare(&mut engine, &child, &inner, span).is_err());
        assert!(engine.work < WORK_LIMIT);
    }
}
