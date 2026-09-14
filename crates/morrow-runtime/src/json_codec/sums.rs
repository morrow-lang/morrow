//! Source-named sum envelopes with strict tag/fields validation and preserved paths.
use super::*;
impl Execution<'_> {
    pub(super) unsafe fn encode_sum(
        &mut self,
        p: *const Codec,
        bits: i64,
        depth: usize,
    ) -> Result<Json> {
        unsafe {
            let data = bits as *const i64;
            let tag = *data;
            if tag < 0 || tag >= (*p).count {
                return self.at("tag", |_| Err(error(13, -1)));
            }
            let variant = &*(*p).children.cast::<Variant>().add(tag as usize);
            self.budget.work(4)?;
            self.budget.node()?;
            self.budget.allocate(32)?;
            let tag_key = self.text(c"tag".as_ptr())?;
            let tag_value = self.at("tag", |c| {
                c.step(depth + 1)?;
                c.text(variant.name)
            })?;
            let fields_key = self.text(c"fields".as_ptr())?;
            let fields = self.at("fields", |c| {
                c.step(depth + 1)?;
                let count = variant.count as usize;
                c.budget.work(count)?;
                c.budget.node()?;
                c.budget.allocate(count * 8)?;
                let mut values = Vec::with_capacity(count);
                for i in 0..count {
                    let index = c.index(i)?;
                    values.push(c.at(&index, |c| {
                        c.encode(*variant.children.add(i), *data.add(i + 1), depth + 2)
                    })?);
                }
                morrow_json::seal(values, false, 0, &mut c.budget)
            })?;
            morrow_json::seal(
                vec![tag_key, tag_value, fields_key, fields],
                true,
                0,
                &mut self.budget,
            )
        }
    }
    pub(super) unsafe fn decode_sum(
        &mut self,
        p: *const Codec,
        v: &Json,
        depth: usize,
    ) -> Result<i64> {
        unsafe {
            let Kind::Object(members, _) = &v.kind else {
                return Err(error(5, -1));
            };
            let mut tag = None;
            let mut fields = None;
            for (key, value) in members {
                let Kind::String(name) = &key.kind else {
                    return Err(error(5, -1));
                };
                self.budget.work(name.len())?;
                if name.contains('\0') {
                    return Err(error(10, -1));
                }
                let is_tag = self.same_key(key, c"tag".as_ptr())?;
                let is_fields = self.same_key(key, c"fields".as_ptr())?;
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
            let selected = self.at("tag", |c| {
                c.step(depth + 1)?;
                if !matches!(tag.kind, Kind::String(_)) {
                    return Err(error(5, -1));
                }
                for i in 0..(*p).count as usize {
                    let variant = &*(*p).children.cast::<Variant>().add(i);
                    if c.same_key(tag, variant.name)? {
                        return Ok(i);
                    }
                }
                Err(error(13, -1))
            })?;
            self.at("fields", |c| {
                c.step(depth + 1)?;
                let variant = &*(*p).children.cast::<Variant>().add(selected);
                let Kind::Array(values) = &fields.kind else {
                    return Err(error(5, -1));
                };
                if values.len() != variant.count as usize {
                    return Err(error(5, -1));
                }
                let out = c.slots(values.len(), false)?;
                let _out_root = ConstructionRoot::new(out as usize);
                *out = selected as i64;
                for (i, value) in values.iter().enumerate() {
                    let index = c.index(i)?;
                    *out.add(i + 1) = c.at(&index, |c| {
                        c.decode(*variant.children.add(i), value, depth + 2)
                    })?;
                }
                Ok(out as i64)
            })
        }
    }
}
