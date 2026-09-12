//! Optional bounded string heap. Every root is an explicit compiler-owned i32
//! pointer slot; integers and native stack contents are never scanned.
//!
//! Strings are leaf objects, so marking needs no graph recursion. Fixed slots
//! make sweep/reuse bounded and nonmoving. This deliberately modest first heap
//! supports 256 live strings of at most 4096 bytes, with 16384 shadow roots.
//! Managed signatures are internal: only scalar export wrappers enter the
//! module, clearing abandoned roots after a previous trapped invocation.
use super::{Result, invalid};
use crate::{Span, Type, ir};
use std::collections::BTreeMap;
use wasm_encoder::{
    BlockType as B, CodeSection, ConstExpr, DataSection, Function, FunctionSection, GlobalSection,
    GlobalType, Instruction as I, MemArg, MemorySection, MemoryType, TypeSection, ValType as V,
};

pub(super) const PUSH: u32 = 0;
const COLLECT: u32 = 1;
const FIND: u32 = 2;
const ALLOC: u32 = 3;
pub(super) const CONCAT: u32 = 4;
pub(super) const LEN: u32 = 5;
pub(super) const EQ: u32 = 6;
const ROOT_BYTES: i32 = 65_536;
const LITERAL_BYTES: i32 = 1_048_576;
const STRING_BYTES: i32 = 4096;
const STRIDE: i32 = STRING_BYTES + 8;
const HEAP_START: i32 = ROOT_BYTES + LITERAL_BYTES;
const HEAP_END: i32 = HEAP_START + 256 * STRIDE;

pub(super) struct Runtime {
    pub first: u32,
    pub literals: BTreeMap<String, i32>,
    data: Vec<u8>,
}

impl Runtime {
    pub fn prepare(program: &ir::Program) -> Result<Option<Self>> {
        let mut runtime = Self {
            first: program.functions.len() as u32,
            literals: BTreeMap::new(),
            data: Vec::new(),
        };
        let mut needed = false;
        for function in &program.functions {
            needed |= function.return_type == Type::String
                || function.params.iter().any(|p| p.ty == Type::String);
            let mut pending = vec![&function.body];
            while let Some(expr) = pending.pop() {
                needed |= expr.ty == Type::String;
                if let ir::ExprKind::String(value) = &expr.kind {
                    runtime.literal(value, expr.span)?;
                }
                pending.extend(ir::children(expr));
            }
        }
        Ok(needed.then_some(runtime))
    }

    fn literal(&mut self, value: &str, span: Span) -> Result<()> {
        if value.len() > STRING_BYTES as usize {
            return Err(invalid(span, "string byte limit exceeded (4096 bytes)"));
        }
        if value.contains('\0') {
            return Err(invalid(
                span,
                "embedded NUL strings are not supported by the browser preview",
            ));
        }
        if self.literals.contains_key(value) {
            return Ok(());
        }
        if self.data.len() + value.len() + 16 > LITERAL_BYTES as usize {
            return Err(invalid(span, "string literal storage limit exceeded"));
        }
        self.literals
            .insert(value.to_owned(), ROOT_BYTES + self.data.len() as i32);
        self.data
            .extend_from_slice(&(value.len() as u32).to_le_bytes());
        self.data.extend_from_slice(&1u32.to_le_bytes());
        self.data.extend_from_slice(value.as_bytes());
        while !self.data.len().is_multiple_of(8) {
            self.data.push(0);
        }
        Ok(())
    }

    pub fn declarations(&self, types: &mut TypeSection, functions: &mut FunctionSection) {
        for (params, result) in [
            (vec![V::I32], V::I32),
            (vec![], V::I32),
            (vec![], V::I32),
            (vec![V::I32], V::I32),
            (vec![V::I32, V::I32], V::I32),
            (vec![V::I32], V::I64),
            (vec![V::I32, V::I32], V::I32),
        ] {
            functions.function(types.len());
            types.ty().function(params, [result]);
        }
    }

    pub fn memory(&self) -> MemorySection {
        let pages = (HEAP_END as u64).div_ceil(65_536);
        let mut section = MemorySection::new();
        section.memory(MemoryType {
            minimum: pages,
            maximum: Some(pages),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        section
    }

    pub fn globals(&self) -> GlobalSection {
        let mut section = GlobalSection::new();
        section.global(
            GlobalType {
                val_type: V::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
        section
    }

    pub fn data(&self) -> DataSection {
        let mut section = DataSection::new();
        section.active(
            0,
            &ConstExpr::i32_const(ROOT_BYTES),
            self.data.iter().copied(),
        );
        section
    }

    pub fn code(&self, code: &mut CodeSection) {
        code.function(&push());
        code.function(&collect());
        code.function(&find());
        code.function(&allocate(self.first));
        code.function(&concat(self.first));
        code.function(&body(
            0,
            [I::LocalGet(0), I::I32Load(mem(0)), I::I64ExtendI32U],
        ));
        code.function(&equal());
    }
}

fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}
fn byte() -> MemArg {
    MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }
}

fn body(locals: u32, instructions: impl IntoIterator<Item = I<'static>>) -> Function {
    let mut function = Function::new([(locals, V::I32)]);
    for instruction in instructions {
        function.instruction(&instruction);
    }
    function.instruction(&I::End);
    function
}

fn push() -> Function {
    body(
        0,
        [
            I::GlobalGet(0),
            I::I32Const(ROOT_BYTES),
            I::I32GeU,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::GlobalGet(0),
            I::LocalGet(0),
            I::I32Store(mem(0)),
            I::GlobalGet(0),
            I::I32Const(4),
            I::I32Add,
            I::GlobalSet(0),
            I::LocalGet(0),
        ],
    )
}

fn collect() -> Function {
    // locals: root byte offset, rooted pointer, slot pointer, reclaimed count.
    body(
        4,
        [
            I::Block(B::Empty),
            I::Loop(B::Empty),
            I::LocalGet(0),
            I::GlobalGet(0),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(0),
            I::I32Load(mem(0)),
            I::LocalTee(1),
            I::I32Const(HEAP_START),
            I::I32GeU,
            I::LocalGet(1),
            I::I32Const(HEAP_END),
            I::I32LtU,
            I::I32And,
            I::If(B::Empty),
            I::LocalGet(1),
            I::I32Const(HEAP_START),
            I::I32Sub,
            I::I32Const(STRIDE),
            I::I32RemU,
            I::I32Eqz,
            I::If(B::Empty),
            I::LocalGet(1),
            I::I32Const(2),
            I::I32Store(mem(4)),
            I::End,
            I::End,
            I::LocalGet(0),
            I::I32Const(4),
            I::I32Add,
            I::LocalSet(0),
            I::Br(0),
            I::End,
            I::End,
            I::I32Const(HEAP_START),
            I::LocalSet(2),
            I::Block(B::Empty),
            I::Loop(B::Empty),
            I::LocalGet(2),
            I::I32Const(HEAP_END),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(2),
            I::I32Load(mem(4)),
            I::I32Const(1),
            I::I32Eq,
            I::If(B::Empty),
            I::LocalGet(2),
            I::I32Const(0),
            I::I32Store(mem(4)),
            I::LocalGet(3),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(3),
            I::Else,
            I::LocalGet(2),
            I::I32Load(mem(4)),
            I::I32Const(2),
            I::I32Eq,
            I::If(B::Empty),
            I::LocalGet(2),
            I::I32Const(1),
            I::I32Store(mem(4)),
            I::End,
            I::End,
            I::LocalGet(2),
            I::I32Const(STRIDE),
            I::I32Add,
            I::LocalSet(2),
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(3),
        ],
    )
}

fn find() -> Function {
    body(
        1,
        [
            I::I32Const(HEAP_START),
            I::LocalSet(0),
            I::Block(B::Empty),
            I::Loop(B::Empty),
            I::LocalGet(0),
            I::I32Const(HEAP_END),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(0),
            I::I32Load(mem(4)),
            I::I32Eqz,
            I::If(B::Empty),
            I::LocalGet(0),
            I::Return,
            I::End,
            I::LocalGet(0),
            I::I32Const(STRIDE),
            I::I32Add,
            I::LocalSet(0),
            I::Br(0),
            I::End,
            I::End,
            I::I32Const(0),
        ],
    )
}

fn allocate(first: u32) -> Function {
    body(
        1,
        [
            I::LocalGet(0),
            I::I32Const(STRING_BYTES),
            I::I32GtU,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::Call(first + FIND),
            I::LocalTee(1),
            I::I32Eqz,
            I::If(B::Empty),
            I::Call(first + COLLECT),
            I::Drop,
            I::Call(first + FIND),
            I::LocalTee(1),
            I::I32Eqz,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::End,
            I::LocalGet(1),
            I::LocalGet(0),
            I::I32Store(mem(0)),
            I::LocalGet(1),
            I::I32Const(1),
            I::I32Store(mem(4)),
            I::LocalGet(1),
        ],
    )
}

fn concat(first: u32) -> Function {
    // parameters a,b; locals a.len,b.len,new pointer.
    body(
        3,
        [
            I::LocalGet(0),
            I::I32Load(mem(0)),
            I::LocalTee(2),
            I::LocalGet(1),
            I::I32Load(mem(0)),
            I::LocalTee(3),
            I::I32Add,
            I::Call(first + ALLOC),
            I::LocalSet(4),
            I::LocalGet(4),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(0),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(2),
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            I::LocalGet(4),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(2),
            I::I32Add,
            I::LocalGet(1),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(3),
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            I::LocalGet(4),
        ],
    )
}

fn equal() -> Function {
    // parameters a,b; locals length,index.
    body(
        2,
        [
            I::LocalGet(0),
            I::I32Load(mem(0)),
            I::LocalTee(2),
            I::LocalGet(1),
            I::I32Load(mem(0)),
            I::I32Ne,
            I::If(B::Empty),
            I::I32Const(0),
            I::Return,
            I::End,
            I::Block(B::Empty),
            I::Loop(B::Empty),
            I::LocalGet(3),
            I::LocalGet(2),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(0),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(3),
            I::I32Add,
            I::I32Load8U(byte()),
            I::LocalGet(1),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(3),
            I::I32Add,
            I::I32Load8U(byte()),
            I::I32Ne,
            I::If(B::Empty),
            I::I32Const(0),
            I::Return,
            I::End,
            I::LocalGet(3),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(3),
            I::Br(0),
            I::End,
            I::End,
            I::I32Const(1),
        ],
    )
}
