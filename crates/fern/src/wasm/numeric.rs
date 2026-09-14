//! Wrapping signed64 operations and IEEE binary64 without native helper imports.
use super::{Result, emit::Emitter, invalid};
use crate::{Span, Type, ast::BinaryOp};
use wasm_encoder::{BlockType, Instruction as I, ValType};

impl Emitter<'_, '_> {
    pub(super) fn numeric(&mut self, op: BinaryOp, operand: &Type, span: Span) -> Result<Type> {
        use BinaryOp::*;
        let instruction = match (operand, op) {
            (Type::Int, Add) => I::I64Add,
            (Type::Int, Subtract) => I::I64Sub,
            (Type::Int, Multiply) => I::I64Mul,
            (Type::Int, Divide) => {
                self.divide(span)?;
                return Ok(Type::Int);
            }
            (Type::Int, Remainder) => {
                self.nonzero_divisor(span)?;
                I::I64RemS
            }
            (Type::Int, Power) => {
                self.power(span)?;
                return Ok(Type::Int);
            }
            (Type::Int, BitAnd) => I::I64And,
            (Type::Int, BitOr) => I::I64Or,
            (Type::Int, BitXor) => I::I64Xor,
            (Type::Int, ShiftLeft) => I::I64Shl,
            (Type::Int, ShiftRight) => I::I64ShrS,
            (Type::Int, Eq) => I::I64Eq,
            (Type::Int, Ne) => I::I64Ne,
            (Type::Int, Lt) => I::I64LtS,
            (Type::Int, Le) => I::I64LeS,
            (Type::Int, Gt) => I::I64GtS,
            (Type::Int, Ge) => I::I64GeS,
            (Type::Float, Add) => I::F64Add,
            (Type::Float, Subtract) => I::F64Sub,
            (Type::Float, Multiply) => I::F64Mul,
            (Type::Float, Divide) => I::F64Div,
            (Type::Float, Eq) => I::F64Eq,
            (Type::Float, Ne) => I::F64Ne,
            (Type::Float, Lt) => I::F64Lt,
            (Type::Float, Le) => I::F64Le,
            (Type::Float, Gt) => I::F64Gt,
            (Type::Float, Ge) => I::F64Ge,
            (Type::Bool, Eq) => I::I32Eq,
            (Type::Bool, Ne) => I::I32Ne,
            (Type::Float, Power) => {
                return Err(invalid(
                    span,
                    "floating power requires a browser math host capability",
                ));
            }
            _ => return Err(invalid(span, "unsupported binary operator or operand type")),
        };
        self.emit(instruction);
        Ok(if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
            Type::Bool
        } else {
            operand.clone()
        })
    }

    fn nonzero_divisor(&mut self, span: Span) -> Result<()> {
        let rhs = self.temp(ValType::I64, span)?;
        self.emit(I::LocalTee(rhs));
        self.emit(I::I64Eqz);
        self.emit(I::If(BlockType::Empty));
        self.emit(I::Unreachable);
        self.emit(I::End);
        self.emit(I::LocalGet(rhs));
        Ok(())
    }

    fn divide(&mut self, span: Span) -> Result<()> {
        let rhs = self.temp(ValType::I64, span)?;
        let lhs = self.temp(ValType::I64, span)?;
        self.emit(I::LocalSet(rhs));
        self.emit(I::LocalSet(lhs));
        self.emit(I::LocalGet(rhs));
        self.emit(I::I64Eqz);
        self.emit(I::If(BlockType::Empty));
        self.emit(I::Unreachable);
        self.emit(I::End);
        // Fern wraps MIN / -1; core Wasm's signed divide traps for this pair.
        self.emit(I::LocalGet(lhs));
        self.emit(I::I64Const(i64::MIN));
        self.emit(I::I64Eq);
        self.emit(I::LocalGet(rhs));
        self.emit(I::I64Const(-1));
        self.emit(I::I64Eq);
        self.emit(I::I32And);
        self.emit(I::If(BlockType::Result(ValType::I64)));
        self.emit(I::I64Const(i64::MIN));
        self.emit(I::Else);
        self.emit(I::LocalGet(lhs));
        self.emit(I::LocalGet(rhs));
        self.emit(I::I64DivS);
        self.emit(I::End);
        Ok(())
    }

    fn power(&mut self, span: Span) -> Result<()> {
        let exponent = self.temp(ValType::I64, span)?;
        let base = self.temp(ValType::I64, span)?;
        let result = self.temp(ValType::I64, span)?;
        self.emit(I::LocalSet(exponent));
        self.emit(I::LocalSet(base));
        self.emit(I::LocalGet(exponent));
        self.emit(I::I64Const(0));
        self.emit(I::I64LtS);
        self.emit(I::If(BlockType::Empty));
        self.emit(I::Unreachable);
        self.emit(I::End);
        self.emit(I::I64Const(1));
        self.emit(I::LocalSet(result));
        self.emit(I::Block(BlockType::Empty));
        self.emit(I::Loop(BlockType::Empty));
        self.emit(I::LocalGet(exponent));
        self.emit(I::I64Eqz);
        self.emit(I::BrIf(1));
        self.emit(I::LocalGet(exponent));
        self.emit(I::I64Const(1));
        self.emit(I::I64And);
        self.emit(I::I64Eqz);
        self.emit(I::If(BlockType::Empty));
        self.emit(I::Else);
        self.emit(I::LocalGet(result));
        self.emit(I::LocalGet(base));
        self.emit(I::I64Mul);
        self.emit(I::LocalSet(result));
        self.emit(I::End);
        self.emit(I::LocalGet(base));
        self.emit(I::LocalGet(base));
        self.emit(I::I64Mul);
        self.emit(I::LocalSet(base));
        self.emit(I::LocalGet(exponent));
        self.emit(I::I64Const(1));
        self.emit(I::I64ShrU);
        self.emit(I::LocalSet(exponent));
        self.emit(I::Br(0));
        self.emit(I::End);
        self.emit(I::End);
        self.emit(I::LocalGet(result));
        Ok(())
    }
}
