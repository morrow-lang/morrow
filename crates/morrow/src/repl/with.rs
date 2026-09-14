//! Sequential Result bindings with concretely typed, outer-scope error handlers.
use super::*;

impl Machine {
    /// Restore all success and error bindings even when the block returns or propagates.
    pub(super) fn with_block(
        &mut self,
        steps: &[ir::WithStep],
        body: &ir::Expr,
        handlers: &[ir::WithHandler],
    ) -> Eval<Value> {
        let outer = self.locals.clone();
        let result = self.with_steps(steps, body, handlers, &outer);
        self.locals = outer;
        result
    }

    /// Stop at the first failed step; no later initializer or success body executes.
    fn with_steps(
        &mut self,
        steps: &[ir::WithStep],
        body: &ir::Expr,
        handlers: &[ir::WithHandler],
        outer: &HashMap<usize, Value>,
    ) -> Eval<Value> {
        for step in steps {
            let value = self.expression(&step.value)?;
            let Value::Sum(tag, fields) = &value else {
                return Err(fault("with step must produce Result"));
            };
            if fields.len() != 1 {
                return Err(fault("invalid Result payload"));
            }
            match tag {
                0 => {
                    if !self.pattern(&step.pattern, &fields[0])? {
                        return Err(fault("with binding must match every success"));
                    }
                }
                1 => {
                    let Some(index) = step.error_handler else {
                        return Err(Failure::Return(value));
                    };
                    let handler = handlers
                        .get(index)
                        .ok_or_else(|| fault("invalid with error handler"))?;
                    self.locals = outer.clone();
                    self.locals.insert(handler.error.id.0, fields[0].clone());
                    return self.expression(&handler.body);
                }
                _ => return Err(fault("invalid Result tag")),
            }
        }
        self.expression(body)
    }
}
