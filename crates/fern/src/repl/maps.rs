//! Immutable interactive maps using the same ordered, semantic key contract as native code.
use super::*;
const MAX_ENTRIES: usize = 65_536;

impl Machine {
    /// Evaluate literal keys and values in source order, including overwritten duplicate values.
    pub(super) fn map_literal(&mut self, pairs: &[(ir::Expr, ir::Expr)]) -> Eval<Value> {
        let mut entries = Vec::new();
        for (key, value) in pairs {
            let key = self.expression(key)?;
            let value = self.expression(value)?;
            insert(&mut entries, key, value)?;
        }
        Ok(Value::Map(Rc::new(entries)))
    }
    /// Handle checked map operations without exposing mutable aliases to their backing vectors.
    pub(super) fn map_builtin(&mut self, builtin: ir::Builtin, args: &[Value]) -> Eval<Value> {
        use ir::Builtin::*;
        match (builtin, args) {
            (MapNew, []) => Ok(Value::Map(Rc::new(Vec::new()))),
            (MapLen, [Value::Map(entries)]) => Ok(Value::Int(entries.len() as i64)),
            (MapIsEmpty, [Value::Map(entries)]) => Ok(Value::Bool(entries.is_empty())),
            (MapGet, [Value::Map(entries), key]) => {
                Ok(match entries.iter().find(|(stored, _)| stored == key) {
                    Some((_, value)) => Value::Sum(0, Rc::new(vec![value.clone()])),
                    None => Value::Sum(1, Rc::new(Vec::new())),
                })
            }
            (MapContains, [Value::Map(entries), key]) => {
                Ok(Value::Bool(entries.iter().any(|(stored, _)| stored == key)))
            }
            (MapPut, [Value::Map(entries), key, value]) => {
                if entries.len() >= MAX_ENTRIES && !entries.iter().any(|(stored, _)| stored == key)
                {
                    return Err(fault("interactive map limit exceeded"));
                }
                let mut changed = entries.as_ref().clone();
                insert(&mut changed, key.clone(), value.clone())?;
                Ok(Value::Map(Rc::new(changed)))
            }
            (MapDelete, [Value::Map(entries), key]) => Ok(Value::Map(Rc::new(
                entries
                    .iter()
                    .filter(|(stored, _)| stored != key)
                    .cloned()
                    .collect(),
            ))),
            (MapKeys | MapValues, [Value::Map(entries)]) => Ok(Value::List(Rc::new(
                entries
                    .iter()
                    .map(|(key, value)| {
                        if builtin == MapKeys {
                            key.clone()
                        } else {
                            value.clone()
                        }
                    })
                    .collect(),
            ))),
            _ => Err(fault("unsupported interactive builtin arguments")),
        }
    }
}
/// Last value wins without moving an existing key; deletion followed by insertion appends.
fn insert(entries: &mut Vec<(Value, Value)>, key: Value, value: Value) -> Eval<()> {
    if let Some((_, old)) = entries.iter_mut().find(|(stored, _)| stored == &key) {
        *old = value;
    } else {
        if entries.len() >= MAX_ENTRIES {
            return Err(fault("interactive map limit exceeded"));
        }
        entries.push((key, value));
    }
    Ok(())
}
