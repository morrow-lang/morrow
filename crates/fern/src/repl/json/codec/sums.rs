//! Stable constructor envelopes share the ordinary JSON budget and path storage.
use super::*;
use crate::json_codec::Variant;
impl Execution<'_, '_> {
    /// Emit source tag then ordered payload fields; native constructor ordinal is never wire text.
    pub(super) fn encode_sum(
        &mut self,
        variants: &[Variant],
        tag: usize,
        fields: &[Value],
        depth: usize,
    ) -> Result<Json> {
        let Some(variant) = variants.get(tag) else {
            return self.at("tag", |_| Err(error(13, -1)));
        };
        if fields.len() != variant.fields.len() {
            return self.at("fields", |_| Err(error(5, -1)));
        }
        self.json_slots(4)?;
        let tag_key = self.text("tag")?;
        let tag = self.at("tag", |this| {
            this.step(depth + 1)?;
            this.text(&variant.wire_tag)
        })?;
        let fields_key = self.text("fields")?;
        let fields = self.at("fields", |this| {
            this.step(depth + 1)?;
            this.encode_array(fields, |i| variant.fields[i], depth + 1)
        })?;
        value::seal(
            vec![tag_key, tag, fields_key, fields],
            true,
            0,
            &mut self.budget,
        )
    }
    /// Validate the whole envelope before any source payload allocation or conversion.
    pub(super) fn decode_sum(
        &mut self,
        variants: &[Variant],
        values: &[(Json, Json)],
        depth: usize,
    ) -> Result<Value> {
        let (tag, fields) = self.envelope(values)?;
        let tag = self.at("tag", |this| {
            this.step(depth + 1)?;
            let Kind::String(name) = &tag.kind else {
                return Err(error(5, -1));
            };
            for (index, variant) in variants.iter().enumerate() {
                if this.same_key(name, &variant.wire_tag)? {
                    return Ok(index);
                }
            }
            Err(error(13, -1))
        })?;
        self.at("fields", |this| {
            this.step(depth + 1)?;
            let Kind::Array(values) = &fields.kind else {
                return Err(error(5, -1));
            };
            let children = &variants[tag].fields;
            if values.len() != children.len() {
                return Err(error(5, -1));
            }
            let fields = this.decode_array(values, |i| children[i], depth + 1)?;
            Ok(Value::Sum(tag, Rc::new(fields)))
        })
    }
    /// Unknown input keys win in input order, then missing tag/fields in canonical order.
    fn envelope<'j>(&mut self, values: &'j [(Json, Json)]) -> Result<(&'j Json, &'j Json)> {
        let (mut tag, mut fields) = (None, None);
        for (key, value) in values {
            let Kind::String(name) = &key.kind else {
                return Err(error(5, -1));
            };
            self.budget.work(name.len())?;
            if name.contains('\0') {
                return Err(error(10, -1));
            }
            let is_tag = self.same_key(name, "tag")?;
            let is_fields = self.same_key(name, "fields")?;
            if is_tag {
                tag = Some(value);
            } else if is_fields {
                fields = Some(value);
            } else {
                return self.at(name, |_| Err(error(12, -1)));
            }
        }
        let tag = match tag {
            Some(v) => v,
            None => return self.at("tag", |_| Err(error(6, -1))),
        };
        let fields = match fields {
            Some(v) => v,
            None => return self.at("fields", |_| Err(error(6, -1))),
        };
        Ok((tag, fields))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_and_payload_budget_matches_native_before_second_scalar() {
        use crate::json_codec::Entry;
        let plan = Plan {
            root: 1,
            entries: vec![
                Entry {
                    ty: Type::Int,
                    kind: Wire::Int,
                },
                Entry {
                    ty: Type::Named("PairType".into(), vec![]),
                    kind: Wire::Sum(vec![Variant {
                        wire_tag: "Pair".into(),
                        fields: vec![0, 0],
                    }]),
                },
            ],
        };
        let mut limits = Limits::new(64 * 1024 * 1024);
        let budget = Budget {
            limits: &mut limits,
            work: 500,
            allocated: 128,
            nodes: 0,
            at: 0,
        };
        let mut execution = Execution {
            plan: &plan,
            budget,
            path: Rc::new(String::new()),
        };
        let failure = execution
            .encode(
                1,
                &Value::Sum(0, Rc::new(vec![Value::Int(1), Value::Int(2)])),
                0,
            )
            .unwrap_err();
        assert_eq!(
            (
                failure.code,
                failure.offset,
                failure.path.as_deref().map(String::as_str)
            ),
            (4, -1, Some("/fields/1"))
        );
        assert_eq!(
            (
                execution.budget.nodes,
                execution.budget.allocated,
                execution.budget.work
            ),
            (6, 1054, 111)
        );
    }
}
