//! Complete traversal proves one universal element without unrolling collection lengths.
use super::*;
impl Engine<'_> {
    /// Isolate a representative iteration; zero iterations never handle an unrelated outer value.
    pub(super) fn iteration(
        &mut self,
        pattern: &ir::Pattern,
        iterable: &ir::Expr,
        body: &ir::Expr,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let collection = self.expression(iterable, depth)?;
        gate::type_cost(&iterable.ty, &mut self.work, span)?;
        let item_type = match &iterable.ty {
            Type::List(item) => item.as_ref().clone(),
            Type::Range => Type::Int,
            Type::Map(key, value) => {
                Type::Tuple(vec![key.as_ref().clone(), value.as_ref().clone()])
            }
            _ => return self.unsupported(span),
        };
        let captures = self.iteration_captures(body, span)?;
        let item = self.iteration_family(&collection, &iterable.ty, span)?;
        let plan = tree_iteration::prepare(self, &item, &item_type, span)?;
        let summary = self.iteration_summary(pattern, &item_type, body, &captures, &plan, span)?;
        self.work = summary.work;
        self.charge(captures.len().saturating_add(1), span)?;
        let mut args = vec![item];
        args.extend(captures.iter().map(|(_, _, value)| value.clone()));
        let parent = self.path;
        let nonempty = self.iteration_nonempty(&collection, &iterable.ty, span)?;
        self.path = self
            .predicates
            .and(parent, nonempty, &mut self.work, span)?;
        let mut substitution = substitute::Substitution::traversal(&summary);
        substitution.apply(self, &args, span)?;
        let returning = substitution.iteration_returns(self, span, depth)?;
        let ordinary = self.predicates.not(returning, &mut self.work, span)?;
        self.path = self
            .predicates
            .and(parent, ordinary, &mut self.work, span)?;
        self.node(Region::Empty, span)
    }
    /// Empty collections cannot execute returns or create callback-local obligations.
    fn iteration_nonempty(&mut self, value: &Value, ty: &Type, span: Span) -> Checked<Predicate> {
        if matches!(ty, Type::List(_)) {
            return self.list_nonempty(value, span, 0);
        }
        if matches!(ty, Type::Map(..)) {
            return self.map_nonempty(value, span, 0);
        }
        self.predicates.variable(&mut self.work, span)
    }
    /// Continue closes one iteration; break additionally prevents any complete-traversal guarantee.
    pub(super) fn iteration_exit(
        &mut self,
        breaking: bool,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let Some(previous) = self.iteration_breaks else {
            return self.unsupported(span);
        };
        if breaking {
            self.iteration_breaks =
                Some(
                    self.predicates
                        .or(previous, self.path, &mut self.work, span)?,
                );
        }
        let empty = self.node(Region::Empty, span)?;
        self.exit(&empty, span, depth)?;
        Ok(empty)
    }
    /// Collect only parent locals; internal bindings are created by the representative body itself.
    fn iteration_captures<'a>(
        &mut self,
        body: &'a ir::Expr,
        span: Span,
    ) -> Checked<Vec<(usize, &'a Type, Value)>> {
        let mut pending = vec![body];
        let mut captures = BTreeMap::new();
        while let Some(expr) = pending.pop() {
            self.charge(1, span)?;
            if let ir::ExprKind::Local(id) = expr.kind
                && let Some(value) = self.locals.get(&id.0)
            {
                captures.insert(id.0, (&expr.ty, value.clone()));
            }
            for child in ir::children(expr) {
                self.charge(1, span)?;
                pending.push(child);
            }
        }
        self.charge(captures.len(), span)?;
        Ok(captures
            .into_iter()
            .map(|(id, (ty, value))| (id, ty, value))
            .collect())
    }
    /// Fresh iteration input regions represent every element, keeping capture types and callables exact.
    fn iteration_summary(
        &mut self,
        pattern: &ir::Pattern,
        item: &Type,
        body: &ir::Expr,
        captures: &[(usize, &Type, Value)],
        plan: &Option<tree_iteration::Plan>,
        span: Span,
    ) -> Checked<Summary> {
        let mut engine = Engine::new(self.program);
        engine.work = self.work;
        engine.mode = self.mode;
        engine.iteration_breaks = Some(Predicate::FALSE);
        engine.summaries = self.summaries;
        engine.relevance = self.relevance;
        engine.effect_cache = self.effect_cache.clone();
        let value = tree_iteration::input(&mut engine, item, plan.as_ref(), span)?;
        engine.inputs.push(value.clone());
        engine.pattern(pattern, &value, span, 0)?;
        for id in engine.locals.keys() {
            engine.parameters.insert(*id, 0);
        }
        for (index, (id, ty, actual)) in captures.iter().enumerate() {
            let shape = engine.effect_shape(actual, span, 0)?;
            let value = engine.fresh_shaped(ty, Some(index + 1), Some(&shape), span, 0)?;
            engine.inputs.push(value.clone());
            engine.locals.insert(*id, value);
            engine.parameters.insert(*id, index + 1);
        }
        let value = engine.expression(body, 0)?;
        engine.exit(&value, span, 0)?;
        let output = engine.output(span)?;
        engine.finish(output)
    }
    /// Universal collection binding retains original value origins; keys carry no Result obligations.
    fn iteration_family(&mut self, value: &Value, ty: &Type, span: Span) -> Checked<Value> {
        match ty {
            Type::List(_) => substitute::Substitution::family(self, value, false, span, 0),
            Type::Range => self.node(Region::Empty, span),
            Type::Map(key, _) => {
                let key = self.fresh(key, None, span, 0)?;
                let value = substitute::Substitution::family(self, value, true, span, 0)?;
                self.node(Region::Product(vec![key, value]), span)
            }
            _ => self.unsupported(span),
        }
    }
}
