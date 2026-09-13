//! Immutable aggregate values keep full-width cells and explicit semantic child types.
use super::*;
use crate::Constructor;
use wasm_encoder::MemArg;

pub(super) fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 3,
        memory_index: 0,
    }
}

impl Emitter<'_, '_> {
    fn fields(&self, ty: &Type, tag: usize, span: Span) -> Result<Vec<Type>> {
        match ty {
            Type::Unit if tag == 0 => Ok(vec![]),
            Type::Tuple(fields) if tag == 0 => Ok(fields.clone()),
            Type::Named(_, _) => self
                .strings
                .and_then(|runtime| runtime.layouts.get(ty))
                .and_then(|layout| layout.variants.get(tag))
                .cloned()
                .ok_or_else(|| invalid(span, "missing aggregate layout or invalid variant tag")),
            Type::Option(value) => match tag {
                0 => Ok(vec![*value.clone()]),
                1 => Ok(vec![]),
                _ => Err(invalid(span, "invalid Option tag")),
            },
            Type::Result(ok, error) => match tag {
                0 => Ok(vec![*ok.clone()]),
                1 => Ok(vec![*error.clone()]),
                _ => Err(invalid(span, "invalid Result tag")),
            },
            _ => Err(invalid(span, "type has no aggregate fields")),
        }
    }

    pub(super) fn load_cell(
        &mut self,
        pointer: u32,
        index: usize,
        ty: &Type,
        span: Span,
    ) -> Result<()> {
        if index >= 511 {
            return Err(invalid(span, "aggregate field limit exceeded"));
        }
        self.emit(I::LocalGet(pointer));
        self.emit(match value_type(ty, span)? {
            ValType::I64 => I::I64Load(mem(16 + index as u64 * 8)),
            ValType::F64 => I::F64Load(mem(16 + index as u64 * 8)),
            _ => I::I32Load(MemArg {
                align: 2,
                ..mem(16 + index as u64 * 8)
            }),
        });
        Ok(())
    }

    pub(super) fn allocate_aggregate(
        &mut self,
        tag: i64,
        values: &[(u32, Type)],
        span: Span,
    ) -> Result<u32> {
        if values.len() > 511 {
            return Err(invalid(span, "aggregate field limit exceeded"));
        }
        let first = self
            .strings
            .ok_or_else(|| invalid(span, "missing managed heap"))?
            .first;
        let pointer = self.temp(ValType::I32, span)?;
        self.emit(I::I32Const((8 + values.len() * 8) as i32));
        self.emit(I::Call(first + strings::ALLOC));
        self.emit(I::LocalTee(pointer));
        self.emit(I::I64Const(tag));
        self.emit(I::I64Store(mem(8)));
        for (index, (local, ty)) in values.iter().enumerate() {
            self.emit(I::LocalGet(pointer));
            self.emit(I::LocalGet(*local));
            self.emit(match value_type(ty, span)? {
                ValType::I64 => I::I64Store(mem(16 + index as u64 * 8)),
                ValType::F64 => I::F64Store(mem(16 + index as u64 * 8)),
                _ => I::I32Store(MemArg {
                    align: 2,
                    ..mem(16 + index as u64 * 8)
                }),
            });
            if managed(ty) {
                let address = self.temp(ValType::I32, span)?;
                self.emit(I::LocalGet(pointer));
                self.emit(I::I32Const(strings::HEAP_START));
                self.emit(I::I32Sub);
                self.emit(I::I32Const(strings::STRIDE));
                self.emit(I::I32DivU);
                self.emit(I::I32Const(64));
                self.emit(I::I32Mul);
                self.emit(I::I32Const(
                    super::super::heap::META_BASE + ((index + 1) / 8) as i32,
                ));
                self.emit(I::I32Add);
                self.emit(I::LocalTee(address));
                self.emit(I::LocalGet(address));
                self.emit(I::I32Load8U(MemArg {
                    offset: 0,
                    align: 0,
                    memory_index: 0,
                }));
                self.emit(I::I32Const(1 << ((index + 1) % 8)));
                self.emit(I::I32Or);
                self.emit(I::I32Store8(MemArg {
                    offset: 0,
                    align: 0,
                    memory_index: 0,
                }));
            }
        }
        Ok(pointer)
    }

    pub(super) fn aggregate(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Result<Type> {
        let (tag, values, expected): (i64, Vec<&Expr>, Vec<Type>) = match &expr.kind {
            ExprKind::Field { value, index } => {
                let actual = self.expr(value, locals, depth)?;
                let fields = self.fields(&actual, 0, expr.span)?;
                let field = fields
                    .get(*index)
                    .ok_or_else(|| invalid(expr.span, "field index exceeds aggregate layout"))?;
                expect(field, &expr.ty, expr.span)?;
                let pointer = self.temp(ValType::I32, expr.span)?;
                self.emit(I::LocalSet(pointer));
                self.load_cell(pointer, *index, field, expr.span)?;
                return Ok(field.clone());
            }
            ExprKind::Unwrap(value) => {
                let actual = self.expr(value, locals, depth)?;
                let fields = self.fields(&actual, 0, expr.span)?;
                if fields.len() != 1 {
                    return Err(invalid(expr.span, "newtype must contain one field"));
                }
                expect(&fields[0], &expr.ty, expr.span)?;
                let pointer = self.temp(ValType::I32, expr.span)?;
                self.emit(I::LocalSet(pointer));
                self.load_cell(pointer, 0, &fields[0], expr.span)?;
                return Ok(expr.ty.clone());
            }
            ExprKind::Wrap(value) => (0, vec![value], self.fields(&expr.ty, 0, expr.span)?),
            ExprKind::Tuple(values) => (
                0,
                values.iter().collect(),
                self.fields(&expr.ty, 0, expr.span)?,
            ),
            ExprKind::List(values) => {
                let Type::List(item) = &expr.ty else {
                    return Err(invalid(expr.span, "list has wrong semantic type"));
                };
                (
                    values.len() as i64,
                    values.iter().collect(),
                    vec![*item.clone(); values.len()],
                )
            }
            ExprKind::CustomConstruct { tag, fields } => (
                *tag as i64,
                fields.iter().collect(),
                self.fields(&expr.ty, *tag, expr.span)?,
            ),
            ExprKind::Construct { constructor, value } => {
                let tag = constructor_tag(*constructor);
                (
                    tag as i64,
                    value.iter().map(|value| value.as_ref()).collect(),
                    self.fields(&expr.ty, tag, expr.span)?,
                )
            }
            _ => return Err(invalid(expr.span, "unsupported aggregate construction")),
        };
        if values.len() != expected.len() {
            return Err(invalid(expr.span, "aggregate constructor arity mismatch"));
        }
        let mut stored = Vec::new();
        for (value, expected) in values.iter().zip(&expected) {
            let actual = self.expr(value, locals, depth)?;
            expect(&actual, expected, value.span)?;
            let local = self.temp(value_type(expected, value.span)?, value.span)?;
            self.emit(I::LocalSet(local));
            stored.push((local, expected.clone()));
        }
        let pointer = self.allocate_aggregate(tag, &stored, expr.span)?;
        self.emit(I::LocalGet(pointer));
        Ok(expr.ty.clone())
    }

    pub(super) fn aggregate_pattern(
        &mut self,
        pattern: &Pattern,
        value: u32,
        ty: &Type,
        locals: &mut Locals,
        span: Span,
    ) -> Result<()> {
        if let Pattern::List { prefix, rest } = pattern {
            let Type::List(item) = ty else {
                return Err(invalid(span, "list pattern has non-list input"));
            };
            self.emit(I::LocalGet(value));
            self.emit(I::I64Load(mem(8)));
            self.emit(I::I64Const(prefix.len() as i64));
            self.emit(if rest.is_some() { I::I64GeU } else { I::I64Eq });
            self.emit(I::If(BlockType::Result(ValType::I32)));
            self.emit(I::I32Const(1));
            for (index, pattern) in prefix.iter().enumerate() {
                self.load_cell(value, index, item, span)?;
                let slot = self.temp(value_type(item, span)?, span)?;
                self.emit(I::LocalSet(slot));
                self.pattern(pattern, slot, item, locals, span)?;
                self.emit(I::I32And);
            }
            if let Some(rest) = rest {
                let suffix = self.slice_list(value, prefix.len(), item, span)?;
                self.pattern(rest, suffix, ty, locals, span)?;
                self.emit(I::I32And);
            }
            self.emit(I::Else);
            self.emit(I::I32Const(0));
            self.emit(I::End);
            return Ok(());
        }
        if let Pattern::TupleRest { prefix, rest } = pattern {
            let fields = self.fields(ty, 0, span)?;
            if prefix.len() > fields.len() {
                return Err(invalid(span, "tuple prefix exceeds its fields"));
            }
            self.emit(I::I32Const(1));
            for (index, pattern) in prefix.iter().enumerate() {
                let field = &fields[index];
                self.load_cell(value, index, field, span)?;
                let slot = self.temp(value_type(field, span)?, span)?;
                self.emit(I::LocalSet(slot));
                self.pattern(pattern, slot, field, locals, span)?;
                self.emit(I::I32And);
            }
            let mut suffix = Vec::new();
            for (index, field) in fields.iter().enumerate().skip(prefix.len()) {
                self.load_cell(value, index, field, span)?;
                let slot = self.temp(value_type(field, span)?, span)?;
                self.emit(I::LocalSet(slot));
                suffix.push((slot, field.clone()));
            }
            let tail_type = if suffix.is_empty() {
                Type::Unit
            } else {
                Type::Tuple(fields[prefix.len()..].to_vec())
            };
            let tail = if suffix.is_empty() {
                let slot = self.temp(ValType::I32, span)?;
                self.emit(I::I32Const(0));
                self.emit(I::LocalSet(slot));
                slot
            } else {
                let slot = self.allocate_aggregate(0, &suffix, span)?;
                self.emit(I::LocalGet(slot));
                self.root_string(span)?;
                self.emit(I::Drop);
                slot
            };
            self.pattern(rest, tail, &tail_type, locals, span)?;
            self.emit(I::I32And);
            return Ok(());
        }
        let (tag, patterns, types) = match pattern {
            Pattern::String(text) if *ty == Type::String => {
                let runtime = self
                    .strings
                    .ok_or_else(|| invalid(span, "missing string heap"))?;
                let pointer = runtime
                    .literals
                    .get(text)
                    .ok_or_else(|| invalid(span, "missing string pattern literal"))?;
                self.emit(I::LocalGet(value));
                self.emit(I::I32Const(*pointer));
                self.emit(I::Call(runtime.first + strings::EQ));
                return Ok(());
            }
            Pattern::Tuple(patterns) => (None, patterns.clone(), self.fields(ty, 0, span)?),
            Pattern::Variant { tag, fields } => {
                (Some(*tag), fields.clone(), self.fields(ty, *tag, span)?)
            }
            Pattern::Newtype(pattern) => (None, vec![*pattern.clone()], self.fields(ty, 0, span)?),
            Pattern::Constructor {
                constructor,
                binding,
            } => {
                let tag = constructor_tag(*constructor);
                let types = self.fields(ty, tag, span)?;
                let patterns = if types.is_empty() {
                    vec![]
                } else {
                    vec![binding.map(Pattern::Bind).unwrap_or(Pattern::Wildcard)]
                };
                (Some(tag), patterns, types)
            }
            _ => {
                return Err(invalid(
                    span,
                    "pattern requires unsupported aggregate representation",
                ));
            }
        };
        if patterns.len() != types.len() {
            return Err(invalid(
                span,
                "pattern arity does not match aggregate layout",
            ));
        }
        if let Some(tag) = tag {
            self.emit(I::LocalGet(value));
            self.emit(I::I64Load(mem(8)));
            self.emit(I::I64Const(tag as i64));
            self.emit(I::I64Eq);
            self.emit(I::If(BlockType::Result(ValType::I32)));
        }
        self.emit(I::I32Const(1));
        for (index, (pattern, field)) in patterns.iter().zip(&types).enumerate() {
            self.load_cell(value, index, field, span)?;
            let slot = self.temp(value_type(field, span)?, span)?;
            self.emit(I::LocalSet(slot));
            self.pattern(pattern, slot, field, locals, span)?;
            self.emit(I::I32And);
        }
        if tag.is_some() {
            self.emit(I::Else);
            self.emit(I::I32Const(0));
            self.emit(I::End);
        }
        Ok(())
    }
}

fn constructor_tag(constructor: Constructor) -> usize {
    match constructor {
        Constructor::Some | Constructor::Ok => 0,
        Constructor::None | Constructor::Err => 1,
    }
}
