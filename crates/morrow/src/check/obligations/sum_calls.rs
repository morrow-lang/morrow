//! Sum combinators inspect only their outer tag and preserve callback/payload responsibilities.
use super::*;
impl Engine<'_> {
    /// Evaluate the callback only on its selected variant and retain guarded output provenance.
    pub(super) fn sum_call(
        &mut self,
        builtin: ir::Builtin,
        value: &Value,
        callback: &Value,
        result: &Type,
        span: Span,
    ) -> Checked<Value> {
        let parent = self.path;
        self.dispose(value, true, false, span)?;
        let mut outputs = Vec::new();
        for tag in 0..2 {
            let (guard, fields) = self.variant(value, tag, span, 0)?;
            self.path = self.predicates.and(parent, guard, &mut self.work, span)?;
            if self.path == Predicate::FALSE {
                continue;
            }
            let output = self.sum_branch(builtin, tag, &fields, callback, result, span)?;
            outputs.push((self.path, output));
        }
        self.path = Predicate::FALSE;
        for (guard, _) in &outputs {
            self.path = self
                .predicates
                .or(self.path, *guard, &mut self.work, span)?;
        }
        self.node(Region::Choice(outputs), span)
    }
    /// Mapping constructs a new outer Result; chaining returns the callback's existing Result.
    fn sum_branch(
        &mut self,
        builtin: ir::Builtin,
        tag: usize,
        fields: &[Value],
        callback: &Value,
        result: &Type,
        span: Span,
    ) -> Checked<Value> {
        use ir::Builtin::*;
        let callback_tag = usize::from(builtin == ResultUnwrapOrElse);
        if tag == callback_tag {
            let value = fields
                .first()
                .ok_or_else(|| Diagnostic::new(span, "missing sum callback payload"))?;
            let output_ty = match (builtin, result) {
                (OptionMap, Type::Option(inner)) | (ResultMap, Type::Result(inner, _)) => {
                    inner.as_ref()
                }
                (ResultAndThen | ResultUnwrapOrElse, _) => result,
                _ => return self.unsupported(span),
            };
            let output = self.invoke(callback, std::slice::from_ref(value), output_ty, span, 0)?;
            if matches!(builtin, OptionMap | ResultMap) {
                self.sum_value(builtin == ResultMap, 0, Some(output), span)
            } else {
                Ok(output)
            }
        } else if builtin == ResultUnwrapOrElse {
            fields
                .first()
                .cloned()
                .ok_or_else(|| Diagnostic::new(span, "missing successful Result payload"))
        } else {
            self.sum_value(builtin != OptionMap, 1, fields.first().cloned(), span)
        }
    }
    /// A fresh outer construction is independent from every already-produced payload duty.
    pub(super) fn sum_value(
        &mut self,
        result: bool,
        tag: usize,
        value: Option<Value>,
        span: Span,
    ) -> Checked<Value> {
        let origin = if result {
            Some(self.origin(None, span)?)
        } else {
            None
        };
        let mut guards = vec![Predicate::FALSE; 2];
        guards[tag] = Predicate::TRUE;
        let mut variants = vec![vec![]; 2];
        variants[tag] = value.into_iter().collect();
        self.node(
            Region::Sum {
                origin,
                tag: Some(tag),
                guards,
                variants,
            },
            span,
        )
    }
}
