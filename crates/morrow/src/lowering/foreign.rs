//! Exact C transport preserves sealed pointer owners across borrowed/interior pointer returns.
use super::*;
impl Emitter<'_> {
    pub(super) fn foreign_call(
        &mut self,
        declaration: &crate::ffi::Declaration,
        args: &[Expr],
        result: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        declaration.validate(span)?;
        expect_type(result.clone(), declaration.result.source_type(), span)?;
        if args.len() != declaration.params.len() {
            return Err(invalid(
                span,
                "foreign argument count differs from declared ABI",
            ));
        }
        let mut values = Vec::new();
        let owners_type = Type::List(Box::new(Type::String));
        let mut owners = Vec::new();
        for (arg, abi) in args.iter().zip(&declaration.params) {
            expect_type(arg.ty.clone(), abi.source_type(), arg.span)?;
            let value = self.expr(arg, locals, depth)?;
            let raw = if matches!(abi, crate::ffi::AbiType::Pointer(_)) {
                self.pointer_layout(&arg.ty, span)?;
                owners.push(self.custom_field(&value, 1, &owners_type, locals));
                self.custom_field(&value, 0, &Type::Int, locals)
            } else {
                value
            };
            let scalar = abi
                .machine_scalar()
                .ok_or_else(|| invalid(span, "void foreign argument"))?;
            values.push((scalar, native_operand(&raw)));
        }
        let operation = NativeOperation::ForeignCall {
            declaration: declaration.clone(),
            args: values,
        };
        // Foreign libraries may hide thread-affine resources behind scalar
        // handles. This also covers ordinary helpers called from actor code.
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_managed_pin_current"),
                args: vec![],
                variadic: None,
            }));
        if *result == Type::Unit {
            self.output.statement(Statement::Effect(operation));
            return Ok((Type::Unit, "0".into()));
        }
        if matches!(&declaration.result, crate::ffi::AbiType::Pointer(_)) {
            self.pointer_layout(result, span)?;
            let address = self.assign(locals, Type::Int, operation);
            let mut retained = self.assign(
                locals,
                owners_type.clone(),
                NativeOperation::Call {
                    callee: native_operand("$morrow_list_new"),
                    args: vec![],
                    variadic: None,
                },
            );
            for owner in owners {
                retained = self.assign(
                    locals,
                    owners_type.clone(),
                    NativeOperation::Call {
                        callee: native_operand("$morrow_list_concat"),
                        args: vec![
                            (Scalar::I64, native_operand(&retained)),
                            (Scalar::I64, native_operand(&owner)),
                        ],
                        variadic: None,
                    },
                );
            }
            let pointer = self.assign(
                locals,
                result.clone(),
                NativeOperation::Call {
                    callee: native_operand("$morrow_alloc"),
                    args: vec![(Scalar::I64, Operand::Int(24))],
                    variadic: None,
                },
            );
            for (offset, value) in [(0, "0".to_owned()), (8, address), (16, retained)] {
                let slot = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Binary(
                        MachineBinary::Add,
                        native_operand(&pointer),
                        Operand::Int(offset),
                    ),
                );
                self.output.statement(Statement::Store {
                    kind: LoadKind::I64,
                    value: native_operand(&value),
                    address: native_operand(&slot),
                });
            }
            Ok((result.clone(), pointer))
        } else {
            Ok((
                result.clone(),
                self.assign(locals, result.clone(), operation),
            ))
        }
    }
    fn pointer_layout(&self, ty: &Type, span: Span) -> Lowering<()> {
        let layout = self
            .layouts
            .get(ty)
            .ok_or_else(|| invalid(span, "foreign pointer requires sealed layout"))?;
        if layout.storage != ir::LayoutStorage::Tagged
            || layout.variants != vec![vec![Type::Int, Type::List(Box::new(Type::String))]]
        {
            return Err(invalid(
                span,
                "foreign pointer layout differs from its sealed contract",
            ));
        }
        Ok(())
    }
}
