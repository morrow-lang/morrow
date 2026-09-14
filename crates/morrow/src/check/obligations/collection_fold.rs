//! Fold uses an accumulator transfer invariant rather than assuming callbacks consume arguments.
use super::*;
impl Engine<'_> {
    /// Zero elements return the original accumulator; nonempty folds preserve its inductive contract.
    pub(super) fn collection_fold(
        &mut self,
        values: &[Value],
        source: &[ir::Expr],
        result: &Type,
        span: Span,
    ) -> Checked<Value> {
        let [list, initial, callback] = values else {
            return self.unsupported(span);
        };
        let [list_expr, _, callback_expr] = source else {
            return self.unsupported(span);
        };
        let Type::List(item) = &list_expr.ty else {
            return self.unsupported(span);
        };
        let nonempty = self.list_nonempty(list, span, 0)?;
        if nonempty == Predicate::FALSE {
            return Ok(initial.clone());
        }
        let summary = self.fold_callback(callback, &callback_expr.ty, item, result, span)?;
        self.work = summary.work;
        let family = substitute::Substitution::family(self, list, false, span, 0)?;
        let parent = self.path;
        self.path = self
            .predicates
            .and(parent, nonempty, &mut self.work, span)?;
        let output = substitute::Substitution::fold(&summary).apply(
            self,
            &[initial.clone(), family, callback.clone()],
            span,
        )?;
        self.path = parent;
        if nonempty == Predicate::TRUE {
            return Ok(output);
        }
        let empty = self.predicates.not(nonempty, &mut self.work, span)?;
        self.node(
            Region::Choice(vec![(empty, initial.clone()), (nonempty, output)]),
            span,
        )
    }
    /// The representative callback proves a general old accumulator, never merely the initial value.
    fn fold_callback(
        &mut self,
        callback: &Value,
        callable_type: &Type,
        item: &Type,
        accumulator: &Type,
        span: Span,
    ) -> Checked<Summary> {
        let mut engine = Engine::new(self.program);
        engine.work = self.work;
        engine.mode = self.mode;
        engine.summaries = self.summaries;
        engine.relevance = self.relevance;
        engine.effect_cache = self.effect_cache.clone();
        let accumulator_value = engine.fresh(accumulator, Some(0), span, 0)?;
        let item = engine.fresh(item, Some(1), span, 0)?;
        let shape = engine.effect_shape(callback, span, 0)?;
        let callable = engine.fresh_shaped(callable_type, Some(2), Some(&shape), span, 0)?;
        engine.inputs = vec![accumulator_value.clone(), item.clone(), callable.clone()];
        engine.used_inputs.extend([0, 1, 2]);
        let output = engine.invoke(&callable, &[accumulator_value, item], accumulator, span, 0)?;
        engine.exit(&output, span, 0)?;
        let output = engine.output(span)?;
        engine.finish(output)
    }
}
