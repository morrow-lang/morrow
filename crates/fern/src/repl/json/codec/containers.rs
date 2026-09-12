//! Ordered composite wire rules with no child-local budgets or erased error duties.
use super::*;
impl Execution<'_, '_> {
    /// Transparent options use null only after the concrete plan proves nonnullable payloads.
    pub(super) fn encode_container(
        &mut self,
        id: usize,
        input: &Value,
        depth: usize,
    ) -> Result<Json> {
        let plan = self.plan;
        match (&plan.entries[id].kind, input) {
            (Wire::Sum(variants), Value::Sum(tag, fields)) => {
                self.encode_sum(variants, *tag, fields, depth)
            }
            (Wire::Option(_), Value::Sum(1, fields)) if fields.is_empty() => {
                self.scalar(&Value::Unit)
            }
            (Wire::Option(child), Value::Sum(0, fields)) if fields.len() == 1 => {
                self.encode(*child, &fields[0], depth + 1)
            }
            (Wire::List(child), Value::List(values)) => {
                self.encode_array(values, |_| *child, depth)
            }
            (Wire::Tuple(children), Value::Sum(0, values)) if children.len() == values.len() => {
                self.encode_array(values, |i| children[i], depth)
            }
            (Wire::Map(child), Value::Map(values)) => self.encode_map(*child, values, depth),
            (Wire::Record(fields), Value::Sum(0, values)) if fields.len() == values.len() => {
                self.json_slots(fields.len() * 2)?;
                let mut children = Vec::with_capacity(fields.len() * 2);
                for field in fields {
                    children.push(self.text(&field.name)?);
                    children.push(self.at(&field.name, |this| {
                        this.encode(field.codec, &values[field.index], depth + 1)
                    })?);
                }
                value::seal(children, true, 0, &mut self.budget)
            }
            _ => Err(error(5, -1)),
        }
    }
    /// Decode lists/tuples/records into their existing semantic representation after wire-shape checks.
    pub(super) fn decode_container(
        &mut self,
        id: usize,
        input: &Json,
        depth: usize,
    ) -> Result<Value> {
        let plan = self.plan;
        match (&plan.entries[id].kind, &input.kind) {
            (Wire::Sum(variants), Kind::Object(values, _)) => {
                self.decode_sum(variants, values, depth)
            }
            (Wire::Option(child), kind) => {
                self.slots(1)?;
                if matches!(kind, Kind::Null) {
                    Ok(Value::Sum(1, Rc::new(vec![])))
                } else {
                    self.decode(*child, input, depth + 1)
                        .map(|value| Value::Sum(0, Rc::new(vec![value])))
                }
            }
            (Wire::List(child), Kind::Array(values)) => {
                self.budget.allocate(16)?;
                self.decode_array(values, |_| *child, depth)
                    .map(|v| Value::List(Rc::new(v)))
            }
            (Wire::Tuple(children), Kind::Array(values)) if children.len() == values.len() => self
                .decode_array(values, |i| children[i], depth)
                .map(|v| Value::Sum(0, Rc::new(v))),
            (Wire::Map(child), Kind::Object(values, _)) => self.decode_map(*child, values, depth),
            (Wire::Record(fields), Kind::Object(values, _)) => {
                self.decode_record(fields, values, depth)
            }
            _ => Err(error(5, -1)),
        }
    }
    pub(super) fn json_slots(&mut self, count: usize) -> Result<()> {
        if count > NODES {
            return Err(error(4, -1));
        }
        self.budget.work(count)?;
        self.budget.node()?;
        self.budget.allocate(count * 8)
    }
    pub(super) fn encode_array(
        &mut self,
        values: &[Value],
        child: impl Fn(usize) -> usize,
        depth: usize,
    ) -> Result<Json> {
        self.json_slots(values.len())?;
        let mut children = Vec::with_capacity(values.len());
        for (i, value) in values.iter().enumerate() {
            self.budget.work(20)?;
            self.budget.allocate(21)?;
            children.push(self.at(&i.to_string(), |this| {
                this.encode(child(i), value, depth + 1)
            })?);
        }
        value::seal(children, false, 0, &mut self.budget)
    }
    pub(super) fn decode_array(
        &mut self,
        values: &[Json],
        child: impl Fn(usize) -> usize,
        depth: usize,
    ) -> Result<Vec<Value>> {
        self.slots(values.len())?;
        let mut result = Vec::with_capacity(values.len());
        for (i, value) in values.iter().enumerate() {
            self.budget.work(20)?;
            self.budget.allocate(21)?;
            result.push(self.at(&i.to_string(), |this| {
                this.decode(child(i), value, depth + 1)
            })?);
        }
        Ok(result)
    }
    fn encode_map(
        &mut self,
        child: usize,
        values: &[(Value, Value)],
        depth: usize,
    ) -> Result<Json> {
        self.json_slots(values.len() * 2)?;
        let mut children = Vec::with_capacity(values.len() * 2);
        for (key, value) in values {
            let Value::String(key) = key else {
                return Err(error(5, -1));
            };
            let key_node = self.text(key)?;
            let Kind::String(name) = &key_node.kind else {
                return Err(error(5, -1));
            };
            let encoded = self.at(name, |this| this.encode(child, value, depth + 1))?;
            children.push(key_node);
            children.push(encoded);
        }
        value::seal(children, true, 0, &mut self.budget)
    }
    fn decode_map(&mut self, child: usize, values: &[(Json, Json)], depth: usize) -> Result<Value> {
        self.budget.work(values.len())?;
        self.budget.allocate(values.len() * 8 + 24)?;
        Limits::charge(
            &mut self.budget.limits.allocated,
            values.len() * 2 * std::mem::size_of::<Value>() + 40,
        )?;
        let mut result = Vec::with_capacity(values.len());
        for (key, value) in values {
            let Kind::String(key) = &key.kind else {
                return Err(error(5, -1));
            };
            self.budget.work(key.len())?;
            self.budget.allocate(64)?;
            let name = self.string(key)?;
            self.budget.allocate(16)?;
            let decoded = self.at(key, |this| this.decode(child, value, depth + 1))?;
            result.push((name, decoded));
        }
        Ok(Value::Map(Rc::new(result)))
    }
    /// Reject unknown input keys before reading required fields; source field order controls missing diagnostics.
    fn decode_record(
        &mut self,
        fields: &[crate::json_codec::Field],
        values: &[(Json, Json)],
        depth: usize,
    ) -> Result<Value> {
        for (key, _) in values {
            let Kind::String(name) = &key.kind else {
                return Err(error(5, -1));
            };
            self.budget.work(name.len())?;
            if name.contains('\0') {
                return Err(error(10, -1));
            }
            let mut known = false;
            for field in fields {
                known |= self.same_key(name, &field.name)?;
            }
            if !known {
                return self.at(name, |_| Err(error(12, -1)));
            }
        }
        self.slots(fields.len())?;
        let mut result = Vec::with_capacity(fields.len());
        for field in fields {
            let mut found = None;
            for (key, value) in values {
                if let Kind::String(name) = &key.kind
                    && self.same_key(name, &field.name)?
                {
                    found = Some(value);
                    break;
                }
            }
            self.budget.work(field.name.len() + 1)?;
            let decoded = self.at(&field.name, |this| match found {
                Some(value) => this.decode(field.codec, value, depth + 1),
                None if field.optional => {
                    this.slots(1)?;
                    Ok(Value::Sum(1, Rc::new(vec![])))
                }
                None => Err(error(6, -1)),
            })?;
            result.push(decoded);
        }
        Ok(Value::Sum(0, Rc::new(result)))
    }
    pub(super) fn same_key(&mut self, a: &str, b: &str) -> Result<bool> {
        self.budget.work(b.len() + 1 + a.len().min(b.len()) + 1)?;
        Ok(a == b)
    }
}
