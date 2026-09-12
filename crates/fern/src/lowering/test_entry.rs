//! Test execution cannot count an explicit process exit as a returned test result.
use super::*;

impl Emitter<'_> {
    /// Redirect the audited runtime exit identity only in test mode, including lifted callables.
    pub(super) fn test_runtime_symbol<'s>(&self, symbol: &'s str) -> &'s str {
        if self.test_mode && symbol == "fern_exit" {
            "fern_rs_test_exit"
        } else {
            symbol
        }
    }

    /// Emit a diagnostic and failing process exit; unused application exit calls remain inert.
    /// The source status is intentionally ignored. Normal compilation emits no helper or message.
    pub(super) fn test_exit_helper(&mut self) {
        if !self.test_mode {
            return;
        }
        const MESSAGE: &str = "fern: System.exit cannot terminate a test";
        let bytes = MESSAGE.len() + 1;
        self.data.data(
            "$fern_rs_test_exit_message",
            vec![
                DataValue::Bytes(MESSAGE.as_bytes().to_vec()),
                DataValue::Bytes(vec![10]),
            ],
        );
        self.output.begin(
            "$fern_rs_test_exit",
            None,
            vec![(Scalar::I64, "%status".to_owned())],
            false,
        );
        self.output.statement(Statement::Label("@start".to_owned()));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$write"),
                args: vec![
                    (Scalar::I32, native_operand("2")),
                    (Scalar::I64, native_operand("$fern_rs_test_exit_message")),
                    (Scalar::I64, native_operand(&(bytes).to_string())),
                ],
                variadic: None,
            }));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$fern_exit"),
                args: vec![(Scalar::I64, native_operand("1"))],
                variadic: None,
            }));
        self.output.statement(Statement::Return(None));
        self.output.end();
    }
}
