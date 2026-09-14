//! Ordered arrays, strict records, and String-key native map adapters.
use super::*;
impl Execution<'_> {
    pub(super) unsafe fn encode_array(
        &mut self,
        p: *const Codec,
        bits: i64,
        depth: usize,
    ) -> Result<Json> {
        unsafe {
            let (data, count) = if (*p).kind == 6 {
                let list = &*(bits as *const abi::List);
                (list.data as *const i64, list.len as usize)
            } else {
                ((bits as *const i64).add(1), (*p).count as usize)
            };
            if count > NODES {
                return Err(error(4, -1));
            }
            self.budget.work(count)?;
            self.budget.node()?;
            self.budget.allocate(count * 8)?;
            let mut values = Vec::with_capacity(count);
            for i in 0..count {
                let index = self.index(i)?;
                let child = *(*p).children.add(if (*p).kind == 6 { 0 } else { i });
                values.push(self.at(&index, |c| c.encode(child, *data.add(i), depth + 1))?);
            }
            morrow_json::seal(values, false, 0, &mut self.budget)
        }
    }
    pub(super) unsafe fn encode_object(
        &mut self,
        p: *const Codec,
        bits: i64,
        depth: usize,
    ) -> Result<Json> {
        unsafe {
            let map = if (*p).kind == 9 {
                Some(&*(bits as *const abi::List))
            } else {
                None
            };
            let count = map.map_or((*p).count as usize, |m| m.len as usize);
            if count > NODES / 2 {
                return Err(error(4, -1));
            }
            self.budget.work(count * 2)?;
            self.budget.node()?;
            self.budget.allocate(count * 16)?;
            let mut children = Vec::with_capacity(count * 2);
            for i in 0..count {
                let (key, value) = if let Some(map) = map {
                    let pair = *map.data.add(i) as *const i64;
                    (*pair as *const c_char, *pair.add(1))
                } else {
                    (*(*p).names.add(i), *(bits as *const i64).add(i + 1))
                };
                let key_node = self.text(key)?;
                let Kind::String(name) = &key_node.kind else {
                    unreachable!()
                };
                let encoded = self.at(name, |c| {
                    c.encode(
                        *(*p).children.add(if map.is_some() { 0 } else { i }),
                        value,
                        depth + 1,
                    )
                })?;
                children.push(key_node);
                children.push(encoded);
            }
            morrow_json::seal(children, true, 0, &mut self.budget)
        }
    }
    pub(super) unsafe fn decode_array(
        &mut self,
        p: *const Codec,
        v: &Json,
        depth: usize,
    ) -> Result<i64> {
        unsafe {
            let Kind::Array(values) = &v.kind else {
                return Err(error(5, -1));
            };
            let list = (*p).kind == 6;
            if !list && values.len() != (*p).count as usize {
                return Err(error(5, -1));
            }
            let out = self.slots(values.len(), list)?;
            let _out_root = ConstructionRoot::new(out as usize);
            let data = out.add(if list { 3 } else { 1 });
            if list {
                out.cast::<abi::List>().write(abi::List {
                    data,
                    len: values.len() as i64,
                    cap: values.len().max(1) as i64,
                });
            }
            for (i, value) in values.iter().enumerate() {
                let key = self.index(i)?;
                *data.add(i) = self.at(&key, |c| {
                    c.decode(
                        *(*p).children.add(if list { 0 } else { i }),
                        value,
                        depth + 1,
                    )
                })?;
            }
            Ok(out as i64)
        }
    }
    pub(super) unsafe fn known_fields(
        &mut self,
        p: *const Codec,
        members: &[(Json, Json)],
    ) -> Result<()> {
        unsafe {
            for (key, _) in members {
                let Kind::String(name) = &key.kind else {
                    return Err(error(5, -1));
                };
                self.budget.work(name.len())?;
                if name.contains('\0') {
                    return Err(error(10, -1));
                }
                let mut known = false;
                for i in 0..(*p).count as usize {
                    known = self.same_key(key, *(*p).names.add(i))? || known;
                }
                if !known {
                    return self.at(name, |_| Err(error(12, -1)));
                }
            }
            Ok(())
        }
    }
    pub(super) unsafe fn decode_record(
        &mut self,
        p: *const Codec,
        v: &Json,
        depth: usize,
    ) -> Result<i64> {
        unsafe {
            let Kind::Object(members, _) = &v.kind else {
                return Err(error(5, -1));
            };
            self.known_fields(p, members)?;
            let fields = self.slots((*p).count as usize, false)?;
            let _fields_root = ConstructionRoot::new(fields as usize);
            for i in 0..(*p).count as usize {
                let mut found = None;
                for (key, value) in members {
                    if self.same_key(key, *(*p).names.add(i))? {
                        found = Some(value);
                        break;
                    }
                }
                let name = self.name(*(*p).names.add(i))?;
                let child = *(*p).children.add(i);
                *fields.add(i + 1) = self.at(name, |c| {
                    if let Some(v) = found {
                        c.decode(child, v, depth + 1)
                    } else if (*child).kind == 7 {
                        let none = c.slots(1, false)?;
                        *none = 1;
                        Ok(none as i64)
                    } else {
                        Err(error(6, -1))
                    }
                })?;
            }
            Ok(fields as i64)
        }
    }
    pub(super) unsafe fn decode_map(
        &mut self,
        p: *const Codec,
        v: &Json,
        depth: usize,
    ) -> Result<i64> {
        unsafe {
            let Kind::Object(members, _) = &v.kind else {
                return Err(error(5, -1));
            };
            let out = self.slots(members.len(), true)?;
            let _out_root = ConstructionRoot::new(out as usize);
            let data = out.add(3);
            out.cast::<abi::List>().write(abi::List {
                data,
                len: members.len() as i64,
                cap: members.len().max(1) as i64,
            });
            for (i, (key, value)) in members.iter().enumerate() {
                let Kind::String(name) = &key.kind else {
                    return Err(error(5, -1));
                };
                self.budget.work(name.len())?;
                self.budget.allocate(64)?;
                if name.contains('\0') {
                    return Err(error(10, -1));
                }
                self.checkpoint()?;
                let text = self.allocated(abi::string(name) as *mut c_char);
                let _text_root = ConstructionRoot::new(text as usize);
                self.budget.allocate(16)?;
                self.checkpoint()?;
                let pair = self.allocated(memory::alloc(16, false).cast::<i64>());
                let _pair_root = ConstructionRoot::new(pair as usize);
                *pair = text as i64;
                *pair.add(1) = self.at(name, |c| c.decode(*(*p).children, value, depth + 1))?;
                *data.add(i) = pair as i64;
            }
            Ok(out as i64)
        }
    }
}
