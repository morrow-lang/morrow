//! Snapshot iteration over immutable collections and lazy full-width ranges.
use super::*;

impl Machine {
    /// Evaluate and retain endpoints without allocating one element per integer.
    pub(super) fn range(
        &mut self,
        start: &ir::Expr,
        end: &ir::Expr,
        inclusive: bool,
    ) -> Eval<Value> {
        let start = self.expression(start)?;
        let end = self.expression(end)?;
        match (start, end) {
            (Value::Int(start), Value::Int(end)) => Ok(Value::Range(start, end, inclusive)),
            _ => Err(fault("range endpoints must be Int")),
        }
    }

    /// Preserve list indexes and full semantic element values in ordinary tuples.
    pub(super) fn enumerate(&mut self, items: &[Value]) -> Eval<Value> {
        if items.len() > 65_536 {
            return Err(fault("interactive list limit exceeded"));
        }
        let values = items
            .iter()
            .enumerate()
            .map(|(index, value)| {
                Value::Sum(0, Rc::new(vec![Value::Int(index as i64), value.clone()]))
            })
            .collect();
        Ok(Value::List(Rc::new(values)))
    }

    /// Read the iterable once, keeping its immutable snapshot alive for the loop.
    pub(super) fn for_each(
        &mut self,
        binding: &ir::Pattern,
        iterable: &ir::Expr,
        body: &ir::Expr,
    ) -> Eval<Value> {
        match self.expression(iterable)? {
            Value::List(items) => self.items(items.iter().cloned(), binding, body),
            Value::Map(entries) => {
                let items = entries
                    .iter()
                    .map(|(key, value)| Value::Sum(0, Rc::new(vec![key.clone(), value.clone()])));
                self.items(items, binding, body)
            }
            Value::Range(start, end, inclusive) => {
                self.range_items(start, end, inclusive, binding, body)
            }
            _ => Err(fault("value is not iterable")),
        }
    }

    /// Consume only the nearest loop's break/continue; returns propagate to the function.
    fn items(
        &mut self,
        items: impl Iterator<Item = Value>,
        binding: &ir::Pattern,
        body: &ir::Expr,
    ) -> Eval<Value> {
        for item in items {
            match self.iteration(binding, &item, body) {
                Ok(_) | Err(Failure::Continue) => {}
                Err(Failure::Break) => break,
                Err(error) => return Err(error),
            }
        }
        Ok(Value::Unit)
    }

    /// Check the final inclusive endpoint before incrementing, including Int::MAX.
    fn range_items(
        &mut self,
        mut current: i64,
        end: i64,
        inclusive: bool,
        binding: &ir::Pattern,
        body: &ir::Expr,
    ) -> Eval<Value> {
        while current < end || (inclusive && current == end) {
            match self.iteration(binding, &Value::Int(current), body) {
                Ok(_) | Err(Failure::Continue) => {}
                Err(Failure::Break) => break,
                Err(error) => return Err(error),
            }
            if current == end {
                break;
            }
            current += 1;
        }
        Ok(Value::Unit)
    }

    /// Restore iteration locals on every path while retaining function-owned deferred captures.
    fn iteration(&mut self, binding: &ir::Pattern, value: &Value, body: &ir::Expr) -> Eval<Value> {
        let previous = self.locals.clone();
        let result = if self.pattern(binding, value)? {
            self.expression(body)
        } else {
            Err(fault("for binding must match every item"))
        };
        self.locals = previous;
        result
    }
}
