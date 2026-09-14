//! Borrowed structural refinement; no candidate decoder or payload allocation.
use super::*;
#[cfg(test)]
#[path = "nested_tests.rs"]
mod tests;
impl Execution<'_> {
    /// The caller has validated the complete immutable descriptor graph and DOM.
    pub(super) unsafe fn nested_shape(
        &mut self,
        p: *const Codec,
        value: &Json,
        depth: usize,
    ) -> Result<bool> {
        // SAFETY: children/names/count come from the fully audited descriptor;
        // recursion borrows it and is charged against the existing execution budget.
        unsafe {
            self.step(depth)?;
            match (*p).kind {
                11 => self.nested_shape(*(*p).children, value, depth + 1),
                7 => {
                    if matches!(value.kind, Kind::Null) {
                        Ok(true)
                    } else {
                        self.nested_shape(*(*p).children, value, depth + 1)
                    }
                }
                13 => {
                    let mut found = false;
                    for i in 0..(*p).count as usize {
                        found |= self.nested_shape(*(*p).children.add(i), value, depth + 1)?;
                    }
                    Ok(found)
                }
                10 => {
                    if !self.shape(p, value, false, depth + 1)? {
                        return Ok(false);
                    }
                    let Kind::Object(values, _) = &value.kind else {
                        return Ok(false);
                    };
                    for (key, value) in values {
                        for i in 0..(*p).count as usize {
                            if self.same_key(key, *(*p).names.add(i))?
                                && !self.nested_shape(*(*p).children.add(i), value, depth + 1)?
                            {
                                return Ok(false);
                            }
                        }
                    }
                    Ok(true)
                }
                8 => {
                    let Kind::Array(values) = &value.kind else {
                        return Ok(false);
                    };
                    if values.len() != (*p).count as usize {
                        return Ok(false);
                    }
                    for (i, value) in values.iter().enumerate() {
                        if !self.nested_shape(*(*p).children.add(i), value, depth + 1)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                12 => {
                    if !self.shape(p, value, false, depth + 1)? {
                        return Ok(false);
                    }
                    let Kind::Object(values, _) = &value.kind else {
                        return Ok(false);
                    };
                    let (mut tag, mut fields) = (None, None);
                    for (key, value) in values {
                        if self.same_key(key, c"tag".as_ptr())? {
                            tag = Some(value);
                        }
                        if self.same_key(key, c"fields".as_ptr())? {
                            fields = Some(value);
                        }
                    }
                    let (Some(tag), Some(fields)) = (tag, fields) else {
                        return Ok(false);
                    };
                    let (Kind::String(_), Kind::Array(values)) = (&tag.kind, &fields.kind) else {
                        return Ok(false);
                    };
                    for i in 0..(*p).count as usize {
                        let variant = &*(*p).children.cast::<Variant>().add(i);
                        if !self.same_key(tag, variant.name)? {
                            continue;
                        }
                        if variant.count as usize != values.len() {
                            return Ok(false);
                        }
                        for (i, value) in values.iter().enumerate() {
                            if !self.nested_shape(*variant.children.add(i), value, depth + 1)? {
                                return Ok(false);
                            }
                        }
                        return Ok(true);
                    }
                    Ok(false)
                }
                _ => self.shape(p, value, true, depth + 1),
            }
        }
    }
}
