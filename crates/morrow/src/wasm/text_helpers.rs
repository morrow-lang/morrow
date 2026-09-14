//! Portable lexical string comparison and bounded immutable joining, without host imports.
fn mem(offset: u64) -> wasm_encoder::MemArg {
    wasm_encoder::MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}
use super::*;

impl Emitter<'_, '_> {
    pub(super) fn compare_text(
        &mut self,
        args: &[Expr],
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let [left, right] = args else {
            return Err(invalid(span, "string comparison requires two arguments"));
        };
        let a = self.temp(ValType::I32, span)?;
        let b = self.temp(ValType::I32, span)?;
        let index = self.temp(ValType::I32, span)?;
        let order = self.temp(ValType::I64, span)?;
        let ca = self.temp(ValType::I32, span)?;
        let cb = self.temp(ValType::I32, span)?;
        let first = self.expr(left, locals, depth)?;
        expect(&first, &Type::String, left.span)?;
        self.emit(I::LocalSet(a));
        let second = self.expr(right, locals, depth)?;
        expect(&second, &Type::String, right.span)?;
        self.emit(I::LocalSet(b));
        for instruction in [
            I::I32Const(0),
            I::LocalSet(index),
            I::I64Const(0),
            I::LocalSet(order),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(index),
            I::LocalGet(a),
            I::I32Load(mem(0)),
            I::I32GeU,
            I::LocalGet(index),
            I::LocalGet(b),
            I::I32Load(mem(0)),
            I::I32GeU,
            I::I32Or,
            I::BrIf(1),
            I::LocalGet(a),
            I::LocalGet(index),
            I::I32Add,
            I::I32Load8U(wasm_encoder::MemArg {
                offset: 8,
                align: 0,
                memory_index: 0,
            }),
            I::LocalSet(ca),
            I::LocalGet(b),
            I::LocalGet(index),
            I::I32Add,
            I::I32Load8U(wasm_encoder::MemArg {
                offset: 8,
                align: 0,
                memory_index: 0,
            }),
            I::LocalSet(cb),
            I::LocalGet(ca),
            I::LocalGet(cb),
            I::I32Ne,
            I::If(BlockType::Empty),
            I::I64Const(-1),
            I::I64Const(1),
            I::LocalGet(ca),
            I::LocalGet(cb),
            I::I32LtU,
            I::Select,
            I::LocalSet(order),
            I::Br(2),
            I::End,
            I::LocalGet(index),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(index),
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(order),
            I::I64Eqz,
            I::If(BlockType::Empty),
            I::LocalGet(a),
            I::I32Load(mem(0)),
            I::LocalGet(b),
            I::I32Load(mem(0)),
            I::I32LtU,
            I::If(BlockType::Result(ValType::I64)),
            I::I64Const(-1),
            I::Else,
            I::LocalGet(a),
            I::I32Load(mem(0)),
            I::LocalGet(b),
            I::I32Load(mem(0)),
            I::I32GtU,
            I::I64ExtendI32U,
            I::End,
            I::LocalSet(order),
            I::End,
            I::LocalGet(order),
        ] {
            self.emit(instruction);
        }
        Ok(if first == Type::Never || second == Type::Never {
            Type::Never
        } else {
            Type::Int
        })
    }

    pub(super) fn join_text(
        &mut self,
        args: &[Expr],
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let [input, separator] = args else {
            return Err(invalid(span, "string join requires two arguments"));
        };
        let runtime = self
            .strings
            .ok_or_else(|| invalid(span, "missing string heap"))?;
        let first = runtime.first;
        let empty = *runtime
            .literals
            .get("")
            .ok_or_else(|| invalid(span, "missing empty string literal"))?;
        let list = self.temp(ValType::I32, span)?;
        let separator_local = self.temp(ValType::I32, span)?;
        let output = self.temp(ValType::I32, span)?;
        let index = self.temp(ValType::I32, span)?;
        let input_type = self.expr(input, locals, depth)?;
        expect(&input_type, &Type::List(Box::new(Type::String)), input.span)?;
        self.emit(I::LocalSet(list));
        let separator_type = self.expr(separator, locals, depth)?;
        expect(&separator_type, &Type::String, separator.span)?;
        self.emit(I::LocalSet(separator_local));
        for instruction in [
            I::I32Const(empty),
            I::LocalSet(output),
            I::I32Const(0),
            I::LocalSet(index),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(index),
            I::LocalGet(list),
            I::I64Load(mem(8)),
            I::I32WrapI64,
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(index),
            I::I32Const(0),
            I::I32GtU,
            I::If(BlockType::Empty),
            I::LocalGet(output),
            I::LocalGet(separator_local),
            I::Call(first + strings::CONCAT),
        ] {
            self.emit(instruction);
        }
        self.root_string(span)?;
        self.emit(I::LocalSet(output));
        self.emit(I::End);
        self.emit(I::LocalGet(output));
        self.dynamic_cell(list, index, &Type::String, span)?;
        self.emit(I::Call(first + strings::CONCAT));
        self.root_string(span)?;
        for instruction in [
            I::LocalSet(output),
            I::LocalGet(index),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(index),
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(output),
        ] {
            self.emit(instruction);
        }
        Ok(
            if input_type == Type::Never || separator_type == Type::Never {
                Type::Never
            } else {
                Type::String
            },
        )
    }
}
