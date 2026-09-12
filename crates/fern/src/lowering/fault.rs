//! An explicit invocation context carries numeric faults across typed call boundaries.
use super::*;

impl Emitter<'_> {
    /// A failed call never exposes its neutral ABI payload to the source program.
    pub(super) fn guard_fault(&mut self, locals: &mut Locals) {
        let code = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
        );
        let failed = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                native_operand(&(code)),
                native_operand("0"),
            ),
        );
        let unwind = locals.label();
        let resume = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(failed)),
            then_label: (unwind).to_string(),
            else_label: (resume).to_string(),
        });
        self.start_block(locals, &unwind);
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand("%return_slot"),
        });
        self.output.statement(Statement::Jump("@return".to_owned()));
        self.start_block(locals, &resume);
    }
}
