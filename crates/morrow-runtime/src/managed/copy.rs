//! Copy preflight-validated native graphs without retaining sender payload storage.
//!
//! Heap copies use temporary roots until publication. Fragment copies keep the
//! source heap current and own every copied block until receiver adoption.
use super::*;
use std::collections::HashMap;
use std::rc::Rc;

struct Rooted {
    // Drop the registration before freeing its stable word.
    _root: memory::Root,
    _word: Box<usize>,
}
impl Rooted {
    fn new(value: usize) -> Self {
        let word = Box::new(value);
        // SAFETY: Box has a stable address; field drop order retires the root first.
        let root = unsafe { memory::root_range(&*word, 1) };
        Self {
            _root: root,
            _word: word,
        }
    }
}

enum Destination {
    Heap,
    Fragment(memory::Fragment),
}

/// Transferable copied graph. All JSON memoization references and source roots
/// are destroyed before construction, leaving exclusively owned payload storage.
pub(super) struct FragmentCopy {
    pub value: i64,
    fragment: memory::Fragment,
}
impl FragmentCopy {
    /// Adopt into the current heap; publish the returned root before a safepoint.
    pub(super) fn adopt(self) -> i64 {
        self.fragment.adopt();
        self.value
    }
}

pub(super) struct Copy {
    pub value: i64,
    session: *mut Session,
    roots: Vec<Rooted>,
    seen: HashMap<(i64, usize), i64>,
    json: HashMap<usize, morrow_json::Json>,
    destination: Destination,
}
impl Copy {
    fn new(session: *mut Session, source: i64) -> Self {
        Self {
            value: 0,
            session,
            roots: vec![Rooted::new(source as usize)],
            seen: HashMap::new(),
            json: HashMap::new(),
            destination: Destination::Heap,
        }
    }
    fn fragment(session: *mut Session) -> Self {
        Self {
            value: 0,
            session,
            roots: Vec::new(),
            seen: HashMap::new(),
            json: HashMap::new(),
            destination: Destination::Fragment(memory::Fragment::new()),
        }
    }
    fn finish_fragment(self) -> FragmentCopy {
        let Destination::Fragment(fragment) = self.destination else {
            unreachable!("only detached copies can be transferred");
        };
        // Other fields, particularly the JSON Rc memoization map, drop here on
        // the sender. No Rc ownership is shared across the transfer boundary.
        FragmentCopy {
            value: self.value,
            fragment,
        }
    }
    fn allocate(&mut self, bytes: usize, atomic: bool) -> *mut u8 {
        match &mut self.destination {
            Destination::Heap => {
                let value = memory::alloc(bytes, atomic);
                self.roots.push(Rooted::new(value as usize));
                value
            }
            Destination::Fragment(fragment) => fragment.allocate(bytes, atomic),
        }
    }
    fn wrap_json(&mut self, node: morrow_json::Json) -> *mut crate::json::NativeJson {
        match &mut self.destination {
            Destination::Heap => {
                let target = crate::json::wrap(node);
                self.roots.push(Rooted::new(target as usize));
                target
            }
            Destination::Fragment(fragment) => {
                let retained = morrow_json::retained_bytes(&node);
                // SAFETY: this graph is deep-copied, contains no GC pointers and
                // its only additional Rc owners are this Copy's temporary map.
                // finish_fragment drops that map before permitting transfer.
                unsafe { fragment.managed(crate::json::NativeJson(node), retained) }
            }
        }
    }
    unsafe fn retain_pid(&mut self, target: *mut Pid) {
        // SAFETY: the fresh wrapper belongs to the selected destination; the
        // copied PID's actor is live retained control storage, never a GC value.
        unsafe {
            let retention = control::Owned::retain((*target).actor).token();
            match &mut self.destination {
                Destination::Heap => memory::retain_control(target.cast(), retention),
                Destination::Fragment(fragment) => {
                    fragment.retain_control(target.cast(), retention)
                }
            }
        }
    }
    // SAFETY: only reached after cost preflight validates graph shape/depth/size.
    // Descriptors and source graphs remain immutable throughout this synchronous copy.
    unsafe fn frame(&mut self, source: *const c_void) -> i64 {
        unsafe {
            let key = (source as i64, 0);
            if let Some(&copy) = self.seen.get(&key) {
                return copy;
            }
            let f = function(self.session, source);
            let target = self
                .allocate(8 * (1 + (*f).capture_count as usize), false)
                .cast::<i64>();
            self.seen.insert(key, target as i64);
            *target = *source.cast::<i64>();
            for i in 0..(*f).capture_count as usize {
                *target.add(i + 1) =
                    self.value(*(*f).captures.add(i), *source.cast::<i64>().add(i + 1));
            }
            target as i64
        }
    }
    unsafe fn value(&mut self, ty: *const Type, source: i64) -> i64 {
        unsafe {
            if (*ty).kind == 0 {
                return source;
            }
            if (*ty).kind == 5 {
                return self.value(*(*ty).children, source);
            }
            if (*ty).kind == 7 {
                return self.frame(source as *const c_void);
            }
            let key = (source, ty as usize);
            if let Some(&copy) = self.seen.get(&key) {
                return copy;
            }
            let copied = match (*ty).kind {
                1 => {
                    let bytes = std::ffi::CStr::from_ptr(source as *const _).to_bytes_with_nul();
                    let target = self.allocate(bytes.len(), true);
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
                    target as i64
                }
                2 | 9 => {
                    let list = &*(source as *const abi::List);
                    let target = self.allocate(24, false).cast::<abi::List>();
                    self.seen.insert(key, target as i64);
                    let data = self.allocate(8 * list.cap as usize, false).cast::<i64>();
                    *target = abi::List {
                        len: list.len,
                        cap: list.cap,
                        data,
                    };
                    for i in 0..list.len as usize {
                        let value = *list.data.add(i);
                        *data.add(i) = if (*ty).kind == 9 {
                            let pair = value as *const i64;
                            let pair_key = (value, ty as usize);
                            if let Some(&prior) = self.seen.get(&pair_key) {
                                prior
                            } else {
                                // Native Map entries are untagged [key, value]
                                // pairs, distinct from source tuple records.
                                let copied = self.allocate(16, false).cast::<i64>();
                                *copied = self.value(*(*ty).children, *pair);
                                *copied.add(1) = self.value(*(*ty).children.add(1), *pair.add(1));
                                self.seen.insert(pair_key, copied as i64);
                                copied as i64
                            }
                        } else {
                            self.value(*(*ty).children, value)
                        };
                    }
                    target as i64
                }
                3 | 4 => {
                    let fields = source as *const i64;
                    let tag = *fields;
                    let (first, count) = if (*ty).kind == 4 {
                        let first = (0..tag as usize)
                            .map(|i| *(*ty).arities.add(i) as usize)
                            .sum::<usize>();
                        (first, *(*ty).arities.add(tag as usize) as usize)
                    } else {
                        (0, (*ty).count as usize)
                    };
                    let target = self.allocate(8 * (count + 1), false).cast::<i64>();
                    self.seen.insert(key, target as i64);
                    *target = tag;
                    for i in 0..count {
                        *target.add(i + 1) =
                            self.value(*(*ty).children.add(first + i), *fields.add(i + 1));
                    }
                    target as i64
                }
                6 => {
                    // PIDs retain scheduler identity, never another actor's payload graph.
                    let target = self
                        .allocate(std::mem::size_of::<Pid>(), false)
                        .cast::<Pid>();
                    std::ptr::copy_nonoverlapping(source as *const Pid, target, 1);
                    self.retain_pid(target);
                    target as i64
                }
                TYPE_RANGE => {
                    let target = self.allocate(24, true).cast::<i64>();
                    std::ptr::copy_nonoverlapping(source as *const i64, target, 3);
                    target as i64
                }
                TYPE_JSON_VALUE => {
                    let original = crate::json::node(source as *const crate::json::NativeJson);
                    let node = self.json(&original);
                    let target = self.wrap_json(node);
                    target as i64
                }
                _ => unreachable!("copy requires successful descriptor and graph preflight"),
            };
            self.seen.insert(key, copied);
            copied
        }
    }
    fn json(&mut self, source: &morrow_json::Json) -> morrow_json::Json {
        use morrow_json::Kind;
        let key = Rc::as_ptr(source) as usize;
        if let Some(copy) = self.json.get(&key) {
            return copy.clone();
        }
        // Sealed JSON bounds depth and expanded nodes. Preserve metadata and DAG
        // sharing without carrying any sender-owned Rc into the copied graph.
        let kind = match &source.kind {
            Kind::Null => Kind::Null,
            Kind::Bool(value) => Kind::Bool(*value),
            Kind::Number(value) => Kind::Number(value.clone()),
            Kind::String(value) => Kind::String(value.clone()),
            Kind::Array(values) => Kind::Array(values.iter().map(|v| self.json(v)).collect()),
            Kind::Object(values, index) => Kind::Object(
                values
                    .iter()
                    .map(|(k, v)| (self.json(k), self.json(v)))
                    .collect(),
                index.clone(),
            ),
        };
        let copy = Rc::new(morrow_json::Node {
            kind,
            offset: source.offset,
            height: source.height,
            nodes: source.nodes,
            encoded: source.encoded,
        });
        self.json.insert(key, copy.clone());
        copy
    }
}

/// Caller must preflight the immutable graph with cost::frame before copying.
pub(super) unsafe fn frame(session: *mut Session, source: *const c_void) -> Copy {
    let mut copy = Copy::new(session, source as i64);
    copy.value = unsafe { copy.frame(source) };
    copy
}
/// Caller must preflight the immutable graph with cost::value before copying.
#[cfg(test)]
pub(super) unsafe fn value(session: *mut Session, ty: *const Type, source: i64) -> Copy {
    let mut copy = Copy::new(session, source);
    copy.value = unsafe { copy.value(ty, source) };
    copy
}

/// Copy into detached storage while the source heap remains current.
/// # Safety
/// The immutable graph and descriptors remain live and cost::value has succeeded.
/// The caller retains the session's logical charge until delivery or discard.
pub(super) unsafe fn value_fragment(
    session: *mut Session,
    ty: *const Type,
    source: i64,
) -> FragmentCopy {
    let mut copy = Copy::fragment(session);
    copy.value = unsafe { copy.value(ty, source) };
    copy.finish_fragment()
}

/// Copy a capture frame into independently transferable storage.
/// # Safety
/// The source frame/descriptors remain live and cost::frame has succeeded.
/// The caller retains the session's logical charge until adoption or discard.
pub(super) unsafe fn frame_fragment(session: *mut Session, source: *const c_void) -> FragmentCopy {
    let mut copy = Copy::fragment(session);
    copy.value = unsafe { copy.frame(source) };
    copy.finish_fragment()
}

#[cfg(test)]
mod fragment_tests {
    use super::*;

    fn ty(kind: i64, children: &[*const Type]) -> Type {
        Type {
            kind,
            count: children.len() as i64,
            children: children.as_ptr(),
            arities: null(),
        }
    }

    #[test]
    fn fragment_copies_full_width_graph_and_dag_after_source_collection() {
        let fragment = std::thread::spawn(|| unsafe {
            let scalar = ty(0, &[]);
            let string = ty(1, &[]);
            let list_children = [&string as *const Type];
            let list_type = ty(2, &list_children);
            let map_children = [&scalar as *const Type, &scalar];
            let map_type = ty(9, &map_children);
            let range_type = ty(TYPE_RANGE, &[]);
            let fields = [&list_type as *const Type, &map_type, &range_type];
            let tuple = ty(3, &fields);
            let text = abi::string("copied 🌿") as i64;
            let list = abi::list(&[text, text]);
            let pair = [i64::MAX, i64::MIN];
            let map = abi::list(&[pair.as_ptr() as i64, pair.as_ptr() as i64]);
            let range = [i64::MIN, i64::MAX, 1];
            let source = [0, list as i64, map as i64, range.as_ptr() as i64];
            assert!(cost::value(null_mut(), &tuple, source.as_ptr() as i64).is_some());
            let fragment = value_fragment(null_mut(), &tuple, source.as_ptr() as i64);
            memory::morrow_gc_collect_precise();
            assert!(!memory::heap_owns(0, list.cast()));
            assert!(!memory::heap_owns(0, map.cast()));
            fragment
        })
        .join()
        .unwrap();
        std::thread::spawn(move || unsafe {
            let address = fragment.value;
            let value = fragment.adopt();
            assert_eq!(value, address);
            let root_word = value as usize;
            let root = memory::root_range(&root_word, 1);
            memory::morrow_gc_collect_precise();
            let fields = value as *const i64;
            assert_eq!(*fields, 0);
            let list = &*(*fields.add(1) as *const abi::List);
            assert_eq!((list.len, list.cap), (2, 2));
            assert_eq!(*list.data, *list.data.add(1));
            assert_eq!(
                std::ffi::CStr::from_ptr(*list.data as *const _)
                    .to_str()
                    .unwrap(),
                "copied 🌿"
            );
            let map = &*(*fields.add(2) as *const abi::List);
            assert_eq!((map.len, map.cap), (2, 2));
            assert_eq!(*map.data, *map.data.add(1));
            let pair = *map.data as *const i64;
            assert_eq!((*pair, *pair.add(1)), (i64::MAX, i64::MIN));
            assert_eq!(
                std::slice::from_raw_parts(*fields.add(3) as *const i64, 3),
                [i64::MIN, i64::MAX, 1]
            );
            drop(root);
            memory::morrow_gc_collect_precise();
            assert_eq!(memory::stats().bytes, 0);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn fragment_json_clones_rc_nodes_and_preserves_internal_sharing() {
        let fragment = std::thread::spawn(|| unsafe {
            let json_type = ty(TYPE_JSON_VALUE, &[]);
            let children = [&json_type as *const Type, &json_type];
            let tuple = ty(3, &children);
            let first = crate::json::morrow_json_value_from_int(i64::MIN);
            let node = crate::json::node(first);
            let weak = Rc::downgrade(&node);
            let second = crate::json::wrap(node.clone());
            drop(node);
            let source = [0, first as i64, second as i64];
            assert!(cost::value(null_mut(), &tuple, source.as_ptr() as i64).is_some());
            let fragment = value_fragment(null_mut(), &tuple, source.as_ptr() as i64);
            memory::morrow_gc_collect_precise();
            assert!(
                weak.upgrade().is_none(),
                "fragment must not retain sender Rc"
            );
            fragment
        })
        .join()
        .unwrap();
        std::thread::spawn(move || unsafe {
            let value = fragment.adopt();
            let fields = value as *const i64;
            assert_ne!(*fields.add(1), *fields.add(2));
            let first = crate::json::node(*fields.add(1) as *const _);
            let second = crate::json::node(*fields.add(2) as *const _);
            assert!(Rc::ptr_eq(&first, &second));
            assert!(
                matches!(&first.kind, morrow_json::Kind::Number(n) if n == "-9223372036854775808")
            );
            let weak = Rc::downgrade(&first);
            drop(first);
            drop(second);
            memory::morrow_gc_collect_precise();
            assert!(
                weak.upgrade().is_none(),
                "receiver collection finalizes JSON"
            );
            assert_eq!(memory::stats().bytes, 0);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn fragment_capture_frame_copies_shared_captures() {
        unsafe extern "C" fn callback(_: *mut Exec, _: *mut c_void) -> i64 {
            2
        }
        let fragment = std::thread::spawn(|| unsafe {
            let string = ty(1, &[]);
            let scalar = ty(0, &[]);
            let captures = [&string as *const Type, &string, &scalar];
            let function = Function {
                identity: callback as *const c_void,
                step: Some(callback),
                select: None,
                capture_count: 3,
                captures: captures.as_ptr(),
                mailbox: &scalar,
            };
            let functions = [&function as *const Function];
            let mut session = Session {
                functions: functions.as_ptr(),
                function_count: 1,
                ..Session::default()
            };
            let text = abi::string("capture");
            let source = [
                callback as *const () as i64,
                text as i64,
                text as i64,
                i64::MIN,
            ];
            assert!(cost::frame(&mut session, source.as_ptr().cast()).is_some());
            let fragment = frame_fragment(&mut session, source.as_ptr().cast());
            memory::morrow_gc_collect_precise();
            fragment
        })
        .join()
        .unwrap();
        std::thread::spawn(move || unsafe {
            let value = fragment.adopt() as *const i64;
            assert_eq!(*value, callback as *const () as i64);
            assert_eq!(*value.add(1), *value.add(2));
            assert_eq!(*value.add(3), i64::MIN);
            assert_eq!(
                std::ffi::CStr::from_ptr(*value.add(1) as *const _).to_bytes(),
                b"capture"
            );
            memory::morrow_gc_collect_precise();
            assert_eq!(memory::stats().bytes, 0);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn fragment_preflight_rejects_over_budget_list_without_allocating() {
        let scalar = ty(0, &[]);
        let children = [&scalar as *const Type];
        let list_type = ty(2, &children);
        let mut word = 0;
        let source = abi::List {
            len: 0,
            cap: (BYTES / 8 + 1) as i64,
            data: &mut word,
        };
        let before = memory::stats();
        assert!(
            unsafe { cost::value(null_mut(), &list_type, &source as *const _ as i64) }.is_none()
        );
        assert_eq!(memory::stats().bytes, before.bytes);
        assert_eq!(memory::stats().objects, before.objects);
    }

    #[test]
    fn fragment_pid_preserves_generation_and_retains_retired_actor_until_collection() {
        unsafe {
            let scalar = ty(0, &[]);
            let children = [&scalar as *const Type];
            let pid_type = ty(6, &children);
            let session = control::Owned::new(Session::default());
            let s = session.as_ptr();
            (*s).session_key = s as usize;
            let actor = control::Owned::new(Actor {
                identity: ActorIdentity {
                    session_key: s as usize,
                    mailbox: &scalar,
                    id: u64::MAX,
                    ..ActorIdentity::default()
                },
                _session: Some(control::Owned::retain(s)),
                ..Actor::default()
            });
            let weak = actor.downgrade();
            let pid = new_pid(actor.as_ptr());
            assert!(cost::value(s, &pid_type, pid as i64).is_some());
            let fragment = value_fragment(s, &pid_type, pid as i64);
            let expected = (
                s as usize,
                actor.as_ptr() as usize,
                &scalar as *const _ as usize,
            );
            drop(actor);
            drop(session);
            memory::morrow_gc_collect_precise();
            assert_eq!(
                weak.strong_count(),
                1,
                "only the fragment retains the actor"
            );
            std::thread::spawn(move || {
                let pid = fragment.adopt() as *const Pid;
                assert_eq!((*pid).id, u64::MAX);
                assert_eq!(
                    (
                        (*pid).session as usize,
                        (*pid).actor as usize,
                        (*pid).mailbox as usize
                    ),
                    expected
                );
                assert!(memory::verify_heap_edges().is_ok());
                memory::morrow_gc_collect_precise();
                assert_eq!(memory::stats().bytes, 0);
            })
            .join()
            .unwrap();
            assert!(weak.upgrade().is_none());
            assert_eq!(memory::stats().bytes, 0);
        }
    }
}
