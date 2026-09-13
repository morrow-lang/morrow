//! Fixed-slot allocation and portable string primitives. Strings are leaf values;
//! aggregate child bits and marking live in `heap`. Compiler shadow roots and
//! typed host handles preserve references without scanning Int or Float payloads.
//! String-only modules use 256 slots; aggregate modules use 8192. Each slot holds
//! at most 4096 UTF-8 bytes or 511 aggregate fields, with 16384 compiler shadow roots.
//! Host entry wrappers clear abandoned transient roots after a trapped call.
use super::{Result, invalid, managed};
use crate::{Span, Type, ir};
use std::collections::BTreeMap;
use wasm_encoder::{
    BlockType as B, CodeSection, ConstExpr, DataSection, Function, FunctionSection, GlobalSection,
    GlobalType, Instruction as I, MemArg, MemorySection, MemoryType, TypeSection, ValType as V,
};

pub(super) const PUSH: u32 = 0;
const COLLECT: u32 = 1;
const FIND: u32 = 2;
pub(super) const ALLOC: u32 = 3;
pub(super) const CONCAT: u32 = 4;
pub(super) const LEN: u32 = 5;
pub(super) const EQ: u32 = 6;
pub(super) const INT_TEXT: u32 = 7;
pub(super) const COUNT: u32 = 8;
const ROOT_BYTES: i32 = 65_536;
const LITERAL_BYTES: i32 = 1_048_576;
const STRING_BYTES: i32 = 4096;
pub(super) const STRIDE: i32 = STRING_BYTES + 8;
pub(super) const HEAP_START: i32 = ROOT_BYTES + LITERAL_BYTES;

pub(super) struct Runtime {
    pub first: u32,
    pub literals: BTreeMap<String, i32>,
    data: Vec<u8>,
    pub layouts: BTreeMap<Type, ir::TypeLayout>,
    pub slots: i32,
    pub type_ids: BTreeMap<Type, u32>,
}

impl Runtime {
    pub fn prepare(program: &ir::Program) -> Result<Option<Self>> {
        let mut runtime = Self {
            first: program.functions.len() as u32,
            literals: BTreeMap::new(),
            data: Vec::new(),
            layouts: program
                .types
                .iter()
                .map(|layout| (layout.ty.clone(), layout.clone()))
                .collect(),
            slots: 256,
            type_ids: BTreeMap::from([(Type::String, 1)]),
        };
        let mut needed = false;
        for function in &program.functions {
            for ty in std::iter::once(&function.return_type)
                .chain(function.params.iter().map(|param| &param.ty))
            {
                if managed(ty) {
                    let next = runtime.type_ids.len() as u32 + 1;
                    runtime.type_ids.entry(ty.clone()).or_insert(next);
                }
            }
            needed |=
                managed(&function.return_type) || function.params.iter().any(|p| managed(&p.ty));
            let mut pending = vec![&function.body];
            while let Some(expr) = pending.pop() {
                needed |= managed(&expr.ty);
                if managed(&expr.ty) && expr.ty != Type::String {
                    runtime.slots = 8192;
                }
                if let ir::ExprKind::String(value) = &expr.kind {
                    runtime.literal(value, expr.span)?;
                }
                if matches!(&expr.kind, ir::ExprKind::Interpolate(_)) {
                    for text in ["", "true", "false", "()"] {
                        runtime.literal(text, expr.span)?;
                    }
                }
                let patterns = match &expr.kind {
                    ir::ExprKind::Match { arms, .. } => {
                        arms.iter().map(|arm| &arm.pattern).collect::<Vec<_>>()
                    }
                    ir::ExprKind::Block(statements) => statements
                        .iter()
                        .filter_map(|statement| {
                            if let ir::Stmt::LetElse { pattern, .. } = statement {
                                Some(pattern)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => vec![],
                };
                for pattern in patterns {
                    runtime.pattern_literals(pattern, expr.span)?;
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
        if self.data.len() + value.len() + 16 > (super::heap::META_BASE - ROOT_BYTES) as usize {
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

    fn pattern_literals(&mut self, pattern: &ir::Pattern, span: Span) -> Result<()> {
        let mut pending = vec![pattern];
        let mut work = 0;
        while let Some(pattern) = pending.pop() {
            work += 1;
            if work > 4096 {
                return Err(invalid(span, "pattern complexity limit exceeded"));
            }
            match pattern {
                ir::Pattern::String(text) => self.literal(text, span)?,
                ir::Pattern::Newtype(pattern) => pending.push(pattern),
                ir::Pattern::Tuple(patterns)
                | ir::Pattern::Variant {
                    fields: patterns, ..
                } => pending.extend(patterns),
                ir::Pattern::List { prefix, rest } => {
                    pending.extend(prefix);
                    pending.extend(rest.as_deref());
                }
                ir::Pattern::TupleRest { prefix, rest } => {
                    pending.extend(prefix);
                    pending.push(rest);
                }
                _ => {}
            }
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
            (vec![V::I64], V::I32),
        ] {
            functions.function(types.len());
            types.ty().function(params, [result]);
        }
    }

    pub fn memory(&self) -> MemorySection {
        let pages = ((HEAP_START + self.slots * STRIDE) as u64).div_ceil(65_536);
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
        section.global(
            GlobalType {
                val_type: V::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(HEAP_START),
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
        code.function(&super::heap::collect(self.slots));
        code.function(&find(HEAP_START + self.slots * STRIDE));
        code.function(&allocate(self.first));
        code.function(&concat(self.first));
        code.function(&body(
            0,
            [I::LocalGet(0), I::I32Load(mem(0)), I::I64ExtendI32U],
        ));
        code.function(&equal());
        code.function(&integer_text(self.first));
    }
}

fn integer_text(first: u32) -> Function {
    let mut function = Function::new([(1, V::I64), (3, V::I32)]);
    for instruction in [
        I::LocalGet(0),
        I::I64Const(0),
        I::I64LtS,
        I::LocalSet(4),
        I::LocalGet(4),
        I::If(B::Result(V::I64)),
        I::I64Const(0),
        I::LocalGet(0),
        I::I64Sub,
        I::Else,
        I::LocalGet(0),
        I::End,
        I::LocalSet(1),
        I::I32Const(20),
        I::Call(first + ALLOC),
        I::LocalSet(3),
        I::I32Const(20),
        I::LocalSet(2),
        I::Loop(B::Empty),
        I::LocalGet(2),
        I::I32Const(1),
        I::I32Sub,
        I::LocalSet(2),
        I::LocalGet(3),
        I::LocalGet(2),
        I::I32Add,
        I::LocalGet(1),
        I::I64Const(10),
        I::I64RemU,
        I::I32WrapI64,
        I::I32Const(48),
        I::I32Add,
        I::I32Store8(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }),
        I::LocalGet(1),
        I::I64Const(10),
        I::I64DivU,
        I::LocalTee(1),
        I::I64Const(0),
        I::I64Ne,
        I::BrIf(0),
        I::End,
        I::LocalGet(4),
        I::If(B::Empty),
        I::LocalGet(2),
        I::I32Const(1),
        I::I32Sub,
        I::LocalSet(2),
        I::LocalGet(3),
        I::LocalGet(2),
        I::I32Add,
        I::I32Const(45),
        I::I32Store8(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }),
        I::End,
        I::LocalGet(3),
        I::I32Const(8),
        I::I32Add,
        I::LocalGet(3),
        I::I32Const(8),
        I::I32Add,
        I::LocalGet(2),
        I::I32Add,
        I::I32Const(20),
        I::LocalGet(2),
        I::I32Sub,
        I::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        },
        I::LocalGet(3),
        I::I32Const(20),
        I::LocalGet(2),
        I::I32Sub,
        I::I32Store(mem(0)),
        I::LocalGet(3),
        I::End,
    ] {
        function.instruction(&instruction);
    }
    function
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

fn find(heap_end: i32) -> Function {
    body(
        2,
        [
            I::GlobalGet(1),
            I::LocalSet(0),
            I::Block(B::Empty),
            I::Loop(B::Empty),
            I::LocalGet(1),
            I::I32Const((heap_end - HEAP_START) / STRIDE),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(0),
            I::I32Const(heap_end),
            I::I32GeU,
            I::If(B::Empty),
            I::I32Const(HEAP_START),
            I::LocalSet(0),
            I::End,
            I::LocalGet(0),
            I::I32Load(mem(4)),
            I::I32Eqz,
            I::If(B::Empty),
            I::LocalGet(0),
            I::I32Const(STRIDE),
            I::I32Add,
            I::GlobalSet(1),
            I::LocalGet(0),
            I::Return,
            I::End,
            I::LocalGet(0),
            I::I32Const(STRIDE),
            I::I32Add,
            I::LocalSet(0),
            I::LocalGet(1),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(1),
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
            I::I32Const(HEAP_START),
            I::I32Sub,
            I::I32Const(STRIDE),
            I::I32DivU,
            I::I32Const(64),
            I::I32Mul,
            I::I32Const(super::heap::META_BASE),
            I::I32Add,
            I::I32Const(0),
            I::I32Const(64),
            I::MemoryFill(0),
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
