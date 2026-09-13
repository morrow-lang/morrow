//! Copy preflight-validated native graphs without retaining sender payload storage.
//!
//! The caller enters the receiver's heap first. Explicit temporary roots protect
//! every partial copy until the receiver publishes its frame/mailbox root.
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

pub(super) struct Copy {
    pub value: i64,
    session: *mut Session,
    roots: Vec<Rooted>,
    seen: HashMap<(i64, usize), i64>,
    json: HashMap<usize, fern_json::Json>,
}
impl Copy {
    fn new(session: *mut Session, source: i64) -> Self {
        Self {
            value: 0,
            session,
            roots: vec![Rooted::new(source as usize)],
            seen: HashMap::new(),
            json: HashMap::new(),
        }
    }
    fn allocate(&mut self, bytes: usize, atomic: bool) -> *mut u8 {
        let value = memory::alloc(bytes, atomic);
        self.roots.push(Rooted::new(value as usize));
        value
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
                    memory::control_edge(target.cast(), (*target).actor.cast());
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
                    let target = crate::json::wrap(node);
                    self.roots.push(Rooted::new(target as usize));
                    target as i64
                }
                _ => unreachable!("copy requires successful descriptor and graph preflight"),
            };
            self.seen.insert(key, copied);
            copied
        }
    }
    fn json(&mut self, source: &fern_json::Json) -> fern_json::Json {
        use fern_json::Kind;
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
        let copy = Rc::new(fern_json::Node {
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
pub(super) unsafe fn value(session: *mut Session, ty: *const Type, source: i64) -> Copy {
    let mut copy = Copy::new(session, source);
    copy.value = unsafe { copy.value(ty, source) };
    copy
}
