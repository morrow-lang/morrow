//! Bounded immutable collection primitives; allocation preserves all source roots.
use super::aggregates::mem;
use super::*;
use ir::Builtin as F;
use wasm_encoder::MemArg;

impl Emitter<'_, '_> {
    pub(super) fn slice_list(
        &mut self,
        input: u32,
        start: usize,
        item: &Type,
        span: Span,
    ) -> Result<u32> {
        if start > 511 {
            return Err(invalid(span, "list pattern prefix exceeds limit"));
        }
        let first = self
            .strings
            .ok_or_else(|| invalid(span, "missing managed heap"))?
            .first;
        let len = self.temp(ValType::I32, span)?;
        let output = self.temp(ValType::I32, span)?;
        for i in [
            I::LocalGet(input),
            I::I64Load(mem(8)),
            I::I32WrapI64,
            I::I32Const(start as i32),
            I::I32Sub,
            I::LocalTee(len),
            I::I32Const(8),
            I::I32Mul,
            I::I32Const(8),
            I::I32Add,
            I::Call(first + strings::ALLOC),
            I::LocalTee(output),
            I::LocalGet(len),
            I::I64ExtendI32U,
            I::I64Store(mem(8)),
            I::LocalGet(output),
            I::I32Const(16),
            I::I32Add,
            I::LocalGet(input),
            I::I32Const(16 + start as i32 * 8),
            I::I32Add,
            I::LocalGet(len),
            I::I32Const(8),
            I::I32Mul,
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
        ] {
            self.emit(i);
        }
        if managed(item) {
            let cell = self.temp(ValType::I32, span)?;
            self.emit(I::I32Const(1));
            self.emit(I::LocalSet(cell));
            self.emit(I::Block(BlockType::Empty));
            self.emit(I::Loop(BlockType::Empty));
            self.emit(I::LocalGet(cell));
            self.emit(I::LocalGet(len));
            self.emit(I::I32GtU);
            self.emit(I::BrIf(1));
            self.mark_cell(output, cell, span)?;
            self.emit(I::LocalGet(cell));
            self.emit(I::I32Const(1));
            self.emit(I::I32Add);
            self.emit(I::LocalSet(cell));
            self.emit(I::Br(0));
            self.emit(I::End);
            self.emit(I::End);
        }
        self.emit(I::LocalGet(output));
        self.root_string(span)?;
        self.emit(I::Drop);
        Ok(output)
    }
    pub(super) fn mark_cell(&mut self, pointer: u32, cell: u32, span: Span) -> Result<()> {
        let address = self.temp(ValType::I32, span)?;
        let byte = MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        };
        for i in [
            I::LocalGet(pointer),
            I::I32Const(strings::HEAP_START),
            I::I32Sub,
            I::I32Const(strings::STRIDE),
            I::I32DivU,
            I::I32Const(64),
            I::I32Mul,
            I::I32Const(super::super::heap::META_BASE),
            I::I32Add,
            I::LocalGet(cell),
            I::I32Const(8),
            I::I32DivU,
            I::I32Add,
            I::LocalTee(address),
            I::LocalGet(address),
            I::I32Load8U(byte),
            I::I32Const(1),
            I::LocalGet(cell),
            I::I32Const(8),
            I::I32RemU,
            I::I32Shl,
            I::I32Or,
            I::I32Store8(byte),
        ] {
            self.emit(i);
        }
        Ok(())
    }

    pub(super) fn dynamic_cell(
        &mut self,
        pointer: u32,
        index: u32,
        ty: &Type,
        span: Span,
    ) -> Result<()> {
        self.emit(I::LocalGet(pointer));
        self.emit(I::LocalGet(index));
        self.emit(I::I32Const(8));
        self.emit(I::I32Mul);
        self.emit(I::I32Add);
        self.emit(match value_type(ty, span)? {
            ValType::I64 => I::I64Load(mem(16)),
            ValType::F64 => I::F64Load(mem(16)),
            _ => I::I32Load(MemArg {
                align: 2,
                ..mem(16)
            }),
        });
        Ok(())
    }

    pub(super) fn collection_call(
        &mut self,
        builtin: F,
        args: &[Expr],
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let arity = if matches!(
            builtin,
            F::ListGet | F::ListPush | F::ListConcat | F::OptionUnwrapOr | F::ResultUnwrapOr
        ) {
            2
        } else {
            1
        };
        if args.len() != arity {
            return Err(invalid(span, "collection argument count mismatch"));
        }
        let mut values = Vec::new();
        for arg in args {
            let ty = self.expr(arg, locals, depth)?;
            let slot = self.temp(value_type(&ty, span)?, span)?;
            self.emit(I::LocalSet(slot));
            values.push((slot, ty));
        }
        let (input, ty) = &values[0];
        let input = *input;
        if matches!(
            builtin,
            F::OptionIsSome
                | F::OptionIsNone
                | F::OptionUnwrapOr
                | F::ResultIsOk
                | F::ResultIsErr
                | F::ResultUnwrapOr
        ) {
            let (success, item) = match ty {
                Type::Option(item) => (0, item.as_ref()),
                Type::Result(item, _) => (0, item.as_ref()),
                _ => {
                    return Err(invalid(
                        span,
                        "option/result helper requires matching aggregate",
                    ));
                }
            };
            self.emit(I::LocalGet(input));
            self.emit(I::I64Load(mem(8)));
            self.emit(I::I64Const(success));
            self.emit(I::I64Eq);
            if matches!(builtin, F::OptionUnwrapOr | F::ResultUnwrapOr) {
                expect(&values[1].1, item, span)?;
                self.emit(I::If(BlockType::Result(value_type(item, span)?)));
                self.load_cell(input, 0, item, span)?;
                self.emit(I::Else);
                self.emit(I::LocalGet(values[1].0));
                self.emit(I::End);
                return Ok(item.clone());
            }
            if matches!(builtin, F::OptionIsNone | F::ResultIsErr) {
                self.emit(I::I32Eqz);
            }
            return Ok(Type::Bool);
        }
        let Type::List(item) = ty else {
            return Err(invalid(span, "list helper requires a List"));
        };
        let len = self.temp(ValType::I32, span)?;
        self.emit(I::LocalGet(input));
        self.emit(I::I64Load(mem(8)));
        self.emit(I::I32WrapI64);
        self.emit(I::LocalSet(len));
        match builtin {
            F::ListLen => {
                self.emit(I::LocalGet(len));
                self.emit(I::I64ExtendI32U);
                return Ok(Type::Int);
            }
            F::ListIsEmpty => {
                self.emit(I::LocalGet(len));
                self.emit(I::I32Eqz);
                return Ok(Type::Bool);
            }
            F::ListGet | F::ListHead => {
                let index = self.temp(ValType::I32, span)?;
                if builtin == F::ListGet {
                    expect(&values[1].1, &Type::Int, span)?;
                    self.emit(I::LocalGet(values[1].0));
                    self.emit(I::LocalGet(len));
                    self.emit(I::I64ExtendI32U);
                    self.emit(I::I64GeU);
                    self.emit(I::If(BlockType::Empty));
                    self.emit(I::Unreachable);
                    self.emit(I::End);
                    self.emit(I::LocalGet(values[1].0));
                    self.emit(I::I32WrapI64);
                } else {
                    self.emit(I::LocalGet(len));
                    self.emit(I::I32Eqz);
                    self.emit(I::If(BlockType::Empty));
                    self.emit(I::Unreachable);
                    self.emit(I::End);
                    self.emit(I::I32Const(0));
                }
                self.emit(I::LocalSet(index));
                self.dynamic_cell(input, index, item, span)?;
                return Ok(*item.clone());
            }
            _ => {}
        }
        let out_len = self.temp(ValType::I32, span)?;
        self.emit(I::LocalGet(len));
        match builtin {
            F::ListPush => {
                expect(&values[1].1, item, span)?;
                self.emit(I::I32Const(1));
                self.emit(I::I32Add);
            }
            F::ListConcat => {
                expect(&values[1].1, ty, span)?;
                self.emit(I::LocalGet(values[1].0));
                self.emit(I::I64Load(mem(8)));
                self.emit(I::I32WrapI64);
                self.emit(I::I32Add);
            }
            F::ListTail => {
                self.emit(I::LocalGet(len));
                self.emit(I::I32Const(0));
                self.emit(I::I32GtU);
                self.emit(I::I32Sub);
            }
            F::ListReverse => {}
            _ => return Err(invalid(span, "unsupported list helper")),
        }
        self.emit(I::LocalTee(out_len));
        self.emit(I::I32Const(511));
        self.emit(I::I32GtU);
        self.emit(I::If(BlockType::Empty));
        self.emit(I::Unreachable);
        self.emit(I::End);
        let first = self
            .strings
            .ok_or_else(|| invalid(span, "missing managed heap"))?
            .first;
        let result = self.temp(ValType::I32, span)?;
        self.emit(I::LocalGet(out_len));
        self.emit(I::I32Const(8));
        self.emit(I::I32Mul);
        self.emit(I::I32Const(8));
        self.emit(I::I32Add);
        self.emit(I::Call(first + strings::ALLOC));
        self.emit(I::LocalTee(result));
        self.emit(I::LocalGet(out_len));
        self.emit(I::I64ExtendI32U);
        self.emit(I::I64Store(mem(8)));
        let index = self.temp(ValType::I32, span)?;
        self.emit(I::I32Const(0));
        self.emit(I::LocalSet(index));
        self.emit(I::Block(BlockType::Empty));
        self.emit(I::Loop(BlockType::Empty));
        self.emit(I::LocalGet(index));
        self.emit(I::LocalGet(out_len));
        self.emit(I::I32GeU);
        self.emit(I::BrIf(1));
        self.emit(I::LocalGet(result));
        self.emit(I::LocalGet(index));
        self.emit(I::I32Const(8));
        self.emit(I::I32Mul);
        self.emit(I::I32Add);
        if builtin == F::ListPush || builtin == F::ListConcat {
            self.emit(I::LocalGet(index));
            self.emit(I::LocalGet(len));
            self.emit(I::I32LtU);
            self.emit(I::If(BlockType::Result(value_type(item, span)?)));
            self.dynamic_cell(input, index, item, span)?;
            self.emit(I::Else);
            if builtin == F::ListPush {
                self.emit(I::LocalGet(values[1].0));
            } else {
                let offset = self.temp(ValType::I32, span)?;
                self.emit(I::LocalGet(index));
                self.emit(I::LocalGet(len));
                self.emit(I::I32Sub);
                self.emit(I::LocalSet(offset));
                self.dynamic_cell(values[1].0, offset, item, span)?;
            }
            self.emit(I::End);
        } else {
            let offset = self.temp(ValType::I32, span)?;
            self.emit(I::LocalGet(index));
            if builtin == F::ListTail {
                self.emit(I::I32Const(1));
                self.emit(I::I32Add);
            } else {
                self.emit(I::I32Const(-1));
                self.emit(I::I32Mul);
                self.emit(I::LocalGet(len));
                self.emit(I::I32Add);
                self.emit(I::I32Const(1));
                self.emit(I::I32Sub);
            }
            self.emit(I::LocalSet(offset));
            self.dynamic_cell(input, offset, item, span)?;
        }
        self.emit(match value_type(item, span)? {
            ValType::I64 => I::I64Store(mem(16)),
            ValType::F64 => I::F64Store(mem(16)),
            _ => I::I32Store(MemArg {
                align: 2,
                ..mem(16)
            }),
        });
        self.emit(I::LocalGet(index));
        self.emit(I::I32Const(1));
        self.emit(I::I32Add);
        self.emit(I::LocalSet(index));
        if managed(item) {
            self.mark_cell(result, index, span)?;
        }
        self.emit(I::Br(0));
        self.emit(I::End);
        self.emit(I::End);
        self.emit(I::LocalGet(result));
        Ok(ty.clone())
    }
}
