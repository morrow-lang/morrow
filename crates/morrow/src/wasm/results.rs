//! Explicit Result propagation and typed with handlers share function cleanup exits.
use super::aggregates::mem;
use super::*;

impl Emitter<'_, '_> {
    fn propagate_error(&mut self, pointer: u32, error: &Type, span: Span) -> Result<()> {
        let Type::Result(_, expected) = self.return_type else {
            return Err(invalid(span, "error propagation requires Result return"));
        };
        expect(error, expected, span)?;
        self.emit(I::LocalGet(pointer));
        self.emit(I::LocalSet(self.return_slot.unwrap()));
        self.emit(I::Br(self.control_depth - self.exit_target.unwrap()));
        Ok(())
    }

    pub(super) fn try_result(
        &mut self,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let ty = self.expr(value, locals, depth)?;
        let Type::Result(success, error) = ty else {
            return Err(invalid(span, "try requires Result"));
        };
        let pointer = self.temp(ValType::I32, span)?;
        self.emit(I::LocalSet(pointer));
        self.emit(I::LocalGet(pointer));
        self.emit(I::I64Load(mem(8)));
        self.emit(I::I64Const(1));
        self.emit(I::I64Eq);
        self.emit(I::If(BlockType::Empty));
        self.propagate_error(pointer, &error, span)?;
        self.emit(I::End);
        self.load_cell(pointer, 0, &success, span)?;
        Ok(*success)
    }

    pub(super) fn with_result(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Result<Type> {
        let ExprKind::With {
            steps,
            body,
            handlers,
        } = &expr.kind
        else {
            return Err(invalid(expr.span, "expected with expression"));
        };
        let result = &expr.ty;
        let span = expr.span;
        if steps.is_empty() || steps.len() > 4096 || handlers.len() > 1024 {
            return Err(invalid(span, "with step/handler limit exceeded"));
        }
        self.emit(I::Block(block_type(result, span)?));
        let done = self.control_depth;
        let mut scoped = locals.clone();
        for step in steps {
            let ty = self.expr(&step.value, &mut scoped, depth)?;
            let Type::Result(success, error) = ty else {
                return Err(invalid(span, "with step requires Result"));
            };
            let pointer = self.temp(ValType::I32, span)?;
            self.emit(I::LocalSet(pointer));
            self.emit(I::LocalGet(pointer));
            self.emit(I::I64Load(mem(8)));
            self.emit(I::I64Const(1));
            self.emit(I::I64Eq);
            self.emit(I::If(BlockType::Empty));
            if let Some(handler) = step.error_handler {
                let handler = handlers
                    .get(handler)
                    .ok_or_else(|| invalid(span, "unknown with handler"))?;
                expect(&error, &handler.error.ty, span)?;
                let mut scope = locals.clone();
                self.load_cell(pointer, 0, &error, span)?;
                let slot = self.bind(&mut scope, handler.error.id, &error, span)?;
                self.emit(I::LocalSet(slot));
                let actual = self.expr(&handler.body, &mut scope, depth)?;
                expect(&actual, result, span)?;
                self.emit(I::Br(self.control_depth - done));
            } else {
                self.propagate_error(pointer, &error, span)?;
            }
            self.emit(I::End);
            self.load_cell(pointer, 0, &success, span)?;
            let value = self.temp(value_type(&success, span)?, span)?;
            self.emit(I::LocalSet(value));
            self.pattern(&step.pattern, value, &success, &mut scoped, span)?;
            self.emit(I::I32Eqz);
            self.emit(I::If(BlockType::Empty));
            self.emit(I::Unreachable);
            self.emit(I::End);
        }
        let actual = self.expr(body, &mut scoped, depth)?;
        expect(&actual, result, span)?;
        self.emit(I::End);
        Ok(result.clone())
    }
}
