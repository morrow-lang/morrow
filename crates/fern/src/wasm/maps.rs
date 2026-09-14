//! Insertion-ordered immutable maps store precisely traced key/value tuples.
use super::aggregates::mem;
use super::*;
use ir::Builtin as F;

impl Emitter<'_, '_> {
    pub(super) fn new_list(&mut self, len: u32, span: Span) -> Result<u32> {
        let first = self
            .strings
            .ok_or_else(|| invalid(span, "missing managed heap"))?
            .first;
        let pointer = self.temp(ValType::I32, span)?;
        self.emit(I::LocalGet(len));
        self.emit(I::I32Const(511));
        self.emit(I::I32GtU);
        self.emit(I::If(BlockType::Empty));
        self.emit(I::Unreachable);
        self.emit(I::End);
        self.emit(I::LocalGet(len));
        self.emit(I::I32Const(8));
        self.emit(I::I32Mul);
        self.emit(I::I32Const(8));
        self.emit(I::I32Add);
        self.emit(I::Call(first + strings::ALLOC));
        self.emit(I::LocalTee(pointer));
        self.emit(I::LocalGet(len));
        self.emit(I::I64ExtendI32U);
        self.emit(I::I64Store(mem(8)));
        self.emit(I::LocalGet(pointer));
        self.root_string(span)?;
        self.emit(I::Drop);
        Ok(pointer)
    }

    pub(super) fn store_dynamic(
        &mut self,
        pointer: u32,
        index: u32,
        value: u32,
        ty: &Type,
        span: Span,
    ) -> Result<()> {
        self.emit(I::LocalGet(pointer));
        self.emit(I::LocalGet(index));
        self.emit(I::I32Const(8));
        self.emit(I::I32Mul);
        self.emit(I::I32Add);
        self.emit(I::LocalGet(value));
        self.emit(match value_type(ty, span)? {
            ValType::I64 => I::I64Store(mem(16)),
            ValType::F64 => I::F64Store(mem(16)),
            _ => I::I32Store(wasm_encoder::MemArg {
                align: 2,
                ..mem(16)
            }),
        });
        if managed(ty) {
            let cell = self.temp(ValType::I32, span)?;
            self.emit(I::LocalGet(index));
            self.emit(I::I32Const(1));
            self.emit(I::I32Add);
            self.emit(I::LocalSet(cell));
            self.mark_cell(pointer, cell, span)?;
        }
        Ok(())
    }

    pub(super) fn equal_values(
        &mut self,
        left: u32,
        right: u32,
        ty: &Type,
        span: Span,
    ) -> Result<()> {
        if matches!(ty, Type::Named(_, _)) {
            let fields = self.fields(ty, 0, span)?;
            if fields.len() != 1 {
                return Err(invalid(span, "map key requires a scalar newtype"));
            }
            let inner = &fields[0];
            let a = self.temp(value_type(inner, span)?, span)?;
            let b = self.temp(value_type(inner, span)?, span)?;
            self.load_cell(left, 0, inner, span)?;
            self.emit(I::LocalSet(a));
            self.load_cell(right, 0, inner, span)?;
            self.emit(I::LocalSet(b));
            return self.equal_values(a, b, inner, span);
        }
        self.emit(I::LocalGet(left));
        self.emit(I::LocalGet(right));
        self.emit(match ty {
            Type::Int => I::I64Eq,
            Type::Bool | Type::Unit => I::I32Eq,
            Type::Float => I::F64Eq,
            Type::String => I::Call(
                self.strings
                    .ok_or_else(|| invalid(span, "missing string heap"))?
                    .first
                    + strings::EQ,
            ),
            _ => return Err(invalid(span, "unsupported collection equality")),
        });
        Ok(())
    }

    pub(super) fn map_literal(
        &mut self,
        entries: &[(Expr, Expr)],
        ty: &Type,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let Type::Map(key, value) = ty else {
            return Err(invalid(span, "map literal has non-map type"));
        };
        if entries.len() > 511 {
            return Err(invalid(span, "map literal entry limit exceeded"));
        }
        let pointer = self.allocate_aggregate(0, &[], span)?;
        self.emit(I::LocalGet(pointer));
        self.root_string(span)?;
        self.emit(I::Drop);
        for (k, v) in entries {
            let kt = self.expr(k, locals, depth)?;
            expect(&kt, key, span)?;
            let ks = self.temp(value_type(key, span)?, span)?;
            self.emit(I::LocalSet(ks));
            let vt = self.expr(v, locals, depth)?;
            expect(&vt, value, span)?;
            let vs = self.temp(value_type(value, span)?, span)?;
            self.emit(I::LocalSet(vs));
            self.map_values(
                F::MapPut,
                &[(pointer, ty.clone()), (ks, kt), (vs, vt)],
                ty,
                span,
            )?;
            self.emit(I::LocalSet(pointer));
        }
        self.emit(I::LocalGet(pointer));
        Ok(ty.clone())
    }

    pub(super) fn map_call(
        &mut self,
        builtin: F,
        args: &[Expr],
        result: &Type,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let mut values = Vec::new();
        for arg in args {
            let ty = self.expr(arg, locals, depth)?;
            let slot = self.temp(value_type(&ty, span)?, span)?;
            self.emit(I::LocalSet(slot));
            values.push((slot, ty));
        }
        self.map_values(builtin, &values, result, span)
    }

    fn map_values(
        &mut self,
        builtin: F,
        values: &[(u32, Type)],
        result: &Type,
        span: Span,
    ) -> Result<Type> {
        if builtin == F::MapNew {
            if !values.is_empty() || !matches!(result, Type::Map(_, _)) {
                return Err(invalid(span, "invalid Map.new signature"));
            }
            let p = self.allocate_aggregate(0, &[], span)?;
            self.emit(I::LocalGet(p));
            return Ok(result.clone());
        }
        let Some((input, Type::Map(key, value))) = values.first() else {
            return Err(invalid(span, "Map helper requires a Map"));
        };
        let input = *input;
        let arity = match builtin {
            F::MapPut => 3,
            F::MapGet | F::MapDelete | F::MapContains => 2,
            _ => 1,
        };
        if values.len() != arity {
            return Err(invalid(span, "Map argument count mismatch"));
        }
        if arity >= 2 {
            expect(&values[1].1, key, span)?;
        }
        if arity == 3 {
            expect(&values[2].1, value, span)?;
        }
        let pair = Type::Tuple(vec![*key.clone(), *value.clone()]);
        let len = self.temp(ValType::I32, span)?;
        self.emit(I::LocalGet(input));
        self.emit(I::I64Load(mem(8)));
        self.emit(I::I32WrapI64);
        self.emit(I::LocalSet(len));
        if matches!(builtin, F::MapLen | F::MapIsEmpty) {
            self.emit(I::LocalGet(len));
            let actual = if builtin == F::MapLen {
                self.emit(I::I64ExtendI32U);
                Type::Int
            } else {
                self.emit(I::I32Eqz);
                Type::Bool
            };
            expect(&actual, result, span)?;
            return Ok(actual);
        }
        let found = self.temp(ValType::I32, span)?;
        self.emit(I::I32Const(-1));
        self.emit(I::LocalSet(found));
        let index = self.temp(ValType::I32, span)?;
        if arity >= 2 {
            self.emit(I::I32Const(0));
            self.emit(I::LocalSet(index));
            self.emit(I::Block(BlockType::Empty));
            self.emit(I::Loop(BlockType::Empty));
            self.emit(I::LocalGet(index));
            self.emit(I::LocalGet(len));
            self.emit(I::I32GeU);
            self.emit(I::BrIf(1));
            self.dynamic_cell(input, index, &pair, span)?;
            let entry = self.temp(ValType::I32, span)?;
            self.emit(I::LocalSet(entry));
            self.load_cell(entry, 0, key, span)?;
            let stored = self.temp(value_type(key, span)?, span)?;
            self.emit(I::LocalSet(stored));
            self.equal_values(stored, values[1].0, key, span)?;
            self.emit(I::If(BlockType::Empty));
            self.emit(I::LocalGet(index));
            self.emit(I::LocalSet(found));
            self.emit(I::Br(2));
            self.emit(I::End);
            self.emit(I::LocalGet(index));
            self.emit(I::I32Const(1));
            self.emit(I::I32Add);
            self.emit(I::LocalSet(index));
            self.emit(I::Br(0));
            self.emit(I::End);
            self.emit(I::End);
        }
        if builtin == F::MapContains {
            self.emit(I::LocalGet(found));
            self.emit(I::I32Const(0));
            self.emit(I::I32GeS);
            expect(result, &Type::Bool, span)?;
            return Ok(Type::Bool);
        }
        if builtin == F::MapGet {
            let actual = Type::Option(value.clone());
            expect(&actual, result, span)?;
            self.emit(I::LocalGet(found));
            self.emit(I::I32Const(0));
            self.emit(I::I32GeS);
            self.emit(I::If(BlockType::Result(ValType::I32)));
            self.dynamic_cell(input, found, &pair, span)?;
            let entry = self.temp(ValType::I32, span)?;
            self.emit(I::LocalSet(entry));
            self.load_cell(entry, 1, value, span)?;
            let payload = self.temp(value_type(value, span)?, span)?;
            self.emit(I::LocalSet(payload));
            let some = self.allocate_aggregate(0, &[(payload, *value.clone())], span)?;
            self.emit(I::LocalGet(some));
            self.emit(I::Else);
            let none = self.allocate_aggregate(1, &[], span)?;
            self.emit(I::LocalGet(none));
            self.emit(I::End);
            return Ok(actual);
        }
        let item = match builtin {
            F::MapKeys => *key.clone(),
            F::MapValues => *value.clone(),
            _ => pair.clone(),
        };
        let actual = if matches!(builtin, F::MapKeys | F::MapValues) {
            Type::List(Box::new(item.clone()))
        } else {
            values[0].1.clone()
        };
        expect(&actual, result, span)?;
        let out_len = self.temp(ValType::I32, span)?;
        self.emit(I::LocalGet(len));
        if matches!(builtin, F::MapPut | F::MapDelete) {
            self.emit(I::LocalGet(found));
            self.emit(I::I32Const(0));
            self.emit(if builtin == F::MapPut {
                I::I32LtS
            } else {
                I::I32GeS
            });
            self.emit(if builtin == F::MapPut {
                I::I32Add
            } else {
                I::I32Sub
            });
        }
        self.emit(I::LocalSet(out_len));
        let output = self.new_list(out_len, span)?;
        let replacement = if builtin == F::MapPut {
            let p = self.allocate_aggregate(
                0,
                &[(values[1].0, *key.clone()), (values[2].0, *value.clone())],
                span,
            )?;
            self.emit(I::LocalGet(p));
            self.root_string(span)?;
            self.emit(I::Drop);
            Some(p)
        } else {
            None
        };
        self.emit(I::I32Const(0));
        self.emit(I::LocalSet(index));
        self.emit(I::Block(BlockType::Empty));
        self.emit(I::Loop(BlockType::Empty));
        self.emit(I::LocalGet(index));
        self.emit(I::LocalGet(out_len));
        self.emit(I::I32GeU);
        self.emit(I::BrIf(1));
        if let Some(replacement) = replacement {
            self.emit(I::LocalGet(index));
            self.emit(I::LocalGet(found));
            self.emit(I::I32Eq);
            self.emit(I::LocalGet(index));
            self.emit(I::LocalGet(len));
            self.emit(I::I32Eq);
            self.emit(I::I32Or);
            self.emit(I::If(BlockType::Result(ValType::I32)));
            self.emit(I::LocalGet(replacement));
            self.emit(I::Else);
            self.dynamic_cell(input, index, &pair, span)?;
            self.emit(I::End);
        } else {
            let offset = self.temp(ValType::I32, span)?;
            self.emit(I::LocalGet(index));
            if builtin == F::MapDelete {
                self.emit(I::LocalGet(index));
                self.emit(I::LocalGet(found));
                self.emit(I::I32GeU);
                self.emit(I::I32Add);
            }
            self.emit(I::LocalSet(offset));
            self.dynamic_cell(input, offset, &pair, span)?;
            if matches!(builtin, F::MapKeys | F::MapValues) {
                let entry = self.temp(ValType::I32, span)?;
                self.emit(I::LocalSet(entry));
                self.load_cell(entry, usize::from(builtin == F::MapValues), &item, span)?;
            }
        }
        let stored = self.temp(value_type(&item, span)?, span)?;
        self.emit(I::LocalSet(stored));
        self.store_dynamic(output, index, stored, &item, span)?;
        self.emit(I::LocalGet(index));
        self.emit(I::I32Const(1));
        self.emit(I::I32Add);
        self.emit(I::LocalSet(index));
        self.emit(I::Br(0));
        self.emit(I::End);
        self.emit(I::End);
        self.emit(I::LocalGet(output));
        Ok(actual)
    }
}
