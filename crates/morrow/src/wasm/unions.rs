//! Structural unions trace only the concrete selected payload and remap subset tags.
use super::aggregates::mem;
use super::*;

impl Emitter<'_, '_> {
    pub(super) fn union_conversion(
        &mut self,
        expr: &Expr,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Result<Type> {
        let Type::Union(target) = &expr.ty else {
            return Err(invalid(expr.span, "union conversion requires Union target"));
        };
        let actual = self.expr(value, locals, depth)?;
        let pointer = self.temp(value_type(&actual, expr.span)?, expr.span)?;
        self.emit(I::LocalSet(pointer));
        if matches!(expr.kind, ExprKind::UnionInject { .. }) {
            let tag = target
                .iter()
                .position(|ty| ty == &actual)
                .ok_or_else(|| invalid(expr.span, "injected value is not an exact union member"))?;
            let result = self.allocate_aggregate(tag as i64, &[(pointer, actual)], expr.span)?;
            self.emit(I::LocalGet(result));
        } else {
            let Type::Union(source) = actual else {
                return Err(invalid(expr.span, "union widening requires Union source"));
            };
            if !source.iter().all(|member| target.contains(member)) {
                return Err(invalid(expr.span, "union widening requires a subset"));
            }
            self.union_rebox(pointer, &source, target, expr.span)?;
        }
        Ok(expr.ty.clone())
    }

    fn union_rebox(
        &mut self,
        pointer: u32,
        source: &[Type],
        target: &[Type],
        span: Span,
    ) -> Result<()> {
        self.emit(I::Block(BlockType::Result(ValType::I32)));
        for (old, member) in source.iter().enumerate() {
            let Some(new) = target.iter().position(|ty| ty == member) else {
                continue;
            };
            self.emit(I::LocalGet(pointer));
            self.emit(I::I64Load(mem(8)));
            self.emit(I::I64Const(old as i64));
            self.emit(I::I64Eq);
            self.emit(I::If(BlockType::Empty));
            self.load_cell(pointer, 0, member, span)?;
            let value = self.temp(value_type(member, span)?, span)?;
            self.emit(I::LocalSet(value));
            let boxed = self.allocate_aggregate(new as i64, &[(value, member.clone())], span)?;
            self.emit(I::LocalGet(boxed));
            self.emit(I::Br(1));
            self.emit(I::End);
        }
        self.emit(I::Unreachable);
        self.emit(I::End);
        Ok(())
    }

    pub(super) fn union_pattern(
        &mut self,
        narrowed: &Type,
        binding: Option<&ir::Param>,
        pointer: u32,
        ty: &Type,
        locals: &mut Locals,
        span: Span,
    ) -> Result<()> {
        let Type::Union(source) = ty else {
            return Err(invalid(span, "union selection requires Union"));
        };
        let members = if let Type::Union(members) = narrowed {
            members.clone()
        } else {
            vec![narrowed.clone()]
        };
        if !members.iter().all(|member| source.contains(member)) {
            return Err(invalid(span, "union selection requires a subset"));
        }
        self.emit(I::I32Const(0));
        for (tag, member) in source.iter().enumerate() {
            if members.contains(member) {
                self.emit(I::LocalGet(pointer));
                self.emit(I::I64Load(mem(8)));
                self.emit(I::I64Const(tag as i64));
                self.emit(I::I64Eq);
                self.emit(I::I32Or);
            }
        }
        if let Some(binding) = binding {
            expect(&binding.ty, narrowed, span)?;
            let slot = self.bind(locals, binding.id, narrowed, span)?;
            self.emit(I::If(BlockType::Result(ValType::I32)));
            if matches!(narrowed, Type::Union(_)) {
                self.union_rebox(pointer, source, &members, span)?;
            } else {
                self.load_cell(pointer, 0, narrowed, span)?;
            }
            if managed(narrowed) {
                self.root_string(span)?;
            }
            self.emit(I::LocalSet(slot));
            self.emit(I::I32Const(1));
            self.emit(I::Else);
            self.emit(I::I32Const(0));
            self.emit(I::End);
        }
        Ok(())
    }
}
