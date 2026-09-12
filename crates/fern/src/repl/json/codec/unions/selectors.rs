//! Strict key/length or source-tag predicates borrow the DOM and allocate no candidate.
use super::*;
impl Execution<'_, '_> {
    pub(super) fn shape_matches(
        &mut self,
        id: usize,
        input: &Json,
        tags: bool,
        depth: usize,
    ) -> Result<bool> {
        self.step(depth)?;
        let plan = self.plan;
        match &plan.entries[id].kind {
            Wire::Newtype(child) => self.shape_matches(*child, input, tags, depth + 1),
            Wire::Option(child) => {
                if matches!(input.kind, Kind::Null) {
                    Ok(true)
                } else {
                    self.shape_matches(*child, input, tags, depth + 1)
                }
            }
            Wire::Union(children) => {
                let mut found = false;
                for child in children {
                    found |= self.shape_matches(*child, input, tags, depth + 1)?;
                }
                Ok(found)
            }
            Wire::Sum(variants) => match &input.kind {
                Kind::Object(values, _) => {
                    if tags {
                        self.tag_matches(variants, values)
                    } else {
                        self.sum_keys(values)
                    }
                }
                _ => Ok(false),
            },
            Wire::Record(fields) => match &input.kind {
                Kind::Object(values, _) => self.record_keys(fields, values),
                _ => Ok(false),
            },
            Wire::Tuple(fields) => {
                Ok(matches!(&input.kind,Kind::Array(values) if fields.len()==values.len()))
            }
            _ => Ok(self.wire_mask(id, depth + 1)? & kind_bit(&input.kind) != 0),
        }
    }
    fn tag_matches(
        &mut self,
        variants: &[crate::json_codec::Variant],
        values: &[(Json, Json)],
    ) -> Result<bool> {
        for (key, value) in values {
            let Kind::String(key) = &key.kind else {
                return Ok(false);
            };
            if self.same_key(key, "tag")? {
                let Kind::String(tag) = &value.kind else {
                    return Ok(false);
                };
                let mut found = false;
                for variant in variants {
                    found |= self.same_key(tag, &variant.wire_tag)?;
                }
                return Ok(found);
            }
        }
        Ok(false)
    }
    fn sum_keys(&mut self, values: &[(Json, Json)]) -> Result<bool> {
        let (mut tag, mut fields) = (false, false);
        for (key, _) in values {
            let Kind::String(key) = &key.kind else {
                return Ok(false);
            };
            let is_tag = self.same_key(key, "tag")?;
            let is_fields = self.same_key(key, "fields")?;
            if !is_tag && !is_fields {
                return Ok(false);
            }
            tag |= is_tag;
            fields |= is_fields;
        }
        Ok(tag && fields)
    }
    fn record_keys(
        &mut self,
        fields: &[crate::json_codec::Field],
        values: &[(Json, Json)],
    ) -> Result<bool> {
        for (key, _) in values {
            let Kind::String(key) = &key.kind else {
                return Ok(false);
            };
            let mut known = false;
            for field in fields {
                known |= self.same_key(key, &field.name)?;
            }
            if !known {
                return Ok(false);
            }
        }
        for field in fields {
            self.budget.work(1)?;
            if field.optional {
                continue;
            }
            let mut found = false;
            for (key, _) in values {
                if let Kind::String(key) = &key.kind {
                    found |= self.same_key(key, &field.name)?;
                }
            }
            if !found {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
