//! Reuse only nonescaping compiler-owned Unit helper frames at self-tail boundaries.
use super::*;

impl Emitter<'_> {
    pub(super) fn reuse_actor_frame(
        &mut self,
        entry: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<String> {
        let ExprKind::Closure {
            function: id,
            captures,
        } = &entry.kind
        else {
            return Err(invalid(
                entry.span,
                "reusable continuation requires a private closure recipe",
            ));
        };
        if !self.actors.helpers.values().any(|helper| *helper == id.0) {
            return Err(invalid(
                entry.span,
                "reusable continuation is not a private Unit helper",
            ));
        }
        let target = self
            .functions
            .get(&id.0)
            .ok_or_else(|| invalid(entry.span, "unknown reusable helper identity"))?;
        if captures.len() != target.captures.len() || captures.len() > 4096 {
            return Err(invalid(
                entry.span,
                "reusable helper capture count differs from signature",
            ));
        }
        let expected: Vec<_> = target
            .captures
            .iter()
            .map(|param| param.ty.clone())
            .collect();
        let mut values = Vec::new();
        for (capture, ty) in captures.iter().zip(expected) {
            expect_type(capture.ty.clone(), ty, capture.span)?;
            let value = self.expr(capture, locals, depth)?;
            // Keep owned arguments in the ordinary precise native root frame
            // until either publication path completes, including fallback GC.
            self.root_value(locals, &capture.ty, &value);
            values.push(self.payload(locals, &capture.ty, value));
        }
        // Entry-owned scratch does not escape the callback. All argument effects
        // and checked faults precede both staging and the first possible mutation.
        let staged = locals.temporary();
        locals.stack_allocations.statement(Statement::Assign {
            destination: staged.clone(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc {
                bytes: (8 * (values.len() + 1)) as u32,
                align: 8,
            },
        });
        let identity = format!("$f{}", id.0);
        self.store_reuse_words(&staged, &identity, &values, locals);
        let status = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$morrow_managed_continue_reuse"),
                args: vec![
                    (Scalar::I64, native_operand("%exec")),
                    (Scalar::I64, native_operand("%env")),
                    (Scalar::I64, native_operand(&staged)),
                    (Scalar::I64, native_operand(&values.len().to_string())),
                ],
                variadic: None,
            },
        );
        let declined = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                native_operand(&status),
                native_operand("4"),
            ),
        );
        let previous = locals.current.clone();
        let fallback = locals.label();
        let done = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&declined),
            then_label: fallback.clone(),
            else_label: done.clone(),
        });
        self.start_block(locals, &fallback);
        // Identity changes or changed owned graph roots use ordinary immutable
        // publication. Reuse the already evaluated values; never repeat effects.
        let frame = self.assign(
            locals,
            entry.ty.clone(),
            NativeOperation::Call {
                callee: native_operand("$morrow_alloc"),
                args: vec![(
                    Scalar::I64,
                    native_operand(&(8 * (values.len() + 1)).to_string()),
                )],
                variadic: None,
            },
        );
        self.store_reuse_words(&frame, &identity, &values, locals);
        let fallback_status = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$morrow_managed_continue"),
                args: vec![
                    (Scalar::I64, native_operand("%exec")),
                    (Scalar::I64, native_operand(&frame)),
                ],
                variadic: None,
            },
        );
        let fallback_predecessor = locals.current.clone();
        self.output.statement(Statement::Jump(done.clone()));
        self.start_block(locals, &done);
        Ok(self.assign(
            locals,
            Type::Int,
            NativeOperation::Phi(vec![
                (previous, native_operand(&status)),
                (fallback_predecessor, native_operand(&fallback_status)),
            ]),
        ))
    }

    fn store_reuse_words(
        &mut self,
        frame: &str,
        identity: &str,
        values: &[String],
        locals: &mut Locals,
    ) {
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(identity),
            address: native_operand(frame),
        });
        for (index, value) in values.iter().enumerate() {
            let address = self.assign(
                locals,
                Type::Int,
                NativeOperation::Binary(
                    MachineBinary::Add,
                    native_operand(frame),
                    native_operand(&(8 * (index + 1)).to_string()),
                ),
            );
            self.output.statement(Statement::Store {
                kind: LoadKind::I64,
                value: native_operand(value),
                address: native_operand(&address),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ir_cannot_forge_a_reusable_private_continuation() {
        let parsed = crate::parse::parse("fn main() -> Int:\n    0\n").unwrap();
        let mut program = crate::check::check(&parsed).unwrap();
        let main = program
            .functions
            .iter_mut()
            .find(|function| function.name == "main")
            .unwrap();
        let original = main.body.clone();
        main.body.kind = ExprKind::Actor(ir::ActorExpr::Lowered(Lowered {
            operation: Operation::ContinueReusable(Box::new(original)),
        }));
        let error = crate::lowering::lower(&program).unwrap_err();
        assert!(
            error
                .message
                .contains("private actor continuation in public IR"),
            "{error:?}"
        );
    }
}
