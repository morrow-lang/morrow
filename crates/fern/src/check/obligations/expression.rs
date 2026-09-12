//! Ordered evaluation preserves argument effects and all produced Result origins.
use super::*;

impl Engine<'_> {
    /// Evaluate only implemented forms; later stages must explicitly supply flow proofs.
    pub(super) fn expression(&mut self, expr: &ir::Expr, depth: usize) -> Checked<Value> {
        self.charge(1, expr.span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                expr.span,
                "Result obligation expression depth limit exceeded",
            ));
        }
        if self.path == Predicate::FALSE {
            return self.node(Region::Empty, expr.span);
        }
        use ir::ExprKind as E;
        match &expr.kind {
            E::Actor(actor) => self.actor(actor, expr, depth + 1),
            E::JsonCodec { input, .. } => self.codec_input(input, expr, depth + 1),
            E::JsonCodecTemplate { input, .. } if self.mode != Mode::Concrete => {
                self.codec_input(input, expr, depth + 1)
            }
            E::Break => self.iteration_exit(true, expr.span, depth + 1),
            E::Continue => self.iteration_exit(false, expr.span, depth + 1),
            E::For {
                pattern,
                iterable,
                body,
            } => self.iteration(pattern, iterable, body, expr.span, depth + 1),
            E::Range { start, end, .. } => {
                self.expression(start, depth + 1)?;
                self.expression(end, depth + 1)?;
                self.node(Region::Empty, expr.span)
            }
            E::If {
                condition,
                then_branch,
                else_branch,
            } => self.conditional(
                condition,
                then_branch,
                else_branch.as_deref(),
                expr.span,
                depth + 1,
            ),
            E::Match { value, arms } => self.matching(value, arms, expr.span, depth + 1),
            E::With {
                steps,
                body,
                handlers,
            } => self.with(steps, body, handlers, expr.span, depth + 1),
            E::Return(value) => self.return_value(value, expr.span, depth + 1),
            E::Try(value) => self.propagate(value, expr.span, depth + 1),
            E::Defer(value) => self.defer(value, expr.span, depth + 1),
            E::Unary {
                op: crate::ast::UnaryOp::Not,
                value,
            } => {
                let value = self.expression(value, depth + 1)?;
                let condition = self.condition(&value, expr.span)?;
                let condition = self.predicates.not(condition, &mut self.work, expr.span)?;
                self.node(Region::Boolean(condition), expr.span)
            }
            E::Binary { op, left, right }
                if matches!(op, crate::ast::BinaryOp::And | crate::ast::BinaryOp::Or) =>
            {
                self.logical(*op, left, right, expr.span, depth + 1)
            }
            _ => self.value_expression(expr, depth),
        }
    }
    /// Evaluate the returned value before proving all remaining duties at this actual function exit.
    fn return_value(&mut self, value: &ir::Expr, span: Span, depth: usize) -> Checked<Value> {
        let value = self.expression(value, depth)?;
        self.function_exit(&value, span, depth)?;
        self.node(Region::Empty, span)
    }
    /// Serialization borrows its one executable input and creates a separate fresh Result duty.
    fn codec_input(&mut self, input: &ir::Expr, expr: &ir::Expr, depth: usize) -> Checked<Value> {
        self.expression(input, depth)?;
        self.fresh(&expr.ty, None, expr.span, depth)
    }
    /// Non-control expressions retain provenance while reusing the caller's checked depth.
    fn value_expression(&mut self, expr: &ir::Expr, depth: usize) -> Checked<Value> {
        use ir::ExprKind as E;
        match &expr.kind {
            E::Local(id) => self.local(id.0, expr.span),
            E::Int(value) => self.node(Region::Scalar(Key::Int(*value)), expr.span),
            E::Bool(value) => self.node(Region::Scalar(Key::Bool(*value)), expr.span),
            E::String(value) => {
                self.charge(value.len() / 8 + 1, expr.span)?;
                self.node(Region::Scalar(Key::String(value.clone())), expr.span)
            }
            E::Unit | E::Float(_) => self.node(Region::Empty, expr.span),
            E::List(items) => self.collection_literal(items, false, expr.span, depth + 1),
            E::Tuple(items) => self.collection_literal(items, true, expr.span, depth + 1),
            E::Construct { constructor, value } => {
                self.construct(*constructor, value.as_deref(), expr, depth + 1)
            }
            E::CustomConstruct { tag, fields } => {
                self.nominal_construct(*tag, fields, expr.span, depth + 1)
            }
            E::Field { value, index } => {
                let value = self.expression(value, depth + 1)?;
                self.field(&value, *index, expr.span)
            }
            E::Wrap(value) | E::Unwrap(value) => self.expression(value, depth + 1),
            E::UnionInject { value } => self.union_inject(value, &expr.ty, expr.span, depth + 1),
            E::UnionWiden { value } => {
                let value = self.expression(value, depth + 1)?;
                self.union_narrow(&value, &expr.ty, expr.span, depth + 1)
                    .map(|(_, v)| v)
            }
            E::Map(entries) => self.map(entries, expr.span, depth + 1),
            E::Block(statements) => self.block(statements, expr.span, depth + 1),
            E::Call { target, args } => self.call(*target, args, expr, depth + 1),
            E::Closure { function, captures } => {
                self.closure(function.0, captures, expr.span, depth + 1)
            }
            E::Invoke { callee, args } => {
                let callee = self.expression(callee, depth + 1)?;
                let args = self.arguments(args, depth + 1, expr.span)?;
                self.invoke(&callee, &args, &expr.ty, expr.span, depth + 1)
            }
            E::Unary { value, .. } => {
                self.expression(value, depth + 1)?;
                self.node(Region::Empty, expr.span)
            }
            E::Binary { op: _, left, right } => {
                self.expression(left, depth + 1)?;
                self.expression(right, depth + 1)?;
                self.fresh(&expr.ty, None, expr.span, depth + 1)
            }
            E::Interpolate(values) => {
                self.arguments(values, depth + 1, expr.span)?;
                self.node(Region::Empty, expr.span)
            }
            E::EditorHole { receiver, .. } if self.mode == Mode::Editor => {
                self.expression(receiver, depth + 1)?;
                self.node(Region::EditorBorrow, expr.span)
            }
            E::Probe { .. } | E::EditorHole { .. } => Err(Diagnostic::new(
                expr.span,
                "private computation cannot enter Result obligation analysis",
            )),
            _ => self.unsupported(expr.span),
        }
    }
    /// Literal product/list construction preserves written evaluation order and exact emptiness.
    fn collection_literal(
        &mut self,
        items: &[ir::Expr],
        tuple: bool,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let items = self.arguments(items, depth, span)?;
        let kind = if tuple {
            Region::Product(items)
        } else {
            let nonempty = if items.is_empty() {
                Predicate::FALSE
            } else {
                Predicate::TRUE
            };
            Region::List {
                items,
                exact: true,
                nonempty,
            }
        };
        self.node(kind, span)
    }
    /// Evaluate written arguments before applying an explicit intrinsic or runtime effect.
    fn call(
        &mut self,
        target: ir::CallTarget,
        args: &[ir::Expr],
        expr: &ir::Expr,
        depth: usize,
    ) -> Checked<Value> {
        let values = self.arguments(args, depth, expr.span)?;
        match target {
            ir::CallTarget::Builtin(ir::Builtin::ListFold) => {
                self.collection_fold(&values, args, &expr.ty, expr.span)
            }
            ir::CallTarget::Builtin(
                builtin @ (ir::Builtin::ListAny | ir::Builtin::ListAll | ir::Builtin::ListFind),
            ) => self.collection_search(builtin, &values, args, &expr.ty, expr.span),
            ir::CallTarget::Builtin(builtin @ (ir::Builtin::ListMap | ir::Builtin::ListFilter)) => {
                self.collection_transform(builtin, &values, args, &expr.ty, expr.span)
            }
            ir::CallTarget::Builtin(builtin) => self.builtin(builtin, &values, &expr.ty, expr.span),
            ir::CallTarget::Runtime(_) => self.fresh(&expr.ty, None, expr.span, 0),
            ir::CallTarget::Function(id) => {
                if self.relevance.is_some_and(|set| !set.contains(&id.0)) {
                    if expr.ty == Type::Never {
                        self.path = Predicate::FALSE;
                        self.node(Region::Empty, expr.span)
                    } else {
                        self.fresh(&expr.ty, None, expr.span, 0)
                    }
                } else {
                    self.source_call(id.0, &values, expr.span)
                }
            }
        }
    }
    /// A source reference is borrowing until the surrounding operation proves handling/transfer.
    fn local(&mut self, id: usize, span: Span) -> Checked<Value> {
        if let Some(index) = self.parameters.get(&id) {
            self.used_inputs.insert(*index);
        }
        self.locals
            .get(&id)
            .cloned()
            .ok_or_else(|| Diagnostic::new(span, "unknown Result obligation local"))
    }
    /// Charge argument count before allocation; evaluate left to right exactly once.
    pub(super) fn arguments(
        &mut self,
        args: &[ir::Expr],
        depth: usize,
        span: Span,
    ) -> Checked<Vec<Value>> {
        self.charge(args.len(), span)?;
        args.iter().map(|arg| self.expression(arg, depth)).collect()
    }
    /// Preserve every intermediate initializer even when its final value is metadata-only.
    fn block(&mut self, statements: &[ir::Stmt], span: Span, depth: usize) -> Checked<Value> {
        self.charge(statements.len(), span)?;
        let mut result = self.node(Region::Empty, span)?;
        for stmt in statements {
            match stmt {
                ir::Stmt::Let { id, value } => {
                    let value = self.expression(value, depth)?;
                    self.locals.insert(id.0, value);
                    result = self.node(Region::Empty, span)?;
                }
                ir::Stmt::Expr(value) => result = self.expression(value, depth)?,
                ir::Stmt::LetElse {
                    pattern,
                    value,
                    else_branch,
                } => {
                    self.let_else(pattern, value, else_branch, span, depth)?;
                    result = self.node(Region::Empty, span)?;
                }
            }
        }
        Ok(result)
    }
    /// Only an actual Result constructor adds an outer error-accountability layer.
    fn construct(
        &mut self,
        constructor: Constructor,
        value: Option<&ir::Expr>,
        expr: &ir::Expr,
        depth: usize,
    ) -> Checked<Value> {
        let fields = value
            .map(|value| self.expression(value, depth))
            .transpose()?
            .into_iter()
            .collect();
        let tag = usize::from(matches!(constructor, Constructor::None | Constructor::Err));
        let mut variants = vec![vec![], vec![]];
        variants[tag] = fields;
        let origin = if matches!(expr.ty, Type::Result(..)) {
            Some(self.origin(None, expr.span)?)
        } else {
            None
        };
        let mut guards = vec![Predicate::FALSE; variants.len()];
        guards[tag] = Predicate::TRUE;
        self.node(
            Region::Sum {
                origin,
                tag: Some(tag),
                guards,
                variants,
            },
            expr.span,
        )
    }
    /// Nominal constructors preserve field identities and their validated active variant.
    fn nominal_construct(
        &mut self,
        tag: usize,
        fields: &[ir::Expr],
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(tag.saturating_add(1), span)?;
        let fields = self.arguments(fields, depth, span)?;
        let mut variants = vec![vec![]; tag + 1];
        variants[tag] = fields;
        let mut guards = vec![Predicate::FALSE; variants.len()];
        guards[tag] = Predicate::TRUE;
        self.node(
            Region::Sum {
                origin: None,
                tag: Some(tag),
                guards,
                variants,
            },
            span,
        )
    }
    /// Projection through a partial region never upgrades that region to complete coverage.
    pub(super) fn field(&mut self, value: &Value, index: usize, span: Span) -> Checked<Value> {
        if matches!(value.node.kind, Region::Nominal { .. }) {
            let value = self.expand_nominal(value, span, 0)?;
            return self.field(&value, index, span);
        }
        if let Region::Choice(choices) = &value.node.kind {
            self.charge(choices.len(), span)?;
            let mut fields = Vec::new();
            for (guard, child) in choices {
                if *guard != Predicate::FALSE {
                    fields.push((*guard, self.field(child, index, span)?));
                }
            }
            let output = self.node(Region::Choice(fields), span)?;
            return Ok(if value.complete {
                output
            } else {
                output.partial()
            });
        }
        let fields = match &value.node.kind {
            Region::Product(fields) => fields,
            Region::Sum {
                tag: None,
                variants,
                ..
            } if variants.len() == 1 => &variants[0],
            Region::Sum {
                tag: Some(tag),
                variants,
                ..
            } => variants
                .get(*tag)
                .ok_or_else(|| Diagnostic::new(span, "invalid obligation variant"))?,
            _ => return self.unsupported(span),
        };
        let field = fields
            .get(index)
            .ok_or_else(|| Diagnostic::new(span, "invalid obligation field"))?;
        Ok(if value.complete {
            field.clone()
        } else {
            field.partial()
        })
    }
    /// Duplicate literal keys replace output entries without erasing previously produced duties.
    fn map(
        &mut self,
        entries: &[(ir::Expr, ir::Expr)],
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(entries.len(), span)?;
        let mut values = Vec::new();
        for (key, value) in entries {
            let key = self.expression(key, depth)?;
            let value = self.expression(value, depth)?;
            let key = self.key(&key, span)?;
            self.insert(&mut values, key, value, span)?;
        }
        self.node(
            Region::Map {
                nonempty: if values.is_empty() {
                    Predicate::FALSE
                } else {
                    Predicate::TRUE
                },
                entries: values,
                exact: true,
            },
            span,
        )
    }
}

impl Engine<'_> {
    /// Sending borrows its message; only the newly returned enqueue Result gains a fresh duty.
    fn actor(&mut self, actor: &ir::ActorExpr, expr: &ir::Expr, depth: usize) -> Checked<Value> {
        match actor {
            ir::ActorExpr::Spawn { entry, .. } => {
                self.expression(entry, depth)?;
                self.node(Region::Empty, expr.span)
            }
            ir::ActorExpr::Send { pid, message } => {
                self.expression(pid, depth)?;
                self.expression(message, depth)?;
                self.fresh(&expr.ty, None, expr.span, depth)
            }
            ir::ActorExpr::Receive {
                mailbox,
                arms,
                timeout,
            } => {
                if let Some((duration, _)) = timeout {
                    self.expression(duration, depth)?;
                }
                let value = self.fresh(mailbox, None, expr.span, depth)?;
                self.matching_region(
                    value,
                    arms,
                    timeout.as_ref().map(|(_, b)| b.as_ref()),
                    true,
                    expr.span,
                    depth,
                )
            }
            ir::ActorExpr::Call { args, .. } => {
                for arg in args {
                    self.expression(arg, depth + 1)?;
                }
                self.node(Region::Empty, expr.span)
            }
            ir::ActorExpr::Lowered(_) => self.unsupported(expr.span),
        }
    }
}
