//! Versioned host roots and a bounded UTF-8 scratch buffer. Handles carry an
//! index and nonwrapping generation, and cannot be substituted for another type.
//! Handles use a positive i64: 55 generation bits and 8 slot bits. Only the scratch
//! interval is a writable host ABI; other linear memory belongs to the module.
use super::strings::{self, Runtime};
use wasm_encoder::{
    BlockType as B, CodeSection, ExportKind, ExportSection, Function, FunctionSection,
    Instruction as I, MemArg, TypeSection, ValType as V,
};
pub(super) const TABLE: i32 = 1_065_000;
const BUFFER: i32 = 1_070_000;
const CAPACITY: i32 = 4096;
pub(super) const NEW: u32 = 0;
pub(super) const GET: u32 = 1;
const RELEASE: u32 = 2;
const UTF8: u32 = 3;
const STRING_NEW: u32 = 4;
const STRING_READ: u32 = 5;
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
fn body(locals: u32, code: impl IntoIterator<Item = I<'static>>) -> Function {
    let mut f = Function::new([(locals, V::I32)]);
    f.instruction(&I::I32Const(0));
    f.instruction(&I::GlobalSet(2));
    for i in code {
        let call = matches!(i, I::Call(_));
        f.instruction(&i);
        if call {
            for instruction in [I::GlobalGet(2), I::If(B::Empty), I::Unreachable, I::End] {
                f.instruction(&instruction);
            }
        }
    }
    f.instruction(&I::End);
    f
}

pub(super) fn declarations(
    runtime: &Runtime,
    types: &mut TypeSection,
    functions: &mut FunctionSection,
    exports: &mut ExportSection,
) {
    for (index, (params, result)) in [
        (vec![V::I32, V::I32], V::I64),
        (vec![V::I64, V::I32], V::I32),
        (vec![V::I64], V::I32),
        (vec![V::I32], V::I32),
        (vec![V::I32], V::I64),
        (vec![V::I64], V::I32),
        (vec![], V::I32),
        (vec![], V::I32),
        (vec![], V::I32),
    ]
    .into_iter()
    .enumerate()
    {
        functions.function(types.len());
        types.ty().function(params, [result]);
        let name = match index as u32 {
            RELEASE => Some("morrow_release"),
            STRING_NEW => Some("morrow_string_new"),
            STRING_READ => Some("morrow_string_read"),
            6 => Some("morrow_abi_version"),
            7 => Some("morrow_io_buffer"),
            8 => Some("morrow_io_capacity"),
            _ => None,
        };
        if let Some(name) = name {
            exports.export(
                name,
                ExportKind::Func,
                runtime.first + strings::COUNT + index as u32,
            );
        }
    }
    exports.export("morrow_memory", ExportKind::Memory, 0);
}

pub(super) fn code(runtime: &Runtime, section: &mut CodeSection) {
    let first = runtime.first + strings::COUNT;
    section.function(&new_handle());
    section.function(&get());
    section.function(&body(
        0,
        [
            I::LocalGet(0),
            I::I32Const(0),
            I::Call(first + GET),
            I::Drop,
            I::LocalGet(0),
            I::I32WrapI64,
            I::I32Const(255),
            I::I32And,
            I::I32Const(16),
            I::I32Mul,
            I::I32Const(TABLE),
            I::I32Add,
            I::I32Const(0),
            I::I32Store(mem(0)),
            I::I32Const(0),
        ],
    ));
    section.function(&utf8());
    section.function(&body(
        1,
        [
            I::I32Const(0),
            I::GlobalSet(0),
            I::LocalGet(0),
            I::Call(first + UTF8),
            I::Drop,
            I::LocalGet(0),
            I::Call(runtime.first + strings::ALLOC),
            I::LocalTee(1),
            I::I32Const(8),
            I::I32Add,
            I::I32Const(BUFFER),
            I::LocalGet(0),
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            I::LocalGet(1),
            I::I32Const(1),
            I::Call(first + NEW),
        ],
    ));
    section.function(&body(
        2,
        [
            I::LocalGet(0),
            I::I32Const(1),
            I::Call(first + GET),
            I::LocalTee(1),
            I::I32Load(mem(0)),
            I::LocalTee(2),
            I::I32Const(CAPACITY),
            I::I32GtU,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::I32Const(BUFFER),
            I::LocalGet(1),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(2),
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            I::LocalGet(2),
        ],
    ));
    for value in [1, BUFFER, CAPACITY] {
        section.function(&body(0, [I::I32Const(value)]));
    }
}

fn new_handle() -> Function {
    let mut function = Function::new([(2, V::I32), (1, V::I64)]);
    for instruction in [
        I::Block(B::Empty),
        I::Loop(B::Empty),
        I::LocalGet(2),
        I::I32Const(256),
        I::I32GeU,
        I::BrIf(1),
        I::LocalGet(2),
        I::I32Const(16),
        I::I32Mul,
        I::I32Const(TABLE),
        I::I32Add,
        I::LocalTee(3),
        I::I32Load(mem(0)),
        I::I32Eqz,
        I::If(B::Empty),
        I::LocalGet(3),
        I::I64Load(mem(8)),
        I::I64Const(1),
        I::I64Add,
        I::LocalTee(4),
        I::I64Const(0x7fffffffffffff),
        I::I64GtU,
        I::If(B::Empty),
        I::Unreachable,
        I::End,
        I::LocalGet(3),
        I::LocalGet(4),
        I::I64Store(mem(8)),
        I::LocalGet(3),
        I::LocalGet(1),
        I::I32Store(mem(4)),
        I::LocalGet(3),
        I::LocalGet(0),
        I::I32Store(mem(0)),
        I::LocalGet(4),
        I::I64Const(8),
        I::I64Shl,
        I::LocalGet(2),
        I::I64ExtendI32U,
        I::I64Or,
        I::Return,
        I::End,
        I::LocalGet(2),
        I::I32Const(1),
        I::I32Add,
        I::LocalSet(2),
        I::Br(0),
        I::End,
        I::End,
        I::Unreachable,
        I::End,
    ] {
        function.instruction(&instruction);
    }
    function
}
fn get() -> Function {
    body(
        1,
        [
            I::LocalGet(0),
            I::I64Const(0),
            I::I64LeS,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::LocalGet(0),
            I::I32WrapI64,
            I::I32Const(255),
            I::I32And,
            I::I32Const(16),
            I::I32Mul,
            I::I32Const(TABLE),
            I::I32Add,
            I::LocalSet(2),
            I::LocalGet(0),
            I::I64Const(8),
            I::I64ShrU,
            I::LocalGet(2),
            I::I64Load(mem(8)),
            I::I64Ne,
            I::LocalGet(2),
            I::I32Load(mem(0)),
            I::I32Eqz,
            I::I32Or,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::LocalGet(1),
            I::I32Eqz,
            I::If(B::Empty),
            I::Else,
            I::LocalGet(2),
            I::I32Load(mem(4)),
            I::LocalGet(1),
            I::I32Ne,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::End,
            I::LocalGet(2),
            I::I32Load(mem(0)),
        ],
    )
}

fn utf8() -> Function {
    // parameters len; locals offset, lead, sequence width, second min/max, tail, byte.
    body(
        7,
        [
            I::LocalGet(0),
            I::I32Const(CAPACITY),
            I::I32GtU,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::Block(B::Empty),
            I::Loop(B::Empty),
            I::LocalGet(1),
            I::LocalGet(0),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(1),
            I::I32Const(BUFFER),
            I::I32Add,
            I::I32Load8U(byte()),
            I::LocalTee(2),
            I::I32Eqz,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::LocalGet(2),
            I::I32Const(128),
            I::I32LtU,
            I::If(B::Empty),
            I::I32Const(1),
            I::LocalSet(3),
            I::Else,
            I::I32Const(128),
            I::LocalSet(4),
            I::I32Const(191),
            I::LocalSet(5),
            I::LocalGet(2),
            I::I32Const(194),
            I::I32LtU,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::LocalGet(2),
            I::I32Const(224),
            I::I32LtU,
            I::If(B::Empty),
            I::I32Const(2),
            I::LocalSet(3),
            I::Else,
            I::LocalGet(2),
            I::I32Const(240),
            I::I32LtU,
            I::If(B::Empty),
            I::I32Const(3),
            I::LocalSet(3),
            I::LocalGet(2),
            I::I32Const(224),
            I::I32Eq,
            I::If(B::Empty),
            I::I32Const(160),
            I::LocalSet(4),
            I::End,
            I::LocalGet(2),
            I::I32Const(237),
            I::I32Eq,
            I::If(B::Empty),
            I::I32Const(159),
            I::LocalSet(5),
            I::End,
            I::Else,
            I::LocalGet(2),
            I::I32Const(244),
            I::I32GtU,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::I32Const(4),
            I::LocalSet(3),
            I::LocalGet(2),
            I::I32Const(240),
            I::I32Eq,
            I::If(B::Empty),
            I::I32Const(144),
            I::LocalSet(4),
            I::End,
            I::LocalGet(2),
            I::I32Const(244),
            I::I32Eq,
            I::If(B::Empty),
            I::I32Const(143),
            I::LocalSet(5),
            I::End,
            I::End,
            I::End,
            I::LocalGet(1),
            I::LocalGet(3),
            I::I32Add,
            I::LocalGet(0),
            I::I32GtU,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::LocalGet(1),
            I::I32Const(BUFFER + 1),
            I::I32Add,
            I::I32Load8U(byte()),
            I::LocalTee(7),
            I::LocalGet(4),
            I::I32LtU,
            I::LocalGet(7),
            I::LocalGet(5),
            I::I32GtU,
            I::I32Or,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::I32Const(2),
            I::LocalSet(6),
            I::Block(B::Empty),
            I::Loop(B::Empty),
            I::LocalGet(6),
            I::LocalGet(3),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(1),
            I::LocalGet(6),
            I::I32Add,
            I::I32Const(BUFFER),
            I::I32Add,
            I::I32Load8U(byte()),
            I::LocalTee(7),
            I::I32Const(128),
            I::I32LtU,
            I::LocalGet(7),
            I::I32Const(191),
            I::I32GtU,
            I::I32Or,
            I::If(B::Empty),
            I::Unreachable,
            I::End,
            I::LocalGet(6),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(6),
            I::Br(0),
            I::End,
            I::End,
            I::End,
            I::LocalGet(1),
            I::LocalGet(3),
            I::I32Add,
            I::LocalSet(1),
            I::Br(0),
            I::End,
            I::End,
            I::I32Const(1),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_generation_is_full_width_and_exhaustion_never_revives_a_stale_handle() {
        let parsed = crate::parse::parse("fn echo(text: String) -> String: text\n").unwrap();
        let checked = crate::check::check_library(&parsed).unwrap();
        let engine = wasmi::Engine::default();
        let module = wasmi::Module::new(&engine, crate::wasm::compile(&checked).unwrap()).unwrap();
        let mut store = wasmi::Store::new(&engine, ());
        let instance = wasmi::Linker::new(&engine)
            .instantiate_and_start(&mut store, &module)
            .unwrap();
        let memory = instance.get_memory(&store, "morrow_memory").unwrap();
        // Test-only seeding of private metadata avoids billions of operations and
        // requires no production export that could mutate handle generations.
        memory
            .write(
                &mut store,
                (TABLE + 8) as usize,
                &(0x7ffffffffffffeu64).to_le_bytes(),
            )
            .unwrap();
        memory.write(&mut store, BUFFER as usize, b"x").unwrap();
        let new = instance
            .get_typed_func::<i32, i64>(&store, "morrow_string_new")
            .unwrap();
        let read = instance
            .get_typed_func::<i64, i32>(&store, "morrow_string_read")
            .unwrap();
        let release = instance
            .get_typed_func::<i64, i32>(&store, "morrow_release")
            .unwrap();
        let handle = new.call(&mut store, 1).unwrap();
        assert_eq!(handle, 0x7fffffffffffff00);
        assert_eq!(read.call(&mut store, handle).unwrap(), 1);
        release.call(&mut store, handle).unwrap();
        assert!(new.call(&mut store, 1).is_err());
        assert!(read.call(&mut store, handle).is_err());
        assert!(read.call(&mut store, 256).is_err());
    }
}
