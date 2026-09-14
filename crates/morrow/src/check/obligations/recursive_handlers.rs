//! Structural list induction permits a handler contract only on a strict suffix of its own input.
use super::*;
/// Select one Result-bearing list input; all other interfaces must carry neither duties nor callbacks.
pub(super) fn candidate(
    program: &ir::Program,
    function: &ir::Function,
    work: &mut usize,
) -> Checked<Option<usize>> {
    let span = function.body.span;
    if !recursive::closed_type(&function.return_type, false, work, span)? {
        return Ok(None);
    }
    let mut candidate = None;
    for (index, param) in function.params.iter().chain(&function.captures).enumerate() {
        if matches!(param.ty, Type::List(_)) && gate::contains(program, &param.ty, work, span)? {
            if candidate.replace(index).is_some() {
                return Ok(None);
            }
        } else if !recursive::closed_type(&param.ty, false, work, span)? {
            return Ok(None);
        }
    }
    Ok(candidate)
}
/// A strict suffix is smaller on every finite list; unrelated or unchanged families cannot self-prove.
pub(super) fn apply(
    engine: &mut Engine<'_>,
    function: &ir::Function,
    index: usize,
    args: &[Value],
    span: Span,
) -> Checked<Value> {
    let actual = args
        .get(index)
        .ok_or_else(|| Diagnostic::new(span, "missing recursive list input"))?;
    let input = engine
        .inputs
        .get(index)
        .ok_or_else(|| Diagnostic::new(span, "missing recursive source input"))?;
    let decreases = engine
        .sequence_offsets
        .get(&actual.node.id)
        .is_some_and(|(base, offset)| *base == input.node.id && *offset > 0);
    if engine.source_function != Some(function.id.0) || !decreases {
        return Err(Diagnostic::new(
            span,
            "Result obligation recursive handler requires a strict suffix of its own input",
        ));
    }
    engine.dispose(actual, false, false, span)?;
    engine.fresh(&function.return_type, None, span, 0)
}
/// The inductive body must handle all input layers; inspecting only the head or its outer tag fails.
pub(super) fn verify(index: usize, summary: &mut Summary, span: Span) -> Checked<()> {
    for origin in &summary.origins {
        charge(&mut summary.work, 1, span)?;
        if origin.input != Some(index) {
            continue;
        }
        let required =
            summary
                .predicates
                .and(origin.exists, summary.exits, &mut summary.work, span)?;
        if !summary
            .predicates
            .implies(required, origin.handled, &mut summary.work, span)?
        {
            return Err(Diagnostic::new(
                span,
                "Result obligation recursive list body does not handle every element and payload",
            ));
        }
    }
    Ok(())
}
