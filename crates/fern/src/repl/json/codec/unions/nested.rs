//! Borrow nested shapes only after shallow selection remains ambiguous.
use super::*;
impl Execution<'_, '_> {
    pub(super) fn nested_matches(&mut self, id: usize, input: &Json, depth: usize) -> Result<bool> {
        self.step(depth)?;
        let plan = self.plan;
        match &plan.entries[id].kind {
            Wire::Newtype(child) => self.nested_matches(*child, input, depth + 1),
            Wire::Option(child) => {
                if matches!(input.kind, Kind::Null) {
                    Ok(true)
                } else {
                    self.nested_matches(*child, input, depth + 1)
                }
            }
            Wire::Union(children) => {
                let mut found = false;
                for child in children {
                    found |= self.nested_matches(*child, input, depth + 1)?;
                }
                Ok(found)
            }
            Wire::Record(fields) => {
                if !self.shape_matches(id, input, false, depth + 1)? {
                    return Ok(false);
                }
                let Kind::Object(values, _) = &input.kind else {
                    return Ok(false);
                };
                for (key, value) in values {
                    let Kind::String(key) = &key.kind else {
                        return Ok(false);
                    };
                    for field in fields {
                        if self.same_key(key, &field.name)?
                            && !self.nested_matches(field.codec, value, depth + 1)?
                        {
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            }
            Wire::Tuple(fields) => {
                let Kind::Array(values) = &input.kind else {
                    return Ok(false);
                };
                if fields.len() != values.len() {
                    return Ok(false);
                }
                for (field, value) in fields.iter().zip(values) {
                    if !self.nested_matches(*field, value, depth + 1)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Wire::Sum(variants) => {
                if !self.shape_matches(id, input, false, depth + 1)? {
                    return Ok(false);
                }
                let Kind::Object(values, _) = &input.kind else {
                    return Ok(false);
                };
                let (mut tag, mut fields) = (None, None);
                for (key, value) in values {
                    let Kind::String(key) = &key.kind else {
                        return Ok(false);
                    };
                    if self.same_key(key, "tag")? {
                        tag = Some(value);
                    }
                    if self.same_key(key, "fields")? {
                        fields = Some(value);
                    }
                }
                let (Some(tag), Some(fields)) = (tag, fields) else {
                    return Ok(false);
                };
                let (Kind::String(tag), Kind::Array(values)) = (&tag.kind, &fields.kind) else {
                    return Ok(false);
                };
                for variant in variants {
                    if !self.same_key(tag, &variant.wire_tag)? {
                        continue;
                    }
                    if variant.fields.len() != values.len() {
                        return Ok(false);
                    }
                    for (child, value) in variant.fields.iter().zip(values) {
                        if !self.nested_matches(*child, value, depth + 1)? {
                            return Ok(false);
                        }
                    }
                    return Ok(true);
                }
                Ok(false)
            }
            _ => self.shape_matches(id, input, true, depth + 1),
        }
    }
}
