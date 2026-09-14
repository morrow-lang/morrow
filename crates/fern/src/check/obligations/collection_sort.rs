//! Sorting proves the comparator over two representative elements; the output is a permutation.
use super::*;
impl Engine<'_> {
    /// The comparator returns an `Ordering`, yet it may still create and must handle Results
    /// internally, so it is instantiated once. Element positions after sorting are unknown, and
    /// Result-bearing elements therefore fail closed instead of receiving invented positions.
    pub(super) fn collection_sort(
        &mut self,
        values: &[Value],
        source: &[ir::Expr],
        result: &Type,
        span: Span,
    ) -> Checked<Value> {
        let [_, callback] = values else {
            return self.unsupported(span);
        };
        let [list_expr, callback_expr] = source else {
            return self.unsupported(span);
        };
        let Type::List(item) = &list_expr.ty else {
            return self.unsupported(span);
        };
        let Type::Function(_, ordering) = &callback_expr.ty else {
            return self.unsupported(span);
        };
        let summary =
            self.comparator_callback(callback, &callback_expr.ty, item, ordering, span)?;
        self.work = summary.work;
        if super::gate::contains(self.program, result, &mut self.work, span)? {
            return self.unsupported(span);
        }
        self.fresh(result, None, span, 0)
    }
    /// Both comparator inputs are general elements; neither is the other or a literal position.
    fn comparator_callback(
        &mut self,
        callback: &Value,
        callable_type: &Type,
        item: &Type,
        ordering: &Type,
        span: Span,
    ) -> Checked<Summary> {
        let mut engine = Engine::new(self.program);
        engine.work = self.work;
        engine.mode = self.mode;
        engine.summaries = self.summaries;
        engine.relevance = self.relevance;
        engine.effect_cache = self.effect_cache.clone();
        let left = engine.fresh(item, Some(0), span, 0)?;
        let right = engine.fresh(item, Some(1), span, 0)?;
        let shape = engine.effect_shape(callback, span, 0)?;
        let callable = engine.fresh_shaped(callable_type, Some(2), Some(&shape), span, 0)?;
        engine.inputs = vec![left.clone(), right.clone(), callable.clone()];
        engine.used_inputs.extend([0, 1, 2]);
        let output = engine.invoke(&callable, &[left, right], ordering, span, 0)?;
        engine.exit(&output, span, 0)?;
        let output = engine.output(span)?;
        engine.finish(output)
    }
}
