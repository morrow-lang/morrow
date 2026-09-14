//! Typed closure environments reuse the precise aggregate heap. Dispatch is closed
//! over checked function identities; no host-supplied table index or raw pointer escapes.
use super::aggregates::mem;
use super::*;

impl Emitter<'_, '_> {
    pub(super) fn closure(
        &mut self,
        id: ir::FunctionId,
        captures: &[Expr],
        ty: &Type,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let (index, function) = self
            .functions
            .get(&id.0)
            .ok_or_else(|| invalid(span, "unknown closure identity"))?;
        let signature = Type::Function(
            function.params.iter().map(|p| p.ty.clone()).collect(),
            Box::new(function.return_type.clone()),
        );
        expect(ty, &signature, span)?;
        if captures.len() != function.captures.len() {
            return Err(invalid(span, "closure capture count mismatch"));
        }
        let mut values = Vec::new();
        for (capture, param) in captures.iter().zip(&function.captures) {
            let actual = self.expr(capture, locals, depth)?;
            expect(&actual, &param.ty, capture.span)?;
            let slot = self.temp(value_type(&param.ty, span)?, span)?;
            self.emit(I::LocalSet(slot));
            values.push((slot, param.ty.clone()));
        }
        let pointer = self.allocate_aggregate(i64::from(*index), &values, span)?;
        self.emit(I::LocalGet(pointer));
        Ok(ty.clone())
    }

    pub(super) fn invoke_values(
        &mut self,
        closure: u32,
        signature: &Type,
        args: &[(u32, Type)],
        span: Span,
    ) -> Result<Type> {
        let Type::Function(params, result) = signature else {
            return Err(invalid(span, "invocation requires callable type"));
        };
        if params.len() != args.len() {
            return Err(invalid(span, "callable argument count mismatch"));
        }
        for ((_, actual), expected) in args.iter().zip(params) {
            expect(actual, expected, span)?;
        }
        self.emit(I::Block(block_type(result, span)?));
        for (index, function) in self.functions.values() {
            if function.mailbox.is_some()
                || function.return_type != **result
                || function.params.iter().map(|p| &p.ty).ne(params.iter())
            {
                continue;
            }
            self.emit(I::LocalGet(closure));
            self.emit(I::I64Load(mem(8)));
            self.emit(I::I64Const(i64::from(*index)));
            self.emit(I::I64Eq);
            self.emit(I::If(BlockType::Empty));
            for (offset, capture) in function.captures.iter().enumerate() {
                self.load_cell(closure, offset, &capture.ty, span)?;
            }
            for (slot, _) in args {
                self.emit(I::LocalGet(*slot));
            }
            self.emit(I::Call(*index));
            self.emit(I::Br(1));
            self.emit(I::End);
        }
        self.emit(I::Unreachable);
        self.emit(I::End);
        Ok(*result.clone())
    }
}
