//! Checked access and allocation boundaries route failures through normal cleanup.
use super::*;

/// The entry wrapper handles only documented successful result payloads.
pub(super) fn main_type(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Unit) || matches!(ty, Type::Result(ok, _) if **ok == Type::Unit)
}

impl Emitter<'_> {
    /// Read one validated list element and retain every bit of its concrete payload.
    pub(super) fn list_access(
        &mut self,
        args: &[Expr],
        head: bool,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let arity = if head { 1 } else { 2 };
        if args.len() != arity {
            return Err(invalid(
                span,
                "list access argument count differs from signature",
            ));
        }
        let Type::List(item) = &args[0].ty else {
            return Err(invalid(span, "list access requires List"));
        };
        if !head {
            expect_type(args[1].ty.clone(), Type::Int, args[1].span)?;
        }
        let list = self.expr(&args[0], locals, depth)?;
        let index = if head {
            "0".into()
        } else {
            self.expr(&args[1], locals, depth)?
        };
        self.list_access_used = true;
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_rs_list_access"),
                args: vec![
                    (Scalar::I64, native_operand("%fault")),
                    (Scalar::I64, native_operand(&(list))),
                    (Scalar::I64, native_operand(&(index))),
                    (Scalar::I32, native_operand(&(u8::from(head)).to_string())),
                ],
                variadic: None,
            },
        );
        self.guard_fault(locals);
        let value = self.unpack(locals, item, raw);
        Ok((*item.clone(), value))
    }

    /// Evaluate arguments once before the guarded native repetition adapter.
    pub(super) fn repeat_string(
        &mut self,
        args: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let [source, count] = args else {
            return Err(invalid(span, "String.repeat requires two arguments"));
        };
        expect_type(source.ty.clone(), Type::String, source.span)?;
        expect_type(count.ty.clone(), Type::Int, count.span)?;
        let source = self.expr(source, locals, depth)?;
        let count = self.expr(count, locals, depth)?;
        self.repeat_used = true;
        let value = self.assign(
            locals,
            Type::String,
            NativeOperation::Call {
                callee: native_operand("$fern_rs_string_repeat"),
                args: vec![
                    (Scalar::I64, native_operand("%fault")),
                    (Scalar::I64, native_operand(&(source))),
                    (Scalar::I64, native_operand(&(count))),
                ],
                variadic: None,
            },
        );
        self.guard_fault(locals);
        Ok((Type::String, value))
    }

    /// Report an application Result only after all deferred cleanup and fault checks.
    pub(super) fn result_main_exit(&mut self) {
        const MESSAGE: &str = "fern: main returned Err";
        let bytes = MESSAGE.len() + 1;
        self.data.data(
            "$fern_rs_main_error",
            vec![
                DataValue::Bytes(MESSAGE.as_bytes().to_vec()),
                DataValue::Bytes(vec![10]),
            ],
        );
        self.output.statement(Statement::Assign {
            destination: "%ok".to_owned(),
            ty: Scalar::I32,
            operation: NativeOperation::Call {
                callee: native_operand("$fern_result_is_ok"),
                args: vec![(Scalar::I64, native_operand("%exit"))],
                variadic: None,
            },
        });
        self.output.statement(Statement::Branch {
            condition: native_operand("%ok"),
            then_label: "@ok".to_owned(),
            else_label: "@err".to_owned(),
        });
        self.output.statement(Statement::Label("@ok".to_owned()));
        self.output
            .statement(Statement::Return(Some(native_operand("0"))));
        self.output.statement(Statement::Label("@err".to_owned()));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$write"),
                args: vec![
                    (Scalar::I32, native_operand("2")),
                    (Scalar::I64, native_operand("$fern_rs_main_error")),
                    (Scalar::I64, native_operand(&(bytes).to_string())),
                ],
                variadic: None,
            }));
        self.output
            .statement(Statement::Return(Some(native_operand("1"))));
        self.output.end();
    }
}

impl Emitter<'_> {
    /// Validate UTF-8 byte endpoints without allocating or re-evaluating source arguments.
    pub(super) fn slice_string(
        &mut self,
        args: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let [source, start, end] = args else {
            return Err(invalid(span, "String.slice requires three arguments"));
        };
        expect_type(source.ty.clone(), Type::String, source.span)?;
        expect_type(start.ty.clone(), Type::Int, start.span)?;
        expect_type(end.ty.clone(), Type::Int, end.span)?;
        let source = self.expr(source, locals, depth)?;
        let start = self.expr(start, locals, depth)?;
        let end = self.expr(end, locals, depth)?;
        self.slice_used = true;
        let value = self.assign(
            locals,
            Type::String,
            NativeOperation::Call {
                callee: native_operand("$fern_rs_string_slice"),
                args: vec![
                    (Scalar::I64, native_operand("%fault")),
                    (Scalar::I64, native_operand(&(source))),
                    (Scalar::I64, native_operand(&(start))),
                    (Scalar::I64, native_operand(&(end))),
                ],
                variadic: None,
            },
        );
        self.guard_fault(locals);
        Ok((Type::String, value))
    }
}

impl Emitter<'_> {
    /// Reject invalid external UTF-8 before splitting can allocate or fail inside C.
    pub(super) fn split_guard(&mut self, arguments: &[(Scalar, Operand)], locals: &mut Locals) {
        let valid = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_str_split_is_valid"),
                args: arguments.to_vec(),
                variadic: None,
            },
        );
        let resume = locals.label();
        let invalid = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(valid)),
            then_label: (resume).to_string(),
            else_label: (invalid).to_string(),
        });
        self.start_block(locals, &invalid);
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("7"),
            address: native_operand("%fault"),
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand("%return_slot"),
        });
        self.output.statement(Statement::Jump("@return".to_owned()));
        self.start_block(locals, &resume);
    }
}

impl Emitter<'_> {
    /// Bound decimal text before native classification so a size fault drains Fern defers.
    pub(super) fn decimal_guard(&mut self, arguments: &[(Scalar, Operand)], locals: &mut Locals) {
        let valid = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_str_decimal_size_is_valid"),
                args: arguments.to_vec(),
                variadic: None,
            },
        );
        let resume = locals.label();
        let invalid = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(valid)),
            then_label: (resume).to_string(),
            else_label: (invalid).to_string(),
        });
        self.start_block(locals, &invalid);
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("5"),
            address: native_operand("%fault"),
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand("%return_slot"),
        });
        self.output.statement(Statement::Jump("@return".to_owned()));
        self.start_block(locals, &resume);
    }
}
