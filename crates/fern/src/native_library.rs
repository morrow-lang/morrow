//! Explicit native host adapters for checked Fern libraries.
//!
//! Each exported function receives `(fault: *mut i64, execution: *mut Exec,
//! ...source arguments)`. The host owns both cells and all argument lifetimes on
//! one invocation thread. It must inspect the fault cell before using a result,
//! retain managed results through registered roots, and never serialize pointers.
//! The compiler emits no startup, interpreter, loader or process-exit wrapper.
use crate::{
    Diagnostic, Span, ir, lowering,
    machine::{self, Operand, Operation, Scalar, Statement},
};
use std::collections::BTreeSet;

/// A checked source identity and a public suffix under the `fern_export_` namespace.
pub struct Export {
    pub function: String,
    pub symbol: String,
}
impl Export {
    pub fn new(function: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self {
            function: function.into(),
            symbol: symbol.into(),
        }
    }
}

/// Build explicitly selected adapters, rejecting capture and actor continuation
/// entry points whose hidden calling conventions are not ordinary host calls.
pub fn lower(program: &ir::Program, exports: &[Export]) -> Result<machine::Program, Diagnostic> {
    let invalid = |message: &str| Diagnostic::new(Span::default(), message);
    if exports.is_empty() || exports.len() > 128 {
        return Err(invalid("native library requires between 1 and 128 exports"));
    }
    let mut names = BTreeSet::new();
    let mut selected = Vec::new();
    for export in exports {
        if export.symbol.is_empty()
            || export.symbol.len() > 64
            || !export
                .symbol
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !names.insert(&export.symbol)
        {
            return Err(invalid(
                "native library export symbols must be unique ASCII identifiers",
            ));
        }
        let mut matching = program
            .functions
            .iter()
            .filter(|f| f.name == export.function);
        let function = matching
            .next()
            .ok_or_else(|| invalid("native library export function is absent"))?;
        if matching.next().is_some()
            || !function.captures.is_empty()
            || function.mailbox.is_some()
            || function.params.len() > 64
        {
            return Err(invalid(
                "native library export must be one non-receiving function without captures",
            ));
        }
        selected.push((export, function.id));
    }
    let mut machine = lowering::lower_library(program)?;
    for (export, id) in selected {
        let function = machine
            .functions
            .iter()
            .find(|f| f.name == format!("$f{}", id.0))
            .ok_or_else(|| invalid("native library function has no ordinary native entry"))?;
        let mut params = vec![(Scalar::I64, "fault".into()), (Scalar::I64, "exec".into())];
        let mut args = Vec::new();
        let mut next = 0;
        for (ty, name) in &function.params {
            let operand = match name.as_str() {
                "%env" => Operand::Int(0),
                "%fault" => Operand::Temp("fault".into()),
                "%exec" => Operand::Temp("exec".into()),
                _ => {
                    let name = format!("arg{next}");
                    next += 1;
                    params.push((*ty, name.clone()));
                    Operand::Temp(name)
                }
            };
            args.push((*ty, operand));
        }
        let result = function
            .result
            .ok_or_else(|| invalid("native library function has no value representation"))?;
        let adapter = machine::Function {
            name: format!("fern_export_{}", export.symbol),
            result: Some(result),
            params,
            export: true,
            body: vec![
                Statement::Label("start".into()),
                Statement::Assign {
                    destination: "result".into(),
                    ty: result,
                    operation: Operation::Call {
                        callee: Operand::Symbol(function.name.clone()),
                        args,
                        variadic: None,
                    },
                },
                Statement::Return(Some(Operand::Temp("result".into()))),
            ],
        };
        machine.functions.push(adapter);
    }
    machine.validate().map_err(|message| invalid(&message))?;
    Ok(machine)
}
