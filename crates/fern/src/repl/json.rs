//! Immutable JSON semantics with native-profile and aggregate interactive budgets.
use super::*;
mod codec;
use fern_json::{convert, parse};
mod value;
pub(super) use fern_json::{
    ALLOC, Budget, DEPTH, Error, INPUT, Json, Kind, Limits, NODES, Node, OUTPUT, Result, error,
    input,
};

impl Machine {
    /// Route the checked registry through safe semantic values; failures remain Results.
    pub(super) fn json(&mut self, symbol: &str, args: &[Value]) -> Option<Eval<Value>> {
        let operation = symbol.strip_prefix("fern_json_value_")?;
        let limits = if self.cleanup_depth == 0 {
            &mut self.json_limits
        } else {
            &mut self.json_cleanup
        };
        Some(
            match scoped_operation(limits, |limits| dispatch(operation, args, limits)) {
                Ok(value) => Ok(value),
                Err(Error { code: 0, .. }) => Err(fault("interactive evaluation limit exceeded")),
                Err(failure) if matches!(operation, "null" | "from_bool" | "from_int") => {
                    Err(if failure.code == 4 {
                        Failure::JsonLimit
                    } else {
                        fault(message(failure.code))
                    })
                }
                Err(failure) => Ok(Value::Sum(1, Rc::new(vec![Value::JsonError(failure)]))),
            },
        )
    }
}
/// Combine the evaluator's aggregate allowance with an active custom codec operation.
fn scoped_operation<T>(
    global: &mut Limits,
    call: impl FnOnce(&mut Limits) -> Result<T>,
) -> Result<T> {
    let mut limits = fern_json::scope::limits();
    let scope_work = limits.work;
    let scope_alloc = limits.allocated;
    limits.constrain(global);
    let initial = (limits.work, limits.allocated);
    let result = call(&mut limits);
    global.work -= initial.0 - limits.work;
    global.allocated -= initial.1 - limits.allocated;
    match result {
        Err(Error { code: 0, .. }) if scope_work != usize::MAX || scope_alloc != usize::MAX => {
            Err(error(4, -1))
        }
        other => other,
    }
}
/// Wrap the success payload only for APIs whose checked contract returns Result.
fn dispatch(operation: &str, args: &[Value], limits: &mut Limits) -> Result<Value> {
    for argument in args {
        if let Value::String(text) = argument {
            Limits::charge(&mut limits.work, text.len().min(INPUT + 1))?;
        }
    }
    let value = operation_value(operation, args, limits)?;
    if matches!(
        operation,
        "null"
            | "from_bool"
            | "from_int"
            | "is_null"
            | "error_code"
            | "error_offset"
            | "error_message"
            | "error_path"
    ) {
        Ok(value)
    } else {
        Ok(Value::Sum(0, Rc::new(vec![value])))
    }
}
/// Evaluate checked JSON arguments under `limits`, returning a payload or an ordinary JSON error.
fn operation_value(operation: &str, args: &[Value], limits: &mut Limits) -> Result<Value> {
    match (operation, args) {
        ("parse", [Value::String(text)]) => parse::document(input(text), limits).map(Value::Json),
        ("error_code", [Value::JsonError(e)]) => Ok(Value::Int(e.code.into())),
        ("error_offset", [Value::JsonError(e)]) => Ok(Value::Int(e.offset)),
        ("error_path", [Value::JsonError(e)]) => error_path(e, limits),
        ("error_message", [Value::JsonError(e)]) => {
            Ok(Value::String(Rc::new(message(e.code).into())))
        }
        ("is_null", [Value::Json(value)]) => Ok(Value::Bool(matches!(value.kind, Kind::Null))),
        (name, [Value::Json(value), rest @ ..]) => value::access(name, value, rest, limits),
        _ => value::build(operation, args, limits),
    }
}
/// Copy exact visible path text after charging interactive storage, never exposing hidden capacity.
fn error_path(error: &Error, limits: &mut Limits) -> Result<Value> {
    let path = error.path.as_deref().map(String::as_str).unwrap_or("");
    Limits::charge(&mut limits.work, path.len())?;
    Limits::charge(&mut limits.allocated, path.len() + 40)?;
    Ok(Value::String(Rc::new(path.to_owned())))
}
/// Return the static message for a stable error code without embedding input text.
fn message(code: u8) -> &'static str {
    [
        "",
        "invalid JSON syntax",
        "invalid JSON Unicode",
        "duplicate JSON object key",
        "JSON resource limit exceeded",
        "JSON value has wrong type",
        "JSON object key not found",
        "JSON array index out of bounds",
        "JSON number out of range",
        "JSON number is not an integer",
        "JSON string contains NUL",
        "JSON number is not finite",
        "unknown JSON object field",
        "unknown JSON variant",
        "no unique JSON union member",
    ]
    .get(code as usize)
    .copied()
    .unwrap_or("invalid JSON error")
}

/// Count actual retained Rc identities rather than cached expanded subtree sizes.
#[derive(Default)]
pub(super) struct Storage {
    seen: std::collections::HashSet<usize>,
}
impl Storage {
    /// Visit unique JSON nodes reachable from `root`, updating storage counters or rejecting the retained graph.
    pub(super) fn add(
        &mut self,
        root: &Json,
        bytes: &mut usize,
        count: &mut usize,
    ) -> std::result::Result<(), String> {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if !self.seen.insert(Rc::as_ptr(node) as usize) {
                continue;
            }
            *count = count.saturating_add(1);
            *bytes = bytes
                .saturating_add(std::mem::size_of::<Node>() + 2 * std::mem::size_of::<usize>());
            match &node.kind {
                Kind::Number(text) | Kind::String(text) => {
                    *bytes = bytes.saturating_add(text.capacity())
                }
                Kind::Array(values) => {
                    *bytes = bytes.saturating_add(values.capacity() * std::mem::size_of::<Json>());
                    pending.extend(values);
                }
                Kind::Object(values, index) => {
                    *bytes = bytes.saturating_add(
                        values.capacity() * std::mem::size_of::<(Json, Json)>()
                            + index.capacity() * std::mem::size_of::<usize>(),
                    );
                    pending.extend(values.iter().flat_map(|(k, v)| [k, v]));
                }
                _ => {}
            }
            if *bytes > 16 * 1024 * 1024 || count.saturating_add(pending.len()) > 200_000 {
                return Err("interactive value storage limit exceeded".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
