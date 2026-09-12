//! Decoder-free union selection. Shallow profiles never allocate candidate payloads.
use super::*;
fn bit(v: &Json) -> u32 {
    match v.kind {
        Kind::Null => 1,
        Kind::Bool(_) => 2,
        Kind::Number(_) => 4,
        Kind::String(_) => 8,
        Kind::Array(_) => 16,
        Kind::Object(_, _) => 32,
    }
}
impl Execution<'_> {
    unsafe fn profile(&mut self, p: *const Codec, depth: usize, sums: bool) -> Result<u32> {
        unsafe {
            self.step(depth)?;
            match (*p).kind {
                11 => self.profile(*(*p).children, depth + 1, sums),
                7 => Ok(if sums { 0 } else { 1 } | self.profile(*(*p).children, depth + 1, sums)?),
                13 => {
                    let mut mask = 0;
                    for i in 0..(*p).count as usize {
                        mask |= self.profile(*(*p).children.add(i), depth + 1, sums)?;
                    }
                    Ok(mask)
                }
                kind => Ok(if sums {
                    match kind {
                        12 => 1,
                        9 | 10 | 5 => 2,
                        _ => 0,
                    }
                } else {
                    match kind {
                        0 | 1 => 4,
                        2 => 2,
                        3 => 8,
                        4 => 1,
                        5 => 63,
                        6 | 8 => 16,
                        9 | 10 | 12 => 32,
                        _ => 0,
                    }
                }),
            }
        }
    }
    unsafe fn shape(
        &mut self,
        p: *const Codec,
        v: &Json,
        tags: bool,
        depth: usize,
    ) -> Result<bool> {
        unsafe {
            self.step(depth)?;
            match (*p).kind {
                11 => self.shape(*(*p).children, v, tags, depth + 1),
                7 => {
                    if matches!(v.kind, Kind::Null) {
                        Ok(true)
                    } else {
                        self.shape(*(*p).children, v, tags, depth + 1)
                    }
                }
                13 => {
                    let mut found = false;
                    for i in 0..(*p).count as usize {
                        found |= self.shape(*(*p).children.add(i), v, tags, depth + 1)?;
                    }
                    Ok(found)
                }
                12 => {
                    let Kind::Object(members, _) = &v.kind else {
                        return Ok(false);
                    };
                    if tags {
                        for (key, value) in members {
                            if self.same_key(key, c"tag".as_ptr())? {
                                if !matches!(value.kind, Kind::String(_)) {
                                    return Ok(false);
                                }
                                let mut found = false;
                                for i in 0..(*p).count as usize {
                                    found |= self.same_key(
                                        value,
                                        (*(*p).children.cast::<Variant>().add(i)).name,
                                    )?;
                                }
                                return Ok(found);
                            }
                        }
                        Ok(false)
                    } else {
                        let mut tag = false;
                        let mut fields = false;
                        for (key, _) in members {
                            let a = self.same_key(key, c"tag".as_ptr())?;
                            let b = self.same_key(key, c"fields".as_ptr())?;
                            if !a && !b {
                                return Ok(false);
                            }
                            tag |= a;
                            fields |= b;
                        }
                        Ok(tag && fields)
                    }
                }
                10 => {
                    let Kind::Object(members, _) = &v.kind else {
                        return Ok(false);
                    };
                    for (key, _) in members {
                        let mut known = false;
                        for i in 0..(*p).count as usize {
                            known |= self.same_key(key, *(*p).names.add(i))?;
                        }
                        if !known {
                            return Ok(false);
                        }
                    }
                    for i in 0..(*p).count as usize {
                        self.budget.work(1)?;
                        if (**(*p).children.add(i)).kind == 7 {
                            continue;
                        }
                        let mut found = false;
                        for (key, _) in members {
                            found |= self.same_key(key, *(*p).names.add(i))?;
                        }
                        if !found {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                8 => Ok(matches!(&v.kind,Kind::Array(values)if values.len()==(*p).count as usize)),
                _ => Ok(self.profile(p, depth + 1, false)? & bit(v) != 0),
            }
        }
    }
    pub(super) unsafe fn union_select(&mut self, p: *const Codec, v: &Json) -> Result<usize> {
        unsafe {
            let bit = bit(v);
            let mut count = 0;
            let mut selected = 0;
            for i in 0..(*p).count as usize {
                if self.profile(*(*p).children.add(i), 0, false)? & bit != 0 {
                    count += 1;
                    selected = i;
                }
            }
            if count == 1 {
                return Ok(selected);
            }
            if count == 0 {
                return Err(error(14, -1));
            }
            let mut sums = bit == 32;
            if sums {
                for i in 0..(*p).count as usize {
                    let child = *(*p).children.add(i);
                    if self.profile(child, 0, false)? & bit != 0
                        && self.profile(child, 0, true)? != 1
                    {
                        sums = false;
                    }
                }
            }
            count = 0;
            for i in 0..(*p).count as usize {
                let child = *(*p).children.add(i);
                if self.profile(child, 0, false)? & bit != 0 && self.shape(child, v, sums, 0)? {
                    count += 1;
                    selected = i;
                }
            }
            if count == 1 {
                Ok(selected)
            } else {
                Err(error(14, -1))
            }
        }
    }
}
