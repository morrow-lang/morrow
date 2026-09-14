//! Lazy i64 ranges and lexical loop exits, with bounded transient roots per turn.
use super::aggregates::mem;
use super::*;

impl Emitter<'_, '_> {
    pub(super) fn range(
        &mut self,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let mut values = Vec::new();
        for expr in [start, end] {
            let actual = self.expr(expr, locals, depth)?;
            expect(&actual, &Type::Int, span)?;
            let slot = self.temp(ValType::I64, span)?;
            self.emit(I::LocalSet(slot));
            values.push((slot, Type::Int));
        }
        let pointer = self.allocate_aggregate(i64::from(inclusive), &values, span)?;
        self.emit(I::LocalGet(pointer));
        Ok(Type::Range)
    }

    pub(super) fn iteration(
        &mut self,
        pattern: &Pattern,
        iterable: &Expr,
        body: &Expr,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let ty = self.expr(iterable, locals, depth)?;
        let collection = self.temp(ValType::I32, span)?;
        self.emit(I::LocalSet(collection));
        let index = self.temp(ValType::I64, span)?;
        let end = self.temp(ValType::I64, span)?;
        let inclusive = self.temp(ValType::I32, span)?;
        let item = match &ty {
            Type::Range => {
                self.load_cell(collection, 0, &Type::Int, span)?;
                self.emit(I::LocalSet(index));
                self.load_cell(collection, 1, &Type::Int, span)?;
                self.emit(I::LocalSet(end));
                self.emit(I::LocalGet(collection));
                self.emit(I::I64Load(mem(8)));
                self.emit(I::I32WrapI64);
                self.emit(I::LocalSet(inclusive));
                Type::Int
            }
            Type::List(_) | Type::Map(_, _) => {
                self.emit(I::I64Const(0));
                self.emit(I::LocalSet(index));
                self.emit(I::LocalGet(collection));
                self.emit(I::I64Load(mem(8)));
                self.emit(I::LocalSet(end));
                self.emit(I::I32Const(0));
                self.emit(I::LocalSet(inclusive));
                match &ty {
                    Type::List(item) => *item.clone(),
                    Type::Map(key, value) => Type::Tuple(vec![*key.clone(), *value.clone()]),
                    _ => unreachable!(),
                }
            }
            _ => return Err(invalid(span, "iteration requires a supported collection")),
        };
        let mark = self.temp(ValType::I32, span)?;
        self.emit(I::GlobalGet(0));
        self.emit(I::LocalSet(mark));
        self.emit(I::Block(BlockType::Empty));
        let done = self.control_depth;
        self.emit(I::Loop(BlockType::Empty));
        self.emit(I::LocalGet(mark));
        self.emit(I::GlobalSet(0));
        self.emit(I::LocalGet(index));
        self.emit(I::LocalGet(end));
        self.emit(I::I64LtS);
        self.emit(I::LocalGet(index));
        self.emit(I::LocalGet(end));
        self.emit(I::I64Eq);
        self.emit(I::LocalGet(inclusive));
        self.emit(I::I32And);
        self.emit(I::I32Or);
        self.emit(I::I32Eqz);
        self.emit(I::BrIf(1));
        self.emit(I::Block(BlockType::Empty));
        self.loops.push((done, self.control_depth));
        if ty == Type::Range {
            self.emit(I::LocalGet(index));
        } else {
            let offset = self.temp(ValType::I32, span)?;
            self.emit(I::LocalGet(index));
            self.emit(I::I32WrapI64);
            self.emit(I::LocalSet(offset));
            self.dynamic_cell(collection, offset, &item, span)?;
        }
        let value = self.temp(value_type(&item, span)?, span)?;
        self.emit(I::LocalSet(value));
        let mut scoped = locals.clone();
        self.pattern(pattern, value, &item, &mut scoped, span)?;
        self.emit(I::I32Eqz);
        self.emit(I::If(BlockType::Empty));
        self.emit(I::Unreachable);
        self.emit(I::End);
        let actual = self.expr(body, &mut scoped, depth)?;
        if actual != Type::Never {
            self.emit(I::Drop);
        }
        self.loops.pop();
        self.emit(I::End);
        // Compare before increment so an inclusive MAX endpoint never wraps.
        self.emit(I::LocalGet(index));
        self.emit(I::LocalGet(end));
        self.emit(I::I64Eq);
        self.emit(I::BrIf(1));
        self.emit(I::LocalGet(index));
        self.emit(I::I64Const(1));
        self.emit(I::I64Add);
        self.emit(I::LocalSet(index));
        self.emit(I::Br(0));
        self.emit(I::End);
        self.emit(I::End);
        self.emit(I::LocalGet(mark));
        self.emit(I::GlobalSet(0));
        self.emit(I::I32Const(0));
        Ok(Type::Unit)
    }
}
