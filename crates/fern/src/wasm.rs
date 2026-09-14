//! Portable WebAssembly backend, branching before native pointer-width lowering.
//!
//! Scalar functions export their checked names: Int is i64, Float is f64, and
//! Bool/Unit are i32. Pure scalar modules need no memory or allocator. Immutable
//! strings, records, variants, tuples and lists use a bounded precisely traced
//! heap. Managed signatures export as `fern::<checked-name>`, replacing each
//! managed argument/result with a rooted, generational i64 handle. The versioned
//! UTF-8 scratch API copies string data without exposing unrooted heap pointers.
//! Modules require no imports, WASI or native runtime. Unsupported capabilities
//! are rejected throughout the program rather than silently omitted.
mod emit;
mod heap;
mod host;
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
/// Every concrete function has a scalar or managed-handle export. All functions are checked,
/// including uncalled imported source bodies. Unsupported capabilities and malformed public IR return a
/// diagnostic before any module bytes are published. Execution hosts must set
/// their own work limits; recursive Fern calls use the WebAssembly call stack.
pub fn compile(program: &ir::Program) -> Result<Vec<u8>> {
    if program.functions.len() > 4096 {
        return Err(invalid(Span::default(), "function limit exceeded"));
    }
    crate::ir::reject_probes(program)?;
    validate_types(program)?;
    let runtime = strings::Runtime::prepare(program)?;
    let mut identities = BTreeMap::new();
    for function in &program.functions {
        if function.name.len() > 65_536 {
            return Err(invalid(
                function.body.span,
                "function export name limit exceeded",
            ));
        }
    }
    let mut multiplicity = BTreeMap::new();
    for function in &program.functions {
        *multiplicity.entry(function.name.as_str()).or_insert(0usize) += 1;
    }
    let export_names: Vec<_> = program
        .functions
        .iter()
        .map(|function| {
            if multiplicity[function.name.as_str()] > 1 {
                format!("{}::specialization::{}", function.name, function.id.0)
            } else {
                function.name.clone()
            }
        })
        .collect();
    let mut names = BTreeSet::new();
    let mut types = TypeSection::new();
    let mut functions = FunctionSection::new();
    let mut exports = ExportSection::new();
    let mut export_bytes = 0usize;
    for (index, function) in program.functions.iter().enumerate() {
        let span = function.body.span;
        if function.mailbox.is_some() {
            return Err(invalid(span, "actors require a native runtime"));
        }
        if function.name.len() > 65_536 || !names.insert(&export_names[index]) {
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
        if function.params.len() + function.captures.len() > 1024
            || function.captures.len() > 511
            || function.local_count > 65_536
        {
            return Err(invalid(span, "function local or parameter limit exceeded"));
        }
        let params = function
            .captures
            .iter()
            .chain(&function.params)
            .map(|p| value_type(&p.ty, span))
            .collect::<Result<Vec<_>>>()?;
        let result = value_type(&function.return_type, span)?;
        types.ty().function(params, [result]);
        functions.function(index);
        if runtime.is_none() {
            exports.export(&export_names[index as usize], ExportKind::Func, index);
        }
    }
    if let Some(runtime) = &runtime {
        runtime.declarations(&mut types, &mut functions);
        host::declarations(runtime, &mut types, &mut functions, &mut exports);
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
        host::code(runtime, &mut code);
        for (index, function) in program.functions.iter().enumerate() {
            if !function.captures.is_empty() {
                continue;
            }
            let managed_signature =
                managed(&function.return_type) || function.params.iter().any(|p| managed(&p.ty));
            let wrapper_index = functions.len();
            if managed_signature {
                let parameters = function
                    .params
                    .iter()
                    .map(|param| {
                        if managed(&param.ty) {
                            Ok(ValType::I64)
                        } else {
                            value_type(&param.ty, function.body.span)
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                let result = if managed(&function.return_type) {
                    ValType::I64
                } else {
                    value_type(&function.return_type, function.body.span)?
                };
                functions.function(types.len());
                types.ty().function(parameters, [result]);
            } else {
                functions.function(index as u32);
            }
            exports.export(
                &if managed_signature {
                    format!("fern::{}", export_names[index])
                } else {
                    export_names[index].clone()
                },
                ExportKind::Func,
                wrapper_index,
            );
            let mut wrapper = wasm_encoder::Function::new([]);
            wrapper.instruction(&wasm_encoder::Instruction::I32Const(0));
            wrapper.instruction(&wasm_encoder::Instruction::GlobalSet(2));
            wrapper.instruction(&wasm_encoder::Instruction::I32Const(0));
            wrapper.instruction(&wasm_encoder::Instruction::GlobalSet(0));
            for param in 0..function.params.len() {
                wrapper.instruction(&wasm_encoder::Instruction::LocalGet(param as u32));
                if managed(&function.params[param].ty) {
                    wrapper.instruction(&wasm_encoder::Instruction::I32Const(
                        runtime.type_ids[&function.params[param].ty] as i32,
                    ));
                    wrapper.instruction(&wasm_encoder::Instruction::Call(
                        runtime.first + strings::COUNT + host::GET,
                    ));
                    wrapper.instruction(&wasm_encoder::Instruction::Call(
                        runtime.first + strings::PUSH,
                    ));
                }
            }
            wrapper.instruction(&wasm_encoder::Instruction::Call(index as u32));
            wrapper.instruction(&wasm_encoder::Instruction::GlobalGet(2));
            wrapper.instruction(&wasm_encoder::Instruction::If(
                wasm_encoder::BlockType::Empty,
            ));
            wrapper.instruction(&wasm_encoder::Instruction::Unreachable);
            wrapper.instruction(&wasm_encoder::Instruction::End);
            if managed(&function.return_type) {
                wrapper.instruction(&wasm_encoder::Instruction::I32Const(
                    runtime.type_ids[&function.return_type] as i32,
                ));
                wrapper.instruction(&wasm_encoder::Instruction::Call(
                    runtime.first + strings::COUNT + host::NEW,
                ));
            }
            wrapper.instruction(&wasm_encoder::Instruction::I32Const(0));
            wrapper.instruction(&wasm_encoder::Instruction::GlobalSet(0));
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
        Type::Bool | Type::Unit => Ok(ValType::I32),
        ty if managed(ty) => Ok(ValType::I32),
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
fn managed(ty: &Type) -> bool {
    matches!(
        ty,
        Type::String
            | Type::Tuple(_)
            | Type::Named(_, _)
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::List(_)
            | Type::Map(_, _)
            | Type::Function(_, _)
            | Type::Range
            | Type::Union(_)
    )
}

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

/// Bound caller-built semantic types before hashing, cloning or formatting them.
fn validate_types(program: &ir::Program) -> Result<()> {
    let mut roots = Vec::new();
    for function in &program.functions {
        roots.push(&function.return_type);
        roots.extend(
            function
                .params
                .iter()
                .chain(&function.captures)
                .map(|p| &p.ty),
        );
        let mut expressions = vec![&function.body];
        while let Some(expr) = expressions.pop() {
            roots.push(&expr.ty);
            expressions.extend(ir::children(expr));
        }
    }
    for layout in &program.types {
        roots.push(&layout.ty);
        roots.extend(layout.variants.iter().flatten());
    }
    let mut pending: Vec<_> = roots.into_iter().map(|ty| (ty, 0)).collect();
    let mut work = 0usize;
    let mut unions = Vec::new();
    let mut keys = Vec::new();
    while let Some((ty, depth)) = pending.pop() {
        work += 1;
        if work > 400_000 || depth >= 128 {
            return Err(invalid(Span::default(), "type complexity limit exceeded"));
        }
        let mut child = |ty| pending.push((ty, depth + 1));
        match ty {
            Type::List(item) | Type::Option(item) => child(item.as_ref()),
            Type::Map(key, value) | Type::Result(key, value) => {
                if matches!(ty, Type::Map(_, _)) {
                    keys.push(key.as_ref());
                }
                child(key.as_ref());
                child(value.as_ref());
            }
            Type::Union(items) => {
                unions.push(items);
                if !(2..=128).contains(&items.len())
                    || items.iter().any(|item| matches!(item, Type::Union(_)))
                {
                    return Err(invalid(Span::default(), "union members must be canonical"));
                }
                for item in items {
                    child(item);
                }
            }
            Type::Tuple(items) | Type::Named(_, items) => {
                for item in items {
                    child(item);
                }
            }
            Type::Function(params, result) => {
                child(result.as_ref());
                for param in params {
                    child(param);
                }
            }
            Type::Never => {}
            _ => {
                value_type(ty, Span::default())?;
            }
        }
    }
    for members in unions {
        if members.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid(
                Span::default(),
                "union members must be sorted and distinct",
            ));
        }
    }
    for mut key in keys {
        let mut depth = 0;
        while matches!(key, Type::Named(_, _)) {
            depth += 1;
            if depth > 128 {
                return Err(invalid(Span::default(), "map key representation cycle"));
            }
            let layout = program
                .types
                .iter()
                .find(|layout| &layout.ty == key)
                .ok_or_else(|| {
                    invalid(
                        Span::default(),
                        "map key requires a resolved scalar newtype",
                    )
                })?;
            if layout.storage != ir::LayoutStorage::Unboxed
                || layout.variants.len() != 1
                || layout.variants[0].len() != 1
            {
                return Err(invalid(
                    Span::default(),
                    "map key requires a scalar newtype",
                ));
            }
            key = &layout.variants[0][0];
        }
        if !matches!(key, Type::Int | Type::Bool | Type::String) {
            return Err(invalid(
                Span::default(),
                "map keys require Int, Bool, String, or scalar newtypes",
            ));
        }
    }
    Ok(())
}
