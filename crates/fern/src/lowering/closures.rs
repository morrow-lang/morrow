//! Heap closures use a code pointer followed by full-width lexical captures.
use super::*;

impl Emitter<'_> {
    /// Bind checked capture identities from the uniform environment parameter.
    pub(super) fn load_captures(
        &mut self,
        function: &Function,
        locals: &mut Locals,
    ) -> Lowering<()> {
        for (index, capture) in function.captures.iter().enumerate() {
            let address = self.assign(
                locals,
                Type::Int,
                NativeOperation::Binary(
                    MachineBinary::Add,
                    native_operand("%env"),
                    native_operand(&(8 * (index + 1)).to_string()),
                ),
            );
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Load(LoadKind::I64, native_operand(&(address))),
            );
            let value = self.unpack(locals, &capture.ty, raw);
            locals.define(capture.id.0, capture.ty.clone(), value, function.body.span)?;
        }
        Ok(())
    }

    /// Validate capture arity/types before allocating an escaping lexical environment.
    pub(super) fn closure(
        &mut self,
        id: ir::FunctionId,
        captures: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let function = self
            .functions
            .get(&id.0)
            .ok_or_else(|| invalid(span, "unknown closure function identity"))?;
        if captures.len() != function.captures.len() || captures.len() > MAX_NODES {
            return Err(invalid(
                span,
                "closure capture count differs from lifted function",
            ));
        }
        let mut ty = Type::Function(
            function.params.iter().map(|p| p.ty.clone()).collect(),
            Box::new(function.return_type.clone()),
        );
        if let Some(mailbox) = &function.mailbox {
            ty = Type::ActorFunction(Box::new(mailbox.clone()), Box::new(ty));
        }
        let expected: Vec<_> = function.captures.iter().map(|p| p.ty.clone()).collect();
        let mut values = Vec::new();
        for (capture, expected) in captures.iter().zip(expected) {
            expect_type(capture.ty.clone(), expected, capture.span)?;
            let value = self.expr(capture, locals, depth)?;
            values.push(self.payload(locals, &capture.ty, value));
        }
        let object = self.assign(
            locals,
            ty.clone(),
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(
                    Scalar::I64,
                    native_operand(&(8 * (values.len() + 1)).to_string()),
                )],
                variadic: None,
            },
        );
        let identity = if self.actors.entries.contains_key(&id.0) {
            format!("actor_identity{}", id.0)
        } else {
            format!("f{}", id.0)
        };
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&format!("${}", identity)),
            address: native_operand(&(object)),
        });
        for (index, value) in values.iter().enumerate() {
            let address = self.assign(
                locals,
                Type::Int,
                NativeOperation::Binary(
                    MachineBinary::Add,
                    native_operand(&(object).to_string()),
                    native_operand(&(8 * (index + 1)).to_string()),
                ),
            );
            self.output.statement(Statement::Store {
                kind: LoadKind::I64,
                value: native_operand(&(value).to_string()),
                address: native_operand(&(address)),
            });
        }
        Ok((ty, object))
    }

    /// Evaluate the callee first and each argument once before indirect invocation.
    pub(super) fn invoke(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let Type::Function(params, result) = &callee.ty else {
            return Err(invalid(span, "invocation requires function value"));
        };
        if params.len() != args.len() {
            return Err(invalid(
                span,
                "invocation argument count differs from signature",
            ));
        }
        let closure = self.expr(callee, locals, depth)?;
        let mut values = Vec::new();
        for (arg, param) in args.iter().zip(params) {
            expect_type(arg.ty.clone(), param.clone(), arg.span)?;
            values.push((param.clone(), self.expr(arg, locals, depth)?));
        }
        let value = self.invoke_values(&closure, &values, result, locals);
        Ok((*result.clone(), value))
    }

    /// Invoke an already evaluated, signature-checked callback through its stored code.
    pub(super) fn invoke_values(
        &mut self,
        closure: &str,
        args: &[(Type, String)],
        result: &Type,
        locals: &mut Locals,
    ) -> String {
        let code = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(closure)),
        );
        let mut arguments = vec![
            (Scalar::I64, native_operand(closure)),
            (Scalar::I64, native_operand("%fault")),
        ];
        arguments.extend(
            args.iter()
                .map(|(ty, value)| (machine_width(self.width(ty.clone())), native_operand(value))),
        );
        let instruction = NativeOperation::Call {
            callee: native_operand(&(code)),
            args: arguments.clone(),
            variadic: None,
        };
        let value = if *result == Type::Unit {
            self.output.statement(Statement::Effect(instruction));
            "0".into()
        } else {
            self.assign(locals, result.clone(), instruction)
        };
        self.guard_fault(locals);
        value
    }
}
