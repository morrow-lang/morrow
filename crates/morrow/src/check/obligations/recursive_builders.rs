//! Finite recursive builders preserve complete input duties without promising to handle them.
use super::*;
/// Nominal storage is a finite graph; arbitrary fresh data contains no hidden callable contracts.
pub(super) fn closed_data(
    program: &ir::Program,
    ty: &Type,
    results: bool,
    work: &mut usize,
    span: Span,
) -> Checked<bool> {
    closed_data_mode(program, ty, results, false, work, span)
}
/// Template payloads stay symbolic; concrete specializations must prove their actual duties again.
pub(super) fn template_output(
    program: &ir::Program,
    ty: &Type,
    work: &mut usize,
    span: Span,
) -> Checked<bool> {
    closed_data_mode(program, ty, true, true, work, span)
}
fn closed_data_mode(
    program: &ir::Program,
    ty: &Type,
    results: bool,
    generics: bool,
    work: &mut usize,
    span: Span,
) -> Checked<bool> {
    let mut pending = vec![ty];
    let mut layouts = HashSet::new();
    while let Some(ty) = pending.pop() {
        gate::type_cost(ty, work, span)?;
        match ty {
            Type::Named(..) => {
                let index = gate::layout_index(program, ty, work, span)?;
                if !layouts.insert(index) {
                    continue;
                }
                for fields in &program.types[index].variants {
                    charge(work, fields.len(), span)?;
                    pending.extend(fields);
                }
            }
            Type::List(inner) | Type::Option(inner) | Type::Pid(inner) | Type::ChildKey(inner) | Type::RootFunction(inner) => pending.push(inner),
            Type::Result(ok, error) if results => {
                charge(work, 2, span)?;
                pending.extend([ok.as_ref(), error.as_ref()]);
            }
            Type::Map(key, value) => {
                charge(work, 2, span)?;
                pending.extend([key.as_ref(), value.as_ref()]);
            }
            Type::Tuple(fields) | Type::Union(fields) => {
                charge(work, fields.len(), span)?;
                pending.extend(fields);
            }
            Type::Generic(_) if generics => {}
            Type::Unit
            | Type::Int
            | Type::Float
            | Type::Bool
            | Type::String
            | Type::Range
            | Type::Native(_) => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}
/// A backedge preserves the full input family and may add independently accountable fresh values.
pub(super) fn apply(
    engine: &mut Engine<'_>,
    function: &ir::Function,
    indices: &[usize],
    args: &[Value],
    span: Span,
) -> Checked<Value> {
    engine.charge(indices.len(), span)?;
    let retained = indices
        .iter()
        .map(|index| {
            args.get(*index)
                .cloned()
                .ok_or_else(|| Diagnostic::new(span, "missing recursive builder input"))
        })
        .collect::<Checked<Vec<_>>>()?;
    if matches!(function.return_type, Type::Named(..)) {
        let layout = gate::layout_index(
            engine.program,
            &function.return_type,
            &mut engine.work,
            span,
        )?;
        let origin = engine.origin(None, span)?;
        engine.origins[origin].aggregate = true;
        return engine.node(
            Region::RecursiveCut {
                layout,
                origin: Some(origin),
                retained,
            },
            span,
        );
    }
    list_output(engine, function, indices, &retained, span)
}
/// Preserve each actual element shape and only those nonempty facts justified by returned duties.
fn list_output(
    engine: &mut Engine<'_>,
    function: &ir::Function,
    indices: &[usize],
    retained: &[Value],
    span: Span,
) -> Checked<Value> {
    let fresh = engine.fresh(&function.return_type, None, span, 0)?;
    let Region::List {
        items, nonempty, ..
    } = &fresh.node.kind
    else {
        return engine.unsupported(span);
    };
    engine.charge(items.len().saturating_add(retained.len()), span)?;
    let mut output = items.clone();
    let mut nonempty = *nonempty;
    for (index, input) in indices.iter().zip(retained) {
        engine.charge(index.saturating_add(1), span)?;
        let ty = &function
            .params
            .iter()
            .chain(&function.captures)
            .nth(*index)
            .ok_or_else(|| Diagnostic::new(span, "missing recursive builder parameter"))?
            .ty;
        let (item, present) = if ty == &function.return_type {
            (
                substitute::Substitution::family(engine, input, false, span, 0)?,
                duty_present(engine, input, span)?,
            )
        } else {
            (input.clone(), duty_present(engine, input, span)?)
        };
        output.push(item);
        nonempty = engine
            .predicates
            .or(nonempty, present, &mut engine.work, span)?;
    }
    engine.node(
        Region::List {
            items: output,
            exact: false,
            nonempty,
        },
        span,
    )
}
/// Only present retained duties imply a nonempty output; an absent Option may validly produce [].
fn duty_present(engine: &mut Engine<'_>, input: &Value, span: Span) -> Checked<Predicate> {
    let mut present = Predicate::FALSE;
    for (origin, selected) in engine.origins_of(input, false, span)? {
        engine.charge(1, span)?;
        // A recursive cut requires complete treatment even when its concrete subtree is Empty.
        // Its aggregate origin therefore says nothing about actual Result or element existence.
        if engine.origins[origin].aggregate {
            continue;
        }
        let exists = engine.predicates.and(
            engine.origins[origin].exists,
            selected,
            &mut engine.work,
            span,
        )?;
        present = engine
            .predicates
            .or(present, exists, &mut engine.work, span)?;
    }
    Ok(present)
}
/// Every ordinary return must carry the original accumulator, not just a selected element or subset.
pub(super) fn verify(indices: &[usize], summary: &mut Summary, span: Span) -> Checked<()> {
    for origin in &summary.origins {
        charge(&mut summary.work, 1, span)?;
        charge(&mut summary.work, indices.len(), span)?;
        if !origin.input.is_some_and(|index| indices.contains(&index)) {
            continue;
        }
        let required =
            summary
                .predicates
                .and(origin.exists, summary.exits, &mut summary.work, span)?;
        if !summary
            .predicates
            .implies(required, origin.returned, &mut summary.work, span)?
        {
            return Err(Diagnostic::new(
                span,
                "Result obligation recursive builder does not retain its complete input",
            ));
        }
    }
    Ok(())
}

/// Builders may embed nominal payloads, List elements, or complete List accumulators.
pub(super) fn inputs(
    program: &ir::Program,
    function: &ir::Function,
    work: &mut usize,
) -> Checked<Option<Vec<usize>>> {
    let span = function.body.span;
    let supported = match &function.return_type {
        Type::List(_) => true,
        ty @ Type::Named(..) => recursive_trees::storage(program, ty, work, span)?,
        _ => false,
    };
    if !supported || !closed_data(program, &function.return_type, true, work, span)? {
        return Ok(None);
    }
    let mut indices = Vec::new();
    for (index, param) in function.params.iter().chain(&function.captures).enumerate() {
        if !closed_data(program, &param.ty, true, work, span)? {
            return Ok(None);
        }
        if gate::contains(program, &param.ty, work, span)? {
            if let Type::List(element) = &function.return_type
                && param.ty != function.return_type
                && &param.ty != element.as_ref()
            {
                return Ok(None);
            }
            charge(work, 1, span)?;
            indices.push(index);
        }
    }
    Ok((!indices.is_empty()).then_some(indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program() -> ir::Program {
        let source = "type Tree:\n    Empty\n    Leaf(Result(Int,String))\n    Branch(Tree,Tree)\nfn grow(depth:Int,tree:Tree)->Tree:if depth<=0:tree else:grow(depth-1,Branch(tree,Empty))\nfn main():()\n";
        let ast = crate::parse::parse(source).unwrap();
        crate::check::pipeline_mode(&ast, |_, _, _| Ok(()), false)
            .unwrap()
            .0
    }
    #[test]
    fn builder_layout_walks_share_the_proof_work_limit() {
        let program = program();
        let ty = Type::Named("Tree".into(), vec![]);
        assert!(closed_data(&program, &ty, true, &mut 0, Span::default()).unwrap());
        assert!(!closed_data(&program, &ty, false, &mut 0, Span::default()).unwrap());
        let error =
            closed_data(&program, &ty, true, &mut (WORK_LIMIT - 1), Span::default()).unwrap_err();
        assert!(error.message.contains("work limit"), "{error:?}");
    }
    #[test]
    fn aggregate_obligations_do_not_establish_actual_result_presence() {
        let program = program();
        let mut engine = Engine::new(&program);
        let span = Span::default();
        let cut = engine.recursive_cut(0, 0, Some(0), span).unwrap();
        assert_eq!(
            duty_present(&mut engine, &cut, span).unwrap(),
            Predicate::FALSE
        );
        let list = engine
            .node(
                Region::List {
                    items: vec![cut],
                    exact: true,
                    nonempty: Predicate::TRUE,
                },
                span,
            )
            .unwrap();
        assert_eq!(
            duty_present(&mut engine, &list, span).unwrap(),
            Predicate::FALSE
        );
        let result = engine
            .fresh(
                &Type::Result(Box::new(Type::Int), Box::new(Type::String)),
                Some(1),
                span,
                0,
            )
            .unwrap();
        assert_eq!(
            duty_present(&mut engine, &result, span).unwrap(),
            Predicate::TRUE
        );
    }
    #[test]
    fn retained_cut_coverage_has_no_outer_tag_or_structural_descent_credit() {
        let program = program();
        let function = program.functions.iter().find(|f| f.name == "grow").unwrap();
        let span = Span::default();
        let mut engine = Engine::new(&program);
        let ty = Type::Named("Tree".into(), vec![]);
        let original = engine.fresh(&ty, Some(1), span, 0).unwrap();
        let count = engine.origins_of(&original, false, span).unwrap().len();
        let depth = engine.fresh(&Type::Int, None, span, 0).unwrap();
        let output = apply(&mut engine, function, &[1], &[depth, original], span).unwrap();
        assert_eq!(
            engine.origins_of(&output, false, span).unwrap().len(),
            count + 1
        );
        assert!(engine.origins_of(&output, true, span).unwrap().is_empty());
        assert!(!engine.nominal_roots.contains_key(&output.node.id));
        engine.work = WORK_LIMIT;
        assert!(
            engine
                .origins_of(&output, false, span)
                .unwrap_err()
                .message
                .contains("work limit")
        );
    }
}
