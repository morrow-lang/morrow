//! Portable WebAssembly backend, branching before native pointer-width lowering.
//!
//! This first ABI exports concrete Fern functions by checked name: Int is i64,
//! Float is f64, and Bool/Unit are i32 (Unit is zero). Pure scalar modules have no
//! memory or allocator. Strings opt into a bounded precise tracing heap with
//! internal i32 pointers; managed signatures are not host exports. Modules need
//! no imports, JavaScript, WASI or native runtime. Unsupported capabilities are
//! rejected throughout the program rather than silently omitted.
//! This intentionally does not freeze the future managed browser ABI.
mod emit;
mod numeric;
mod strings;

use crate::{Diagnostic, Span, Type, ir};
use std::collections::{BTreeMap, BTreeSet};
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, FunctionSection, Module, TypeSection, ValType,
};

type Result<T> = std::result::Result<T, Diagnostic>;

/// Compile checked semantic IR into a validated core WebAssembly module.
///
/// Every scalar-signature function is an export. All functions are checked,
/// including uncalled imported source bodies. Unsupported capabilities and malformed public IR return a
/// diagnostic before any module bytes are published. Execution hosts must set
/// their own work limits; recursive Fern calls use the WebAssembly call stack.
pub fn compile(program: &ir::Program) -> Result<Vec<u8>> {
    if program.functions.len() > 4096 {
        return Err(invalid(Span::default(), "function limit exceeded"));
    }
    crate::ir::reject_probes(program)?;
    let runtime = strings::Runtime::prepare(program)?;
    let mut identities = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut types = TypeSection::new();
    let mut functions = FunctionSection::new();
    let mut exports = ExportSection::new();
    let mut export_bytes = 0usize;
    for (index, function) in program.functions.iter().enumerate() {
        let span = function.body.span;
        if function.mailbox.is_some() || !function.captures.is_empty() {
            return Err(invalid(
                span,
                "actors and captured closures require a managed browser runtime",
            ));
        }
        if function.name.len() > 65_536 || !names.insert(&function.name) {
            return Err(invalid(span, "invalid or duplicate function export name"));
        }
        export_bytes += function.name.len();
        if export_bytes > 8 * 1024 * 1024 {
            return Err(invalid(span, "export name byte limit exceeded"));
        }
        let index = index as u32; // Function count is bounded above.
        if identities
            .insert(function.id.0, (index, function))
            .is_some()
        {
            return Err(invalid(span, "duplicate function identity"));
        }
        if function.params.len() > 1024 || function.local_count > 65_536 {
            return Err(invalid(span, "function local or parameter limit exceeded"));
        }
        let params = function
            .params
            .iter()
            .map(|p| value_type(&p.ty, span))
            .collect::<Result<Vec<_>>>()?;
        let result = value_type(&function.return_type, span)?;
        types.ty().function(params, [result]);
        functions.function(index);
        if runtime.is_none() {
            exports.export(&function.name, ExportKind::Func, index);
        }
    }
    if let Some(runtime) = &runtime {
        runtime.declarations(&mut types, &mut functions);
    }
    let mut code = CodeSection::new();
    let mut work = 0;
    for function in &program.functions {
        code.function(&emit::function(
            function,
            &identities,
            &mut work,
            runtime.as_ref(),
        )?);
    }
    if let Some(runtime) = &runtime {
        runtime.code(&mut code);
        for (index, function) in program.functions.iter().enumerate() {
            if function.return_type == Type::String
                || function.params.iter().any(|p| p.ty == Type::String)
            {
                continue;
            }
            let wrapper_index = functions.len();
            functions.function(index as u32);
            exports.export(&function.name, ExportKind::Func, wrapper_index);
            let mut wrapper = wasm_encoder::Function::new([]);
            wrapper.instruction(&wasm_encoder::Instruction::I32Const(0));
            wrapper.instruction(&wasm_encoder::Instruction::GlobalSet(0));
            for param in 0..function.params.len() {
                wrapper.instruction(&wasm_encoder::Instruction::LocalGet(param as u32));
            }
            wrapper.instruction(&wasm_encoder::Instruction::Call(index as u32));
            wrapper.instruction(&wasm_encoder::Instruction::End);
            code.function(&wrapper);
        }
    }
    let mut module = Module::new();
    module.section(&types).section(&functions);
    if let Some(runtime) = &runtime {
        module
            .section(&runtime.memory())
            .section(&runtime.globals());
    }
    module.section(&exports).section(&code);
    if let Some(runtime) = &runtime {
        module.section(&runtime.data());
    }
    let bytes = module.finish();
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid(Span::default(), "module output limit exceeded"));
    }
    wasmparser::Validator::new()
        .validate_all(&bytes)
        .map_err(|error| {
            invalid(
                Span::default(),
                format!("generated module validation failed: {error}"),
            )
        })?;
    Ok(bytes)
}

fn value_type(ty: &Type, span: Span) -> Result<ValType> {
    match ty {
        Type::Int => Ok(ValType::I64),
        Type::Float => Ok(ValType::F64),
        Type::Bool | Type::Unit | Type::String => Ok(ValType::I32),
        _ => Err(invalid(
            span,
            format!(
                "type {} is not supported by the browser value ABI",
                type_name(ty)
            ),
        )),
    }
}

// Never recursively format a caller-constructed type when rejecting public IR.
fn type_name(ty: &Type) -> &'static str {
    match ty {
        Type::Never => "Never",
        Type::Range => "Range",
        Type::Int => "Int",
        Type::Float => "Float",
        Type::Bool => "Bool",
        Type::String => "String",
        Type::Unit => "Unit",
        Type::Native(_) => "native handle",
        Type::Union(_) => "Union",
        Type::Tuple(_) => "Tuple",
        Type::Function(_, _) => "Function callable",
        Type::Pid(_) => "Pid",
        Type::ActorFunction(_, _) => "ActorFunction",
        Type::List(_) => "List",
        Type::Map(_, _) => "Map",
        Type::Option(_) => "Option",
        Type::Result(_, _) => "Result",
        Type::Infer(_) => "inference variable",
        Type::Named(_, _) => "nominal value",
        Type::Generic(_) => "generic variable",
    }
}

fn invalid(span: Span, message: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::new(span, format!("wasm32: {message}"))
}

fn expect(actual: &Type, expected: &Type, span: Span) -> Result<()> {
    if actual == expected || *actual == Type::Never {
        Ok(())
    } else {
        Err(invalid(
            span,
            format!("expected {expected:?}, got {actual:?}"),
        ))
    }
}
