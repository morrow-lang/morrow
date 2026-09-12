//! Searches validate callback-owned effects but cannot acknowledge an unvisited suffix.
use super::*;
impl Engine<'_> {
    /// Even a scalar-returning predicate may create Results internally and must be instantiated.
    pub(super) fn collection_search(
        &mut self,
        builtin: ir::Builtin,
        values: &[Value],
        source: &[ir::Expr],
        result: &Type,
        span: Span,
    ) -> Checked<Value> {
        let [list, callback] = values else {
            return self.unsupported(span);
        };
        let [list_expr, callback_expr] = source else {
            return self.unsupported(span);
        };
        let Type::List(item) = &list_expr.ty else {
            return self.unsupported(span);
        };
        let summary =
            self.collection_callback(callback, &callback_expr.ty, (item, &Type::Bool), None, span)?;
        self.work = summary.work;
        let nonempty = self.list_nonempty(list, span, 0)?;
        let parent = self.path;
        let family = substitute::Substitution::family(self, list, false, span, 0)?;
        if nonempty != Predicate::FALSE {
            self.path = self
                .predicates
                .and(parent, nonempty, &mut self.work, span)?;
            substitute::Substitution::search(&summary).apply(
                self,
                &[family.clone(), callback.clone()],
                span,
            )?;
        }
        self.path = parent;
        if builtin == ir::Builtin::ListFind {
            return self.search_result(family, nonempty, span);
        }
        if nonempty == Predicate::FALSE {
            return self.node(
                Region::Scalar(Key::Bool(builtin == ir::Builtin::ListAll)),
                span,
            );
        }
        self.fresh(result, None, span, 0)
    }
    /// Finding one element returns only a partial view of the original quantified family.
    fn search_result(&mut self, family: Value, nonempty: Predicate, span: Span) -> Checked<Value> {
        if nonempty == Predicate::FALSE {
            return self.sum_value(false, 1, None, span);
        }
        let found = self.predicates.variable(&mut self.work, span)?;
        let found = self.predicates.and(nonempty, found, &mut self.work, span)?;
        let missing = self.predicates.not(found, &mut self.work, span)?;
        self.node(
            Region::Sum {
                origin: None,
                tag: None,
                guards: vec![found, missing],
                variants: vec![vec![family.partial()], vec![]],
            },
            span,
        )
    }
}
