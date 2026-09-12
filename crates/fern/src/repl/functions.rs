//! Typed callback execution without invoking the legacy native callback ABI.
use super::*;

impl Machine {
    /// Dispatch checked higher-order operations, evaluating only the selected callbacks.
    pub(super) fn higher_order(&mut self, builtin: ir::Builtin, args: &[Value]) -> Eval<Value> {
        use ir::Builtin::*;
        match (builtin, args) {
            (ListEnumerate, [Value::List(xs)]) => self.enumerate(xs),
            (ListMap | ListFilter | ListFind | ListAny | ListAll, [Value::List(xs), callback]) => {
                self.list_callback(builtin, xs, callback)
            }
            (ListFold, [Value::List(xs), initial, callback]) => {
                let mut value = initial.clone();
                for item in xs.iter() {
                    value = self.invoke(callback, vec![value, item.clone()])?;
                }
                Ok(value)
            }
            (
                OptionMap | ResultMap | ResultAndThen | ResultUnwrapOrElse,
                [value @ Value::Sum(tag, fields), callback],
            ) => self.sum_callback(builtin, value, *tag, fields, callback),
            _ => self.map_builtin(builtin, args),
        }
    }
    /// Preserve input order and terminate predicates at their first deciding element.
    fn list_callback(
        &mut self,
        builtin: ir::Builtin,
        xs: &[Value],
        callback: &Value,
    ) -> Eval<Value> {
        use ir::Builtin::*;
        if xs.len() > 65_536 {
            return Err(fault("interactive list limit exceeded"));
        }
        let mut mapped = Vec::new();
        for item in xs {
            let value = self.invoke(callback, vec![item.clone()])?;
            if builtin == ListMap {
                mapped.push(value);
                continue;
            }
            let Value::Bool(selected) = value else {
                return Err(fault("invalid callback predicate"));
            };
            match builtin {
                ListFilter if selected => mapped.push(item.clone()),
                ListFind if selected => return Ok(Value::Sum(0, Rc::new(vec![item.clone()]))),
                ListAny if selected => return Ok(Value::Bool(true)),
                ListAll if !selected => return Ok(Value::Bool(false)),
                _ => {}
            }
        }
        Ok(match builtin {
            ListMap | ListFilter => Value::List(Rc::new(mapped)),
            ListFind => Value::Sum(1, Rc::new(Vec::new())),
            ListAll => Value::Bool(true),
            ListAny => Value::Bool(false),
            _ => return Err(fault("invalid list callback")),
        })
    }
    /// Preserve absent/error values and catch callback Result propagation at its own function.
    fn sum_callback(
        &mut self,
        builtin: ir::Builtin,
        original: &Value,
        tag: usize,
        fields: &[Value],
        callback: &Value,
    ) -> Eval<Value> {
        use ir::Builtin::*;
        if builtin == ResultUnwrapOrElse {
            let field = fields
                .first()
                .ok_or_else(|| fault("missing Result payload"))?;
            return if tag == 0 {
                Ok(field.clone())
            } else {
                self.invoke(callback, vec![field.clone()])
            };
        }
        if tag == 1 {
            return Ok(original.clone());
        }
        let field = fields.first().ok_or_else(|| fault("missing sum payload"))?;
        let value = self.invoke(callback, vec![field.clone()])?;
        if builtin == ResultAndThen {
            Ok(value)
        } else {
            Ok(Value::Sum(0, Rc::new(vec![value])))
        }
    }
}
