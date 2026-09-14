//! Direct self-tail transfers reuse typed parameter state and their invocation context.
use super::*;

pub(super) struct TailLoop {
    function: ir::FunctionId,
    params: Vec<ir::Param>,
    state: Parameters,
}

enum Parameters {
    /// Managed and mixed representations retain private invocation-local storage.
    Slots(Vec<String>),
    /// Plain Int parameters need no collector roots and can remain SSA loop values.
    Phis(Vec<TailPhi>),
}

struct TailPhi {
    instruction: usize,
    incoming: Vec<(String, Operand)>,
}

impl Locals {
    /// Reserve fixed scratch storage in the entry block, not on a repeated dynamic edge.
    pub(super) fn stack_slot(&mut self) -> String {
        let name = self.temporary();
        self.stack_allocations.statement(Statement::Assign {
            destination: (name).to_string(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc { bytes: 8, align: 8 },
        });
        name
    }
}

impl Emitter<'_> {
    /// Initialize parameter storage once, then enter the reusable body through a fresh block.
    pub(super) fn start_tail(&mut self, function: &Function, locals: &mut Locals) -> Lowering<()> {
        if !eligible(function)? {
            return Ok(());
        }
        if function.params.iter().all(|param| param.ty == Type::Int) {
            let predecessor = locals.current.clone();
            // Snapshot original parameter identities before rebinding them to loop values.
            let initial = function
                .params
                .iter()
                .map(|param| {
                    Self::local(param.id, function.body.span, locals).map(|(_, value)| value)
                })
                .collect::<Lowering<Vec<_>>>()?;
            self.output.statement(Statement::Jump("@recur".into()));
            self.start_block(locals, "@recur");
            let mut phis = Vec::new();
            for (param, initial) in function.params.iter().zip(initial) {
                let incoming = vec![(predecessor.clone(), native_operand(&initial))];
                let instruction = self.output.len();
                let value = self.assign(locals, Type::Int, NativeOperation::Phi(incoming.clone()));
                locals.values.insert(param.id.0, (Type::Int, value));
                phis.push(TailPhi {
                    instruction,
                    incoming,
                });
            }
            locals.tail = Some(TailLoop {
                function: function.id,
                params: function.params.clone(),
                state: Parameters::Phis(phis),
            });
            return Ok(());
        }
        let mut slots = Vec::new();
        for param in &function.params {
            let slot = locals.stack_slot();
            let (_, value) = Self::local(param.id, function.body.span, locals)?;
            let value = self.payload(locals, &param.ty, value);
            self.output.statement(Statement::Store {
                kind: LoadKind::I64,
                value: native_operand(&(value)),
                address: native_operand(&(slot)),
            });
            slots.push(slot);
        }
        self.output.statement(Statement::Jump("@recur".to_owned()));
        self.start_block(locals, "@recur");
        for (param, slot) in function.params.iter().zip(&slots) {
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Load(LoadKind::I64, native_operand(&(slot).to_string())),
            );
            let value = self.unpack(locals, &param.ty, raw);
            locals.values.insert(param.id.0, (param.ty.clone(), value));
        }
        locals.tail = Some(TailLoop {
            function: function.id,
            params: function.params.clone(),
            state: Parameters::Slots(slots),
        });
        Ok(())
    }

    /// Complete forward phi inputs before inserting any hoisted allocations.
    pub(super) fn finish_tail(&mut self, locals: &mut Locals, span: Span) -> Lowering<()> {
        let Some(TailLoop {
            state: Parameters::Phis(phis),
            ..
        }) = locals.tail.take()
        else {
            return Ok(());
        };
        for phi in phis {
            let Some(machine::Item::Statement(Statement::Assign {
                operation: NativeOperation::Phi(incoming),
                ..
            })) = self.output.items.get_mut(phi.instruction)
            else {
                return Err(invalid(span, "missing tail parameter phi"));
            };
            *incoming = phi.incoming;
        }
        Ok(())
    }

    /// Keep argument/condition evaluation in value mode while propagating explicit tail positions.
    pub(super) fn position_expr(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<String> {
        if tail {
            self.tail_expr(expr, locals, depth)
        } else {
            self.expr(expr, locals, depth)
        }
    }

    /// Rewrite only expressions whose successful result is the enclosing function's result.
    pub(super) fn tail_expr(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<String> {
        if locals.tail.is_none() || (expr.ty != locals.return_type && expr.ty != Type::Never) {
            return self.expr(expr, locals, depth);
        }
        let supported = matches!(
            expr.kind,
            ExprKind::Call {
                target: CallTarget::Function(_),
                ..
            } | ExprKind::Block(_)
                | ExprKind::If { .. }
                | ExprKind::Match { .. }
                | ExprKind::With { .. }
        );
        if !supported {
            return self.expr(expr, locals, depth);
        }
        self.validate_expr(expr, depth)?;
        self.strict_termination(expr, locals, depth + 1)?;
        let (actual, value) = match &expr.kind {
            ExprKind::Call { target, args } => {
                if matches!(target, CallTarget::Function(id) if Some(*id) == locals.tail.as_ref().map(|t|t.function))
                {
                    return self.tail_call(*target, args, &expr.ty, expr.span, locals, depth + 1);
                }
                self.call(*target, args, &expr.ty, expr.span, locals, depth + 1)?
            }
            ExprKind::Block(stmts) => self.block(stmts, locals, depth + 1, true)?,
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.conditional(
                condition,
                then_branch,
                else_branch.as_deref(),
                locals,
                depth + 1,
                true,
            )?,
            ExprKind::Match { value, arms } => {
                self.matching(value, arms, locals, depth + 1, true)?
            }
            ExprKind::With {
                steps,
                body,
                handlers,
            } => self.with(steps, body, handlers, locals, depth + 1, true)?,
            _ => unreachable!("tail dispatch was checked before lowering"),
        };
        expect_type(actual, expr.ty.clone(), expr.span)?;
        Ok(value)
    }

    /// Evaluate every new argument before changing any old parameter, retaining fault checks.
    fn tail_call(
        &mut self,
        target: CallTarget,
        args: &[Expr],
        result: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<String> {
        let (_, params, actual) = self.signature(target, args, span)?;
        expect_type(actual.clone(), result.clone(), span)?;
        expect_type(actual, locals.return_type.clone(), span)?;
        if args.len() != params.len() {
            return Err(invalid(
                span,
                "call argument count differs from resolved signature",
            ));
        }
        let mut values = Vec::new();
        for (arg, expected) in args.iter().zip(params) {
            expect_type(arg.ty.clone(), expected.clone(), arg.span)?;
            let value = self.expr(arg, locals, depth)?;
            values.push(self.payload(locals, &expected, value));
        }
        let tail = locals
            .tail
            .as_mut()
            .ok_or_else(|| invalid(span, "missing self-tail target"))?;
        if tail.params.len() != values.len() {
            return Err(invalid(span, "self-tail parameter count mismatch"));
        }
        match &mut tail.state {
            Parameters::Slots(slots) => {
                for (slot, value) in slots.iter().zip(values) {
                    self.output.statement(Statement::Store {
                        kind: LoadKind::I64,
                        value: native_operand(&value),
                        address: native_operand(slot),
                    });
                }
            }
            Parameters::Phis(phis) => {
                // Only successful argument evaluation reaches this predecessor.
                // Phi assignment is simultaneous, including swaps of old parameters.
                for (phi, value) in phis.iter_mut().zip(values) {
                    phi.incoming
                        .push((locals.current.clone(), native_operand(&value)));
                }
            }
        }
        self.output.statement(Statement::Jump("@recur".to_owned()));
        Err(Exit::Terminated)
    }
}

/// An owned defer anywhere disables reuse; lifted closure bodies own independent frames.
fn eligible(function: &Function) -> Lowering<bool> {
    if !function.captures.is_empty() {
        return Ok(false);
    }
    let mut pending = vec![(&function.body, 0)];
    let mut count = 0;
    let mut recursive = false;
    while let Some((expr, depth)) = pending.pop() {
        count += 1;
        if count > MAX_NODES || depth > MAX_DEPTH {
            return Err(invalid(
                expr.span,
                "tail analysis complexity limit exceeded",
            ));
        }
        match &expr.kind {
            ExprKind::Defer(_) => return Ok(false),
            ExprKind::Call {
                target: CallTarget::Function(id),
                ..
            } if *id == function.id => recursive = true,
            _ => {}
        }
        pending.extend(children(expr).into_iter().map(|child| (child, depth + 1)));
    }
    Ok(recursive)
}

/// Walk only expressions executed in this function, including closure capture expressions.
fn children(expr: &Expr) -> Vec<&Expr> {
    match &expr.kind {
        ExprKind::Closure { captures, .. } => captures.iter().collect(),
        ExprKind::Invoke { callee, args } => std::iter::once(callee.as_ref()).chain(args).collect(),
        ExprKind::Return(value)
        | ExprKind::Defer(value)
        | ExprKind::Unary { value, .. }
        | ExprKind::Try(value)
        | ExprKind::Field { value, .. }
        | ExprKind::Wrap(value)
        | ExprKind::JsonCodecTemplate { input: value, .. }
        | ExprKind::JsonCodec { input: value, .. }
        | ExprKind::UnionInject { value }
        | ExprKind::UnionWiden { value }
        | ExprKind::Unwrap(value) => vec![value],
        ExprKind::Binary { left, right, .. }
        | ExprKind::Range {
            start: left,
            end: right,
            ..
        } => vec![left, right],
        ExprKind::Call { args, .. }
        | ExprKind::List(args)
        | ExprKind::Tuple(args)
        | ExprKind::Interpolate(args)
        | ExprKind::CustomConstruct { fields: args, .. } => args.iter().collect(),
        ExprKind::Map(entries) => entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        ExprKind::Construct { value, .. } => value.iter().map(|v| v.as_ref()).collect(),
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => std::iter::once(condition.as_ref())
            .chain(std::iter::once(then_branch.as_ref()))
            .chain(else_branch.as_deref())
            .collect(),
        ExprKind::Match { value, arms } => std::iter::once(value.as_ref())
            .chain(
                arms.iter()
                    .flat_map(|a| a.guard.iter().chain(std::iter::once(&a.body))),
            )
            .collect(),
        ExprKind::Block(stmts) => stmts
            .iter()
            .flat_map(|s| match s {
                Stmt::Expr(v) | Stmt::Let { value: v, .. } => vec![v],
                Stmt::LetElse {
                    value, else_branch, ..
                } => vec![value, else_branch],
            })
            .collect(),
        ExprKind::For { iterable, body, .. } => vec![iterable, body],
        ExprKind::With {
            steps,
            body,
            handlers,
        } => steps
            .iter()
            .map(|s| &s.value)
            .chain(std::iter::once(body.as_ref()))
            .chain(handlers.iter().map(|h| &h.body))
            .collect(),
        _ => vec![],
    }
}
