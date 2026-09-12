//! Native builder contracts, copied outer storage and cached shared subtrees.
use super::*;
/// Dispatch checked builder arguments into immutable nodes; all builder-domain offsets are normalized to -1.
pub(in crate::repl::json) fn build(
    operation: &str,
    args: &[Value],
    limits: &mut Limits,
) -> Result<Value> {
    let result = match (operation, args) {
        ("null", []) => primitive(Kind::Null, 4, limits),
        ("from_bool", [Value::Bool(value)]) => {
            primitive(Kind::Bool(*value), if *value { 4 } else { 5 }, limits)
        }
        ("from_int", [Value::Int(value)]) => number(&value.to_string(), limits),
        ("from_float", [Value::Float(value)]) => {
            Limits::charge(&mut limits.work, 64)?;
            Limits::charge(&mut limits.allocated, 64)?;
            convert::format(*value).and_then(|s| number(&s, limits))
        }
        ("from_number_text", [Value::String(text)]) => number(input(text), limits),
        ("from_string", [Value::String(text)]) => string(input(text), limits),
        ("from_array", [Value::List(values)]) => array(values, limits),
        ("from_object", [Value::Map(values)]) => object(values, limits),
        _ => Err(error(5, -1)),
    };
    result
        .map(Value::Json)
        .map_err(|e| Error { offset: -1, ..e })
}
/// Reserve and construct a null/Boolean node using its known compact encoded length.
fn primitive(kind: Kind, encoded: usize, limits: &mut Limits) -> Result<Json> {
    Budget::new(limits, 0)?.node()?;
    Ok(Rc::new(Node {
        kind,
        offset: 0,
        height: 1,
        nodes: 1,
        encoded,
    }))
}
/// Require bounded scalar input, then preserve exactly one validated number token.
fn number(text: &str, limits: &mut Limits) -> Result<Json> {
    if text.len() > INPUT {
        return Err(error(4, -1));
    }
    parse::number(text, Budget::new(limits, text.len())?)
}
/// Require bounded decoded source text and copy it into a new immutable JSON String.
fn string(text: &str, limits: &mut Limits) -> Result<Json> {
    if text.len() > INPUT {
        return Err(error(4, -1));
    }
    let mut budget = Budget::new(limits, text.len())?;
    owned_text(text, &mut budget)
}
/// Charge node/text storage before copying valid UTF-8 into an immutable key or String node.
fn owned_text(text: &str, budget: &mut Budget<'_>) -> Result<Json> {
    budget.node()?;
    budget.allocate(text.len() + 1)?;
    Ok(text_node(text.to_owned(), 0))
}
/// Validate the bounded child count, copy opaque references once, and seal expanded metadata.
fn array(values: &[Value], limits: &mut Limits) -> Result<Json> {
    if values.len() >= NODES {
        return Err(error(4, -1));
    }
    let mut budget = Budget::new(limits, 0)?;
    budget.node()?;
    budget.allocate(values.len() * 8)?;
    Limits::charge(&mut budget.limits.work, values.len())?;
    let mut children = Vec::with_capacity(values.len());
    for value in values {
        let Value::Json(value) = value else {
            return Err(error(5, -1));
        };
        children.push(value.clone());
    }
    seal(children, false, 0, &mut budget)
}
/// Charge bounded key scans before copying; reject oversized individual or aggregate decoded key text.
fn key_bytes(values: &[(Value, Value)], limits: &mut Limits) -> Result<usize> {
    let mut bytes = 0;
    for (key, _) in values {
        let Value::String(key) = key else {
            return Err(error(5, -1));
        };
        Limits::charge(&mut limits.work, key.len().min(INPUT + 1))?;
        let length = input(key).len();
        if length > INPUT || length > OUTPUT - bytes {
            return Err(error(4, -1));
        }
        bytes += length;
    }
    Ok(bytes)
}
/// Build copied keys and retained values in source order, including native bridge reservations and index checks.
fn object(values: &[(Value, Value)], limits: &mut Limits) -> Result<Json> {
    if values.len() > (NODES - 1) / 2 {
        return Err(error(4, -1));
    }
    let bytes = key_bytes(values, limits)?;
    let mut budget = Budget::new(limits, bytes)?;
    budget.allocate(values.len().max(1) * 16 + 48)?;
    budget.node()?;
    budget.allocate(values.len() * 16)?;
    let mut children = Vec::with_capacity(values.len() * 2);
    for (key, value) in values {
        let (Value::String(key), Value::Json(value)) = (key, value) else {
            return Err(error(5, -1));
        };
        children.push(owned_text(input(key), &mut budget)?);
        children.push(value.clone());
    }
    seal(children, true, 0, &mut budget)
}
