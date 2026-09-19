//! Bounded semantic ownership accounting; never infers graph shape from pointer width.
use super::*;
#[path = "cost_scratch.rs"]
mod scratch;
use scratch::Scratch;
pub(super) fn work(work: &mut usize) -> bool {
    *work += 1;
    *work <= WORK
}

// SAFETY: all descriptors/values originate in the validated compiler ABI. Bounds
// reject malformed shape before following its declared child/payload arrays.
pub(super) unsafe fn descriptor(root: *const Type, nullable: bool, work: &mut usize) -> bool {
    unsafe {
        if !self::work(work) {
            return false;
        }
        if root.is_null() {
            return nullable;
        }
        let mut pending = Scratch::default();
        pending.push(root);
        let mut seen = Scratch::default();
        while let Some(ty) = pending.pop() {
            if ty.is_null() || !self::work(work) {
                return false;
            }
            let mut known = false;
            for &prior in seen.iter() {
                if !self::work(work) {
                    return false;
                }
                if prior == ty {
                    known = true;
                    break;
                }
            }
            if known {
                continue;
            }
            if seen.len() >= 4096
                || !(0..=TYPE_CHILD_SPEC).contains(&(*ty).kind)
                || !(0..=4096).contains(&(*ty).count)
            {
                return false;
            }
            seen.push(ty);
            let mut children = (*ty).count as usize;
            match (*ty).kind {
                2 | 5 | 6 | TYPE_CHILD_KEY => {
                    if children != 1 {
                        return false;
                    }
                }
                9 => {
                    if children != 2 {
                        return false;
                    }
                }
                4 => {
                    if children == 0 || (*ty).arities.is_null() {
                        return false;
                    }
                    let mut total = 0;
                    for i in 0..children {
                        if !self::work(work) {
                            return false;
                        }
                        let arity = *(*ty).arities.add(i);
                        if !(0..=4096).contains(&arity) || total > 4096 - arity as usize {
                            return false;
                        }
                        total += arity as usize;
                    }
                    children = total;
                }
                3 => (),
                _ => {
                    if children != 0 {
                        return false;
                    }
                }
            }
            if children > 4096 - pending.len() || (children != 0 && (*ty).children.is_null()) {
                return false;
            }
            for i in 0..children {
                if !self::work(work) {
                    return false;
                }
                pending.push(*(*ty).children.add(i));
            }
        }
        true
    }
}
struct Cost {
    session: *mut Session,
    seen: Scratch<(*const c_void, *const Type, bool)>,
    work: usize,
    bytes: usize,
}
impl Cost {
    fn add(&mut self, bytes: usize) -> bool {
        if !work(&mut self.work) || bytes > BYTES - self.bytes {
            false
        } else {
            self.bytes += bytes;
            true
        }
    }
    unsafe fn supervisor_name(&mut self, name: *const std::ffi::c_char) -> bool {
        let Some(bytes) = (unsafe { supervisor::values::name_bytes(name) }) else {
            return false;
        };
        for _ in 0..bytes.div_ceil(64) {
            if !work(&mut self.work) {
                return false;
            }
        }
        self.add(bytes)
    }
    unsafe fn supervisor_key(
        &mut self,
        key: *const supervisor::values::ChildKey,
        mailbox: *const Type,
    ) -> bool {
        unsafe {
            supervisor::values::valid_key(self.session, key, mailbox)
                && self.add(
                    std::mem::size_of::<supervisor::values::ChildKey>()
                        + std::mem::size_of::<supervisor::values::KeyToken>(),
                )
                && self.supervisor_name((*key).name)
        }
    }
    unsafe fn supervisor_spec(
        &mut self,
        ty: *const Type,
        pointer: *const supervisor::values::ChildSpec,
        depth: usize,
    ) -> bool {
        unsafe {
            if supervisor::values::validate_header(self.session, pointer, &mut self.work).is_err()
                || !self.add(std::mem::size_of::<supervisor::values::ChildSpec>())
            {
                return false;
            }
            let v = &*pointer;
            if v.kind == 0 {
                self.supervisor_key(v.key, (*(*v.key).token).mailbox)
                    && self.frame(v.initializer, depth + 1)
            } else {
                if !self.supervisor_name(v.name) || !self.add(v.children_len as usize * 8) {
                    return false;
                }
                for i in 0..v.children_len as usize {
                    if !self.value(ty, *v.children.add(i) as i64, depth + 1) {
                        return false;
                    }
                }
                true
            }
        }
    }
    unsafe fn frame(&mut self, closure: *const c_void, depth: usize) -> bool {
        unsafe {
            if depth >= 128 {
                return false;
            }
            let f = function_work(self.session, closure, &mut self.work);
            if f.is_null() || !self.add(8 * (1 + (*f).capture_count as usize)) {
                return false;
            }
            for i in 0..(*f).capture_count as usize {
                if !self.value(
                    *(*f).captures.add(i),
                    *closure.cast::<i64>().add(i + 1),
                    depth + 1,
                ) {
                    return false;
                }
            }
            true
        }
    }
    unsafe fn list(&mut self, ty: *const Type, list: *const abi::List, depth: usize) -> bool {
        unsafe {
            if (*list).len < 0
                || (*list).cap < 1
                || (*list).len > (*list).cap
                || (*list).cap as u64 > (BYTES / 8) as u64
                || (*list).data.is_null()
            {
                return false;
            }
            if !self.add(24 + 8 * (*list).cap as usize) {
                return false;
            }
            for i in 0..(*list).len as usize {
                let value = *(*list).data.add(i);
                if (*ty).kind == 9 {
                    let pair = value as *const i64;
                    if pair.is_null() || !self.add(16) {
                        return false;
                    }
                    if !self.value(*(*ty).children, *pair, depth + 1)
                        || !self.value(*(*ty).children.add(1), *pair.add(1), depth + 1)
                    {
                        return false;
                    }
                } else if !self.value(*(*ty).children, value, depth + 1) {
                    return false;
                }
            }
            true
        }
    }
    unsafe fn fields(&mut self, ty: *const Type, fields: *const i64, depth: usize) -> bool {
        unsafe {
            let tag = *fields;
            let mut first = 0;
            let mut count = (*ty).count as usize;
            if (*ty).kind == 4 {
                if tag < 0 || tag >= (*ty).count {
                    return false;
                }
                for i in 0..tag as usize {
                    if !work(&mut self.work) {
                        return false;
                    }
                    first += *(*ty).arities.add(i) as usize;
                }
                count = *(*ty).arities.add(tag as usize) as usize;
            } else if tag != 0 {
                return false;
            }
            if !self.add(8 * (count + 1)) {
                return false;
            }
            for i in 0..count {
                if !self.value(
                    *(*ty).children.add(first + i),
                    *fields.add(i + 1),
                    depth + 1,
                ) {
                    return false;
                }
            }
            true
        }
    }
    unsafe fn value(&mut self, ty: *const Type, value: i64, depth: usize) -> bool {
        unsafe {
            if depth >= 128 || ty.is_null() || !work(&mut self.work) {
                return false;
            }
            if (*ty).kind == 0 {
                return self.add(8);
            }
            if (*ty).kind == 5 {
                return self.value(*(*ty).children, value, depth + 1);
            }
            let pointer = value as *const c_void;
            if pointer.is_null() {
                return false;
            }
            for &(prior, prior_type, active) in self.seen.iter() {
                if !work(&mut self.work) {
                    return false;
                }
                if prior == pointer && prior_type == ty {
                    return !active;
                }
            }
            if self.seen.len() >= 4096 {
                return false;
            }
            let index = self.seen.len();
            self.seen.push((pointer, ty, true));
            let valid = match (*ty).kind {
                1 => {
                    let mut found = false;
                    for length in 0..(16 * 1024 * 1024 + 1).min(BYTES - self.bytes) {
                        if length % 64 == 0 && !work(&mut self.work) {
                            break;
                        }
                        if *pointer.cast::<u8>().add(length) == 0 {
                            found = self.add(length + 1);
                            break;
                        }
                    }
                    found
                }
                2 | 9 => self.list(ty, pointer.cast(), depth),
                3 | 4 => self.fields(ty, pointer.cast(), depth),
                6 => {
                    let pid = pointer.cast::<Pid>();
                    let s = self.session;
                    valid_pid(s, pid)
                        && (*pid).mailbox == *(*ty).children
                        && self.add(std::mem::size_of::<Pid>())
                }
                TYPE_SUPERVISOR_HANDLE => {
                    supervisor::values::valid_handle(self.session, pointer.cast())
                        && self.add(std::mem::size_of::<supervisor::values::Handle>())
                }
                TYPE_CHILD_KEY => self.supervisor_key(pointer.cast(), *(*ty).children),
                TYPE_CHILD_SPEC => self.supervisor_spec(ty, pointer.cast(), depth),
                TYPE_PROCESS_ID => {
                    process::valid_identity(self.session, pointer.cast())
                        && self.add(std::mem::size_of::<process::Identity>())
                }
                TYPE_MONITOR_REF => {
                    relations::valid(self.session, pointer.cast())
                        && self.add(std::mem::size_of::<relations::Reference>())
                }
                7 => self.frame(pointer, depth),
                TYPE_RANGE => self.add(24),
                TYPE_JSON_VALUE => {
                    let node = crate::json::node(pointer.cast());
                    if node.nodes > WORK - self.work {
                        false
                    } else {
                        self.work += node.nodes;
                        self.add(
                            std::mem::size_of::<crate::json::NativeJson>()
                                + morrow_json::retained_bytes(&node),
                        )
                    }
                }
                _ => false,
            };
            self.seen[index].2 = false;
            valid
        }
    }
}
pub(super) unsafe fn frame(s: *mut Session, closure: *const c_void) -> Option<usize> {
    let mut cost = Cost {
        session: s,
        seen: Scratch::default(),
        work: 0,
        bytes: 0,
    };
    unsafe { cost.frame(closure, 0).then_some(cost.bytes) }
}
pub(super) unsafe fn value(s: *mut Session, ty: *const Type, value: i64) -> Option<usize> {
    let mut cost = Cost {
        session: s,
        seen: Scratch::default(),
        work: 0,
        bytes: 0,
    };
    unsafe {
        (descriptor(ty, false, &mut cost.work) && cost.value(ty, value, 0)).then_some(cost.bytes)
    }
}

/// Start one graph accounting pass for an engine-owned opaque template.
pub(super) unsafe fn child_spec(
    s: *mut Session,
    spec: *const supervisor::values::ChildSpec,
) -> Option<usize> {
    let ty = Type {
        kind: TYPE_CHILD_SPEC,
        count: 0,
        children: null(),
        arities: null(),
    };
    unsafe { value(s, &ty, spec as i64) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_graph_validation_preserves_cost_and_shared_value_accounting() {
        let string = Type {
            kind: 1,
            count: 0,
            children: null(),
            arities: null(),
        };
        let children = [&string as *const Type; 32];
        let tuple = Type {
            kind: 3,
            count: children.len() as i64,
            children: children.as_ptr(),
            arities: null(),
        };
        let strings = [[b'a', 0]; 32];
        let mut fields = [0i64; 33];
        for (field, text) in fields[1..].iter_mut().zip(&strings) {
            *field = text.as_ptr() as i64;
        }
        let mut session = Session::default();
        let mut work = 0;
        // SAFETY: all descriptors and distinct string/tuple payloads are
        // initialized stack storage retained until both validations complete.
        unsafe {
            assert!(descriptor(&tuple, false, &mut work));
            assert_eq!(work, 129, "descriptor traversal charges are unchanged");
            assert_eq!(
                value(&mut session, &tuple, fields.as_ptr() as i64),
                Some(328)
            );
            fields[17] = fields[1];
            assert_eq!(
                value(&mut session, &tuple, fields.as_ptr() as i64),
                Some(326)
            );
        }
    }
}

#[cfg(test)]
#[path = "supervisor_value_tests.rs"]
mod supervisor_value_tests;
