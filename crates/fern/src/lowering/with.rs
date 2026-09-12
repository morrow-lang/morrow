//! Flat Result sequencing keeps typed error handlers separate from success bindings.
use super::*;

struct Handler {
    label: String,
    slot: String,
    reachable: bool,
}

impl Emitter<'_> {
    /// Route each failure to its statically typed handler and merge only continuing paths.
    pub(super) fn with(
        &mut self,
        steps: &[ir::WithStep],
        body: &Expr,
        handlers: &[ir::WithHandler],
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<(Type, String)> {
        let ty = self.with_signature(steps, body, handlers, locals)?;
        let outer = locals.values.clone();
        let merge = locals.label();
        let mut targets: Vec<_> = handlers
            .iter()
            .map(|_| Handler {
                label: locals.label(),
                slot: locals.stack_slot(),
                reachable: false,
            })
            .collect();
        let mut incoming = Vec::new();
        let success = self.with_success(steps, body, &mut targets, locals, depth, tail);
        locals.values = outer.clone();
        self.incoming(success, &mut incoming, &merge, locals)?;
        for (handler, target) in handlers.iter().zip(targets) {
            if !target.reachable {
                continue;
            }
            self.start_block(locals, &target.label);
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Load(LoadKind::I64, native_operand(&(target.slot).to_string())),
            );
            let value = self.unpack(locals, &handler.error.ty, raw);
            locals.define(
                handler.error.id.0,
                handler.error.ty.clone(),
                value,
                handler.body.span,
            )?;
            let outcome = self.position_expr(&handler.body, locals, depth, tail);
            locals.values = outer.clone();
            self.incoming(outcome, &mut incoming, &merge, locals)?;
        }
        self.join(ty, incoming, &merge, locals)
    }

    /// Validate handler references, concrete error identities, and live branch result types.
    fn with_signature(
        &self,
        steps: &[ir::WithStep],
        body: &Expr,
        handlers: &[ir::WithHandler],
        locals: &Locals,
    ) -> Lowering<Type> {
        if steps.is_empty() || steps.len() > MAX_NODES || handlers.len() > MAX_NODES {
            return Err(invalid(
                body.span,
                "with requires a bounded nonempty step list",
            ));
        }
        let mut ty = body.ty.clone();
        let mut used = BTreeSet::new();
        for step in steps {
            if step.value.ty == Type::Never {
                continue;
            }
            let Type::Result(_, error) = &step.value.ty else {
                return Err(invalid(step.value.span, "with step requires Result"));
            };
            let expected = if let Some(index) = step.error_handler {
                used.insert(index);
                &handlers
                    .get(index)
                    .ok_or_else(|| invalid(step.value.span, "unknown with handler"))?
                    .error
                    .ty
            } else {
                let Type::Result(_, error) = &locals.return_type else {
                    return Err(invalid(
                        step.value.span,
                        "with propagation requires enclosing Result",
                    ));
                };
                error.as_ref()
            };
            expect_type(*error.clone(), expected.clone(), step.value.span)?;
        }
        for (index, handler) in handlers.iter().enumerate() {
            nominal::resolved(&handler.error.ty, &self.layouts, handler.body.span, 0)?;
            if !used.contains(&index) {
                return Err(invalid(handler.body.span, "unused with handler"));
            }
            ty = control::joined(&ty, &handler.body.ty, handler.body.span)?;
        }
        Ok(ty)
    }

    /// Evaluate steps in order; only successful payloads enter subsequent binding scopes.
    fn with_success(
        &mut self,
        steps: &[ir::WithStep],
        body: &Expr,
        targets: &mut [Handler],
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<String> {
        for step in steps {
            let value = self.expr(&step.value, locals, depth)?;
            let Type::Result(item, _) = &step.value.ty else {
                return Err(invalid(step.value.span, "with step requires Result"));
            };
            let good = locals.label();
            let bad = locals.label();
            let tag = self.assign(
                locals,
                Type::Bool,
                NativeOperation::Call {
                    callee: native_operand("$fern_result_is_ok"),
                    args: vec![(Scalar::I64, native_operand(&(value).to_string()))],
                    variadic: None,
                },
            );
            self.output.statement(Statement::Branch {
                condition: native_operand(&(tag)),
                then_label: (good).to_string(),
                else_label: (bad).to_string(),
            });
            self.start_block(locals, &bad);
            if let Some(index) = step.error_handler {
                let target = &mut targets[index];
                let error = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$fern_result_unwrap"),
                        args: vec![(Scalar::I64, native_operand(&(value).to_string()))],
                        variadic: None,
                    },
                );
                self.output.statement(Statement::Store {
                    kind: LoadKind::I64,
                    value: native_operand(&(error)),
                    address: native_operand(&(target.slot).to_string()),
                });
                self.output
                    .statement(Statement::Jump((target.label).to_string()));
                target.reachable = true;
            } else {
                self.save_return(&value, locals);
            }
            self.start_block(locals, &good);
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand("$fern_result_unwrap"),
                    args: vec![(Scalar::I64, native_operand(&(value).to_string()))],
                    variadic: None,
                },
            );
            let item_value = self.unpack(locals, item, raw);
            self.bind_irrefutable(
                &step.pattern,
                item,
                &item_value,
                step.value.span,
                locals,
                depth,
            )?;
        }
        self.position_expr(body, locals, depth, tail)
    }
}
