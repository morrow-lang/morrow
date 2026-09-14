//! Uniform native callback thunks preserve source widths and explicit fault propagation.
use super::*;
use crate::json_codec::Callback;

impl Emitter<'_> {
    pub(super) fn custom_codec(
        &mut self,
        prefix: &str,
        encode: &Callback,
        decode: &Callback,
        span: Span,
    ) -> Lowering<()> {
        let Callback::Function(id) = encode else {
            return Err(invalid(span, "unresolved custom JSON encoder"));
        };
        let function = self
            .functions
            .get(&id.0)
            .ok_or_else(|| invalid(span, "missing custom JSON encoder"))?;
        let Some(parameter) = function.params.first() else {
            return Err(invalid(span, "custom JSON encoder requires one argument"));
        };
        let managed = !matches!(
            self.representation(&parameter.ty),
            Type::Int | Type::Float | Type::Bool | Type::Unit | Type::Never
        );
        let encode = self.codec_callback(&format!("{prefix}_encode"), encode, span)?;
        let decode = self.codec_callback(&format!("{prefix}_decode"), decode, span)?;
        let callbacks = format!("{prefix}_callbacks");
        self.data.data(
            &callbacks,
            vec![
                DataValue::Word(native_operand(&encode)),
                DataValue::Word(native_operand(&decode)),
                DataValue::Word(Operand::Int(i64::from(managed))),
            ],
        );
        self.data.data(
            prefix,
            vec![
                DataValue::Word(Operand::Int(14)),
                DataValue::Word(Operand::Int(2)),
                DataValue::Word(native_operand(&callbacks)),
                DataValue::Word(Operand::Int(0)),
            ],
        );
        Ok(())
    }

    fn codec_callback(&mut self, name: &str, callback: &Callback, span: Span) -> Lowering<String> {
        let Callback::Function(id) = callback else {
            return Err(invalid(span, "unresolved custom JSON callback"));
        };
        let function = self
            .functions
            .get(&id.0)
            .ok_or_else(|| invalid(span, "missing custom JSON callback"))?;
        let [parameter] = function.params.as_slice() else {
            return Err(invalid(span, "custom JSON callback requires one argument"));
        };
        if !function.captures.is_empty() || function.mailbox.is_some() {
            return Err(invalid(
                span,
                "custom JSON callback must be a closed ordinary function",
            ));
        }
        let width = machine_width(self.width(parameter.ty.clone()));
        self.data.begin(
            name,
            Some(Scalar::I64),
            vec![
                (Scalar::I64, "%fault".into()),
                (Scalar::I64, "%bits".into()),
            ],
            false,
        );
        self.data.statement(Statement::Label("entry".into()));
        let argument = if width == Scalar::I64 {
            native_operand("%bits")
        } else {
            self.data.statement(Statement::Assign {
                destination: "%argument".into(),
                ty: width,
                operation: NativeOperation::Unary(
                    if width == Scalar::F64 {
                        MachineUnary::Cast
                    } else {
                        MachineUnary::Copy
                    },
                    native_operand("%bits"),
                ),
            });
            native_operand("%argument")
        };
        // No allocation occurs before the checked callee establishes its typed
        // roots. The caller keeps the incoming value rooted across this thunk.
        self.data.statement(Statement::Assign {
            destination: "%result".into(),
            ty: Scalar::I64,
            operation: NativeOperation::Call {
                callee: native_operand(&format!("$f{}", id.0)),
                args: vec![
                    (Scalar::I64, Operand::Int(0)),
                    (Scalar::I64, native_operand("%fault")),
                    (width, argument),
                ],
                variadic: None,
            },
        });
        self.data
            .statement(Statement::Return(Some(native_operand("%result"))));
        self.data.end();
        Ok(name.into())
    }
}
