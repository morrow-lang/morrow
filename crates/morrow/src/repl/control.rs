//! Function-owned abrupt exits and bounded, dynamically registered cleanup.
use super::*;

impl Machine {
    /// Reserve a separate entry-wide cleanup budget, including nested cleanup calls.
    pub(super) fn charge_step(&mut self) -> Eval<()> {
        self.charge_steps(1)
    }

    /// Reserve aggregate work before copying pattern tails or executing a node.
    pub(super) fn charge_steps(&mut self, count: usize) -> Eval<()> {
        let exhausted = if self.cleanup_depth == 0 {
            self.steps = self.steps.saturating_add(count);
            self.steps > 100_000
        } else {
            self.cleanup_steps = self.cleanup_steps.saturating_add(count);
            self.cleanup_steps > 10_000
        };
        if exhausted || self.depth >= 128 {
            Err(fault("interactive evaluation limit exceeded"))
        } else {
            Ok(())
        }
    }

    /// Capture immutable locals now; execute the captured expression only at function exit.
    pub(super) fn defer(&mut self, value: &ir::Expr) -> Eval<Value> {
        if self.defers.len() >= 4096 {
            return Err(fault("interactive deferred cleanup limit exceeded"));
        }
        let value = self.expression(value)?;
        if !matches!(value, Value::Closure(_)) {
            return Err(fault("invalid deferred cleanup"));
        }
        self.defers.push(value);
        Ok(Value::Unit)
    }

    /// Drain in reverse registration order, preserving the first evaluation failure.
    pub(super) fn finish(&mut self, mut result: Eval<Value>) -> Eval<Value> {
        self.cleanup_depth += 1;
        while let Some(cleanup) = self.defers.pop() {
            let outcome = self.invoke(&cleanup, Vec::new());
            let failure = match outcome {
                Ok(Value::Unit) => None,
                Ok(_) => Some(fault("deferred cleanup must return Unit")),
                Err(error) => Some(error),
            };
            if let Some(error) = failure
                && !matches!(result, Err(Failure::Message(_) | Failure::JsonLimit))
            {
                result = Err(error);
            }
        }
        self.cleanup_depth -= 1;
        result
    }

    /// Keep success binders in the current scope; failed matching discards partial binders.
    pub(super) fn let_else(
        &mut self,
        binding: &ir::Pattern,
        value: &ir::Expr,
        alternative: &ir::Expr,
    ) -> Eval<Value> {
        let value = self.expression(value)?;
        let previous = self.locals.clone();
        if self.pattern(binding, &value)? {
            Ok(Value::Unit)
        } else {
            self.locals = previous;
            self.expression(alternative)?;
            Err(fault("let-else alternative must leave the function"))
        }
    }
}
