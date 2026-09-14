//! Function-owned LIFO cleanup and portable fault propagation. Host cancellation
//! (fuel/stack exhaustion) remains an external trap; language faults unwind callbacks.
use super::aggregates::mem;
use super::*;

impl Emitter<'_, '_> {
    pub(super) fn defer(
        &mut self,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let ty = self.expr(value, locals, depth)?;
        expect(&ty, &Type::Function(vec![], Box::new(Type::Unit)), span)?;
        let closure = self.temp(ValType::I32, span)?;
        self.emit(I::LocalSet(closure));
        let head = self
            .defer_head
            .ok_or_else(|| invalid(span, "missing cleanup stack"))?;
        let node = self.allocate_aggregate(
            0,
            &[(closure, ty), (head, Type::List(Box::new(Type::Unit)))],
            span,
        )?;
        self.emit(I::LocalGet(node));
        self.emit(I::LocalSet(head));
        self.emit(I::LocalGet(self.defer_root.unwrap()));
        self.emit(I::LocalGet(head));
        self.emit(I::I32Store(wasm_encoder::MemArg { align: 2, ..mem(0) }));
        self.emit(I::I32Const(0));
        Ok(Type::Unit)
    }

    pub(super) fn cleanups(&mut self, span: Span) -> Result<()> {
        let Some(head) = self.defer_head else {
            return Ok(());
        };
        let original_fault = self.temp(ValType::I32, span)?;
        let callback = self.temp(ValType::I32, span)?;
        let closure_ty = Type::Function(vec![], Box::new(Type::Unit));
        self.emit(I::GlobalGet(2));
        self.emit(I::LocalSet(original_fault));
        self.emit(I::Block(BlockType::Empty));
        self.emit(I::Loop(BlockType::Empty));
        self.emit(I::LocalGet(head));
        self.emit(I::I32Eqz);
        self.emit(I::BrIf(1));
        // Retain the head until callback exit, so its environment remains traced.
        self.load_cell(head, 0, &closure_ty, span)?;
        self.emit(I::LocalSet(callback));
        self.emit(I::I32Const(0));
        self.emit(I::GlobalSet(2));
        self.emit(I::Block(BlockType::Empty));
        self.exit_target = Some(self.control_depth);
        self.invoke_values(callback, &closure_ty, &[], span)?;
        self.emit(I::Drop);
        self.emit(I::End);
        self.exit_target = None;
        self.emit(I::LocalGet(original_fault));
        self.emit(I::GlobalGet(2));
        self.emit(I::I32Or);
        self.emit(I::LocalSet(original_fault));
        self.load_cell(head, 1, &Type::List(Box::new(Type::Unit)), span)?;
        self.emit(I::LocalSet(head));
        self.emit(I::LocalGet(self.defer_root.unwrap()));
        self.emit(I::LocalGet(head));
        self.emit(I::I32Store(wasm_encoder::MemArg { align: 2, ..mem(0) }));
        self.emit(I::Br(0));
        self.emit(I::End);
        self.emit(I::End);
        self.emit(I::LocalGet(original_fault));
        self.emit(I::GlobalSet(2));
        Ok(())
    }
}
