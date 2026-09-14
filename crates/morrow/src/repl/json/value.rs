//! Immutable subtree metadata, indexed objects and bounded semantic projections.
use super::*;
mod build;
pub(super) use build::build;

/// Evaluate a checked projection/conversion, retaining opaque children and returning precise domain errors.
pub(super) fn access(
    operation: &str,
    node: &Json,
    rest: &[Value],
    limits: &mut Limits,
) -> Result<Value> {
    match (operation, &node.kind, rest) {
        ("stringify", _, []) => stringify(node, limits).map(|s| Value::String(Rc::new(s))),
        ("get", Kind::Object(members, index), [Value::String(key)]) => {
            get(members, index, input(key), limits).map(Value::Json)
        }
        ("at", Kind::Array(values), [Value::Int(index)]) => usize::try_from(*index)
            .ok()
            .and_then(|n| values.get(n))
            .cloned()
            .map(Value::Json)
            .ok_or_else(|| error(7, -1)),
        ("length", Kind::Array(values), []) => Ok(Value::Int(values.len() as i64)),
        ("length", Kind::Object(values, _), []) => Ok(Value::Int(values.len() as i64)),
        ("as_bool", Kind::Bool(value), []) => Ok(Value::Bool(*value)),
        ("as_int", Kind::Number(text), []) => {
            Limits::charge(&mut limits.work, text.len() * 2)?;
            convert::integer(text).map(Value::Int)
        }
        ("as_float", Kind::Number(text), []) => {
            Limits::charge(&mut limits.work, text.len())?;
            convert::float(text).map(Value::Float)
        }
        ("number_text", Kind::Number(text), []) => copied(text, limits),
        ("as_string", Kind::String(text), []) if !text.contains('\0') => copied(text, limits),
        ("as_string", Kind::String(_), []) => Err(error(10, -1)),
        ("elements", Kind::Array(values), []) => elements(values, limits),
        ("members", Kind::Object(values, _), []) => members(values, limits),
        _ => Err(error(5, -1)),
    }
}
/// Charge work and output allocation before copying representable text into a Morrow String payload.
fn copied(text: &str, limits: &mut Limits) -> Result<Value> {
    Limits::charge(&mut limits.work, text.len())?;
    Limits::charge(&mut limits.allocated, text.len() + 1)?;
    Ok(Value::String(Rc::new(text.into())))
}
/// Compute actual Rc/Vec/value storage for a bounded semantic collection of `count` entries.
fn collection_bytes(count: usize) -> usize {
    count * std::mem::size_of::<Value>()
        + std::mem::size_of::<Vec<Value>>()
        + 2 * std::mem::size_of::<usize>()
}
/// Copy array references into fresh semantic list storage after charging logical and actual allocation.
fn elements(values: &[Json], limits: &mut Limits) -> Result<Value> {
    Limits::charge(&mut limits.work, values.len())?;
    let actual = collection_bytes(values.len());
    Limits::charge(
        &mut limits.allocated,
        actual.max(values.len().max(1) * 8 + 24),
    )?;
    Ok(Value::List(Rc::new(
        values.iter().cloned().map(Value::Json).collect(),
    )))
}
/// Copy ordered key/value references into tagged semantic tuples under the native adapter and aggregate caps.
fn members(values: &[(Json, Json)], limits: &mut Limits) -> Result<Value> {
    let bytes = values.len().max(1) * 56 + 48;
    Limits::charge(&mut limits.work, values.len())?;
    let actual = collection_bytes(values.len()) + values.len() * collection_bytes(2);
    Limits::charge(&mut limits.allocated, bytes.max(actual))?;
    if bytes > ALLOC {
        return Err(error(4, -1));
    }
    Ok(Value::List(Rc::new(
        values
            .iter()
            .map(|(k, v)| {
                Value::Sum(
                    0,
                    Rc::new(vec![Value::Json(k.clone()), Value::Json(v.clone())]),
                )
            })
            .collect(),
    )))
}

pub(super) use morrow_json::{encode, seal, text_node};
use morrow_json::{get, stringify};
