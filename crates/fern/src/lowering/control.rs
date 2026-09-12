//! Explicit control exits and function-owned LIFO deferred cleanup.
use super::*;

impl Emitter<'_> {
    /// A strict Never expression evaluates only its operands, stopping at termination.
    pub(super) fn strict_termination(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<()> {
        if expr.ty != Type::Never {
            return Ok(());
        }
        let children: Vec<&Expr> = match &expr.kind {
            ExprKind::List(items) | ExprKind::Tuple(items) | ExprKind::Interpolate(items) => {
                items.iter().collect()
            }
            ExprKind::Range { start, end, .. } => vec![start, end],
            ExprKind::For { iterable, .. } if iterable.ty == Type::Never => vec![iterable],
            ExprKind::Map(entries) => entries.iter().flat_map(|(k, v)| [k, v]).collect(),
            ExprKind::CustomConstruct { fields, .. } => fields.iter().collect(),
            ExprKind::Construct { value, .. } => value.iter().map(|v| v.as_ref()).collect(),
            ExprKind::Field { value, .. }
            | ExprKind::Try(value)
            | ExprKind::Wrap(value)
            | ExprKind::JsonCodecTemplate { input: value, .. }
            | ExprKind::JsonCodec { input: value, .. }
            | ExprKind::UnionInject { value }
            | ExprKind::UnionWiden { value }
            | ExprKind::Unwrap(value)
            | ExprKind::Unary { value, .. } => vec![value],
            ExprKind::Call { args, .. } => args.iter().collect(),
            ExprKind::Invoke { callee, args } => {
                std::iter::once(callee.as_ref()).chain(args).collect()
            }
            ExprKind::Closure { captures, .. } => captures.iter().collect(),
            ExprKind::Binary { op, left, right } if !matches!(op, BinaryOp::And | BinaryOp::Or) => {
                vec![left, right]
            }
            ExprKind::If { condition, .. } if condition.ty == Type::Never => vec![condition],
            ExprKind::Match { value, .. } if value.ty == Type::Never => vec![value],
            _ => return Ok(()),
        };
        for child in children {
            self.expr(child, locals, depth)?;
        }
        Err(invalid(
            expr.span,
            "Never expression has no terminating operand",
        ))
    }

    /// Save a full-width return payload before any deferred callback can allocate or run.
    pub(super) fn save_return(&mut self, value: &str, locals: &mut Locals) {
        let payload = if locals.return_type == Type::Unit {
            "0".into()
        } else {
            self.payload(locals, &locals.return_type.clone(), value.into())
        };
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(payload)),
            address: native_operand("%return_slot"),
        });
        self.output.statement(Statement::Jump("@return".to_owned()));
    }

    /// Flush only this function's dynamic cleanup stack and restore its typed return ABI.
    pub(super) fn finish_function(&mut self, locals: &mut Locals) {
        self.start_block(locals, "@return");
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$fern_rs_run_defers"),
                args: vec![
                    (Scalar::I64, native_operand("%defer_head")),
                    (Scalar::I64, native_operand("%fault")),
                ],
                variadic: None,
            }));
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand("%return_slot")),
        );
        let value = self.unpack(locals, &locals.return_type.clone(), raw);
        self.output
            .statement(Statement::Return(Some(native_operand(&(value)))));
        self.output.end();
    }

    /// Route explicit returns through the same cleanup path as implicit return and Try.
    pub(super) fn returned(
        &mut self,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        expect_type(value.ty.clone(), locals.return_type.clone(), value.span)?;
        let value = self.tail_expr(value, locals, depth)?;
        self.save_return(&value, locals);
        Err(Exit::Terminated)
    }

    /// Register a captured Unit callback without running its deferred body now.
    pub(super) fn defer(
        &mut self,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        expect_type(
            value.ty.clone(),
            Type::Function(vec![], Box::new(Type::Unit)),
            value.span,
        )?;
        let closure = self.expr(value, locals, depth)?;
        let previous = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand("%defer_head")),
        );
        let node = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(Scalar::I64, native_operand("16"))],
                variadic: None,
            },
        );
        let link = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(node)),
                native_operand("8"),
            ),
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(closure)),
            address: native_operand(&(node)),
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(previous)),
            address: native_operand(&(link)),
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(node)),
            address: native_operand("%defer_head"),
        });
        Ok((Type::Unit, "0".into()))
    }

    /// Record a branch only if it reaches the merge block with an actual value.
    pub(super) fn incoming(
        &mut self,
        outcome: Lowering<String>,
        incoming: &mut Vec<(String, String)>,
        merge: &str,
        locals: &mut Locals,
    ) -> Lowering<()> {
        match outcome {
            Ok(value) => {
                incoming.push((locals.current.clone(), value));
                self.output.statement(Statement::Jump((merge).to_string()));
            }
            Err(Exit::Terminated) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// Merge continuing branches; a single predecessor needs no artificial phi.
    pub(super) fn join(
        &mut self,
        ty: Type,
        incoming: Vec<(String, String)>,
        merge: &str,
        locals: &mut Locals,
    ) -> Lowering<(Type, String)> {
        if incoming.is_empty() {
            return Err(Exit::Terminated);
        }
        self.start_block(locals, merge);
        let value = if ty == Type::Unit {
            "0".into()
        } else if incoming.len() == 1 {
            incoming[0].1.clone()
        } else {
            let operands = incoming
                .into_iter()
                .map(|(label, value)| (label, native_operand(&value)))
                .collect();
            self.assign(locals, ty.clone(), NativeOperation::Phi(operands))
        };
        Ok((ty, value))
    }
}

/// Join branch types through bottom without permitting incompatible live results.
pub(super) fn joined(left: &Type, right: &Type, span: Span) -> Lowering<Type> {
    if *left == Type::Never {
        return Ok(right.clone());
    }
    expect_type(right.clone(), left.clone(), span)?;
    Ok(left.clone())
}
