//! Typed native JSON codecs with one shared parse/conversion/output allowance.
use crate::{abi, json, memory};
use fern_json::{
    Budget, DEPTH, Error, INPUT, Json, Kind, Limits, NODES, Node, OUTPUT, Result, convert, error,
    parse,
};
use std::ffi::c_char;
use std::rc::Rc;
#[repr(C)]
pub struct Codec {
    pub kind: i64,
    pub count: i64,
    pub children: *const *const Codec,
    pub names: *const *const c_char,
}
#[repr(C)]
pub struct Variant {
    pub name: *const c_char,
    pub count: i64,
    pub children: *const *const Codec,
}
#[cfg(test)]
#[path = "json_codec/budget_tests.rs"]
mod budget_tests;
#[path = "json_codec/containers.rs"]
mod containers;
#[path = "json_codec/custom.rs"]
mod custom;
#[cfg(test)]
#[path = "json_codec/custom_tests.rs"]
mod custom_tests;
#[cfg(test)]
#[path = "json_codec/root_tests.rs"]
mod root_tests;
#[path = "json_codec/sums.rs"]
mod sums;
#[cfg(test)]
#[path = "json_codec/tests.rs"]
mod tests;
#[path = "json_codec/unions.rs"]
mod unions;

// A fixed-address root for one partially constructed native value. Guards stay
// on the bounded decode call stack; completed children become reachable from
// their parent before the guard is retired.
struct ConstructionRoot {
    _root: memory::Root,
    _slot: Box<usize>,
}
impl ConstructionRoot {
    fn new(value: usize) -> Self {
        let slot = Box::new(value);
        // SAFETY: the boxed word outlives the root; fields drop in that order.
        let root = unsafe { memory::root_range(&*slot, 1) };
        Self {
            _root: root,
            _slot: slot,
        }
    }
    unsafe fn decoded(mut plan: *const Codec, value: i64) -> Self {
        // Transparent newtypes retain their underlying representation. Scalar
        // payloads never become false roots merely because their bits resemble
        // a managed address.
        unsafe {
            for _ in 0..DEPTH {
                match (*plan).kind {
                    14 => {
                        return Self::new(
                            if (*(*plan).children.cast::<custom::Callbacks>()).managed == 0 {
                                0
                            } else {
                                value as usize
                            },
                        );
                    }
                    11 => plan = *(*plan).children,
                    0 | 1 | 2 | 4 => return Self::new(0),
                    _ => return Self::new(value as usize),
                }
            }
        }
        unreachable!("successful decoding has already bounded descriptor depth")
    }
}

struct Execution<'a> {
    fault: *mut i64,
    budget: Budget<'a>,
    path: Rc<String>,
    #[cfg(test)]
    precise: bool,
    #[cfg(test)]
    allocations: Vec<usize>,
}
impl<'a> Execution<'a> {
    fn new(limits: &'a mut Limits) -> Self {
        Self {
            fault: std::ptr::null_mut(),
            budget: Budget {
                limits,
                work: 64 * 1024 * 1024,
                allocated: 128,
                nodes: 0,
                at: 0,
            },
            path: Rc::new(String::new()),
            #[cfg(test)]
            precise: false,
            #[cfg(test)]
            allocations: Vec::new(),
        }
    }
    fn allocated<T>(&mut self, pointer: *mut T) -> *mut T {
        #[cfg(test)]
        if self.precise {
            self.allocations.push(pointer as usize);
        }
        pointer
    }
    fn checkpoint(&self) -> Result<()> {
        #[cfg(test)]
        if self.precise {
            // Only enabled by the owned heap-0 codec test fixture. Detect a lost
            // temporary by address before any caller can dereference that object.
            unsafe {
                memory::fern_gc_collect_precise();
            }
            if self
                .allocations
                .iter()
                .any(|&p| !memory::heap_owns(0, p as *const _))
            {
                return Err(error(4, -1));
            }
        }
        Ok(())
    }
    fn locate<T>(&self, result: Result<T>) -> Result<T> {
        result.map_err(|mut e| {
            if e.path.is_none() {
                if e.code == 4 {
                    e.offset = -1;
                }
                e.path = Some(self.path.clone());
            }
            e
        })
    }
    fn step(&mut self, depth: usize) -> Result<()> {
        if depth >= DEPTH {
            return self.locate(Err(error(4, -1)));
        }
        let result = self.budget.work(1);
        self.locate(result)
    }
    fn at<T>(&mut self, key: &str, call: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.path.len() >= OUTPUT || key.len() > (OUTPUT - self.path.len() - 1) / 2 {
            return self.locate(Err(error(4, -1)));
        }
        let maximum = self.path.len() + 1 + 2 * key.len();
        let reserved = (|| {
            self.budget.work(maximum)?;
            self.budget.allocate(40)?;
            self.budget.allocate(maximum + 1)
        })();
        self.locate(reserved)?;
        let mut path = String::with_capacity(maximum);
        path.push_str(&self.path);
        path.push('/');
        for c in key.chars() {
            match c {
                '~' => path.push_str("~0"),
                '/' => path.push_str("~1"),
                _ => path.push(c),
            }
        }
        let old = std::mem::replace(&mut self.path, Rc::new(path));
        let result = call(self);
        let result = self.locate(result);
        self.path = old;
        result
    }
    unsafe fn source<'s>(&mut self, pointer: *const c_char) -> Result<&'s [u8]> {
        for length in 0..=INPUT {
            self.budget.work(1)?;
            // SAFETY: codec ABI inputs are live NUL-terminated strings; scan is bounded.
            if unsafe { *pointer.cast::<u8>().add(length) } == 0 {
                return Ok(unsafe { std::slice::from_raw_parts(pointer.cast(), length) });
            }
        }
        Err(error(4, -1))
    }
    unsafe fn name<'s>(&mut self, pointer: *const c_char) -> Result<&'s str> {
        std::str::from_utf8(unsafe { self.source(pointer) }?).map_err(|_| error(2, -1))
    }
    unsafe fn text(&mut self, pointer: *const c_char) -> Result<Json> {
        let text = unsafe { self.name(pointer) }?;
        self.budget.work(text.len() * 2)?;
        self.budget.node()?;
        self.budget.allocate(text.len() + 1)?;
        Ok(fern_json::text_node(text.to_owned(), 0))
    }
    fn scalar(&mut self, kind: i64, bits: i64) -> Result<Json> {
        self.budget.work(256)?;
        self.budget.allocate(256)?;
        if self.budget.nodes == NODES {
            return Err(error(4, -1));
        }
        fern_json::scope::charge_nodes(1)?;
        self.budget.nodes += 1;
        let (kind, encoded) = match kind {
            0 => {
                let text = bits.to_string();
                let length = text.len();
                (Kind::Number(text), length)
            }
            1 => {
                let text = convert::format(f64::from_bits(bits as u64))?;
                let length = text.len();
                (Kind::Number(text), length)
            }
            2 => (Kind::Bool(bits != 0), if bits != 0 { 4 } else { 5 }),
            4 => (Kind::Null, 4),
            _ => return Err(error(5, -1)),
        };
        Ok(Rc::new(Node {
            kind,
            offset: 0,
            height: 1,
            nodes: 1,
            encoded,
        }))
    }
    fn slots(&mut self, count: usize, list: bool) -> Result<*mut i64> {
        if count > NODES {
            return Err(error(4, -1));
        }
        self.budget.work(count)?;
        // Native lists always own at least one writable data slot, including
        // decoded empties. Actor graph validation/copying uses this same ABI.
        let capacity = if list { count.max(1) } else { count };
        let bytes = (capacity + if list { 3 } else { 1 }) * 8;
        self.budget.allocate(bytes)?;
        self.checkpoint()?;
        Ok(self.allocated(memory::alloc(bytes, false).cast()))
    }
    fn index(&mut self, index: usize) -> Result<String> {
        self.budget.work(20)?;
        self.budget.allocate(21)?;
        Ok(index.to_string())
    }
    unsafe fn same_key(&mut self, key: &Json, name: *const c_char) -> Result<bool> {
        let Kind::String(key) = &key.kind else {
            return Err(error(5, -1));
        };
        let name = unsafe { self.name(name) }?;
        self.budget.work(key.len().min(name.len()) + 1)?;
        Ok(key == name)
    }
    unsafe fn encode(&mut self, plan: *const Codec, bits: i64, depth: usize) -> Result<Json> {
        self.step(depth)?;
        let result = unsafe { self.encode_kind(plan, bits, depth) };
        self.locate(result)
    }
    unsafe fn encode_kind(&mut self, p: *const Codec, bits: i64, depth: usize) -> Result<Json> {
        unsafe {
            match (*p).kind {
                14 => self.custom_encode(p, bits, depth),
                0 | 1 | 2 | 4 => self.scalar((*p).kind, bits),
                3 => self.text(bits as *const c_char),
                5 => {
                    let value = json::node(bits as *const json::NativeJson);
                    self.budget.work(value.nodes)?;
                    Ok(value)
                }
                6 | 8 => self.encode_array(p, bits, depth),
                7 => {
                    let value = bits as *const i64;
                    if *value != 0 {
                        self.scalar(4, 0)
                    } else {
                        self.encode(*(*p).children, *value.add(1), depth + 1)
                    }
                }
                9 | 10 => self.encode_object(p, bits, depth),
                11 => self.encode(*(*p).children, bits, depth + 1),
                12 => self.encode_sum(p, bits, depth),
                13 => {
                    let values = bits as *const i64;
                    let tag = *values;
                    if tag < 0 || tag >= (*p).count {
                        return Err(error(14, -1));
                    }
                    self.encode(*(*p).children.add(tag as usize), *values.add(1), depth + 1)
                }
                _ => Err(error(5, -1)),
            }
        }
    }
    unsafe fn decode(&mut self, plan: *const Codec, value: &Json, depth: usize) -> Result<i64> {
        self.checkpoint()?;
        self.step(depth)?;
        let result = unsafe { self.decode_kind(plan, value, depth) };
        self.locate(result)
    }
    unsafe fn decode_kind(&mut self, p: *const Codec, v: &Json, depth: usize) -> Result<i64> {
        unsafe {
            if (*p).kind <= 3 {
                let work = match &v.kind {
                    Kind::Number(t) | Kind::String(t) => t.len() * 2,
                    _ => 1,
                };
                self.budget.work(work)?;
                self.budget.allocate(64)?;
            }
            match ((*p).kind, &v.kind) {
                (14, _) => self.custom_decode(p, v),
                (0, Kind::Number(t)) => convert::integer(t),
                (1, Kind::Number(t)) => convert::float(t).map(|v| v.to_bits() as i64),
                (2, Kind::Bool(v)) => Ok(i64::from(*v)),
                (3, Kind::String(t)) => {
                    if t.contains('\0') {
                        Err(error(10, -1))
                    } else {
                        Ok(self.allocated(abi::string(t) as *mut c_char) as i64)
                    }
                }
                (4, Kind::Null) => Ok(0),
                (5, _) => {
                    self.budget.work(v.nodes)?;
                    Ok(self.allocated(json::wrap(v.clone())) as i64)
                }
                (6 | 8, _) => self.decode_array(p, v, depth),
                (7, _) => {
                    let out = self.slots(1, false)?;
                    let _out_root = ConstructionRoot::new(out as usize);
                    *out = i64::from(matches!(v.kind, Kind::Null));
                    if *out == 0 {
                        *out.add(1) = self.decode(*(*p).children, v, depth + 1)?;
                    }
                    Ok(out as i64)
                }
                (9, _) => self.decode_map(p, v, depth),
                (10, _) => self.decode_record(p, v, depth),
                (11, _) => self.decode(*(*p).children, v, depth + 1),
                (12, _) => self.decode_sum(p, v, depth),
                (13, _) => {
                    let selected = self.union_select(p, v)?;
                    let payload = self.decode(*(*p).children.add(selected), v, depth + 1)?;
                    let _payload_root =
                        ConstructionRoot::decoded(*(*p).children.add(selected), payload);
                    let out = self.slots(1, false)?;
                    *out = selected as i64;
                    *out.add(1) = payload;
                    Ok(out as i64)
                }
                _ => Err(error(5, -1)),
            }
        }
    }
}

/// Encode a compiler-validated native value and serialize under one shared budget.
/// # Safety
/// Plan is a live compiler-validated finite descriptor graph; payload follows its exact native layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_codec_encode(plan: *const Codec, payload: i64) -> i64 {
    let mut fault = 0;
    let result = unsafe { fern_json_codec_encode_context(plan, payload, &mut fault) };
    if fault == 0 { result } else { json::domain(4) }
}

/// Encode with an invocation-owned fault word, preserving language faults in custom methods.
/// # Safety
/// Plan/payload satisfy the codec ABI; fault addresses a live exclusive i64 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_codec_encode_context(
    plan: *const Codec,
    payload: i64,
    fault: *mut i64,
) -> i64 {
    if unsafe { *fault } != 0 {
        return 0;
    }
    let mut limits = json::limits();
    let mut c = Execution::new(&mut limits);
    c.fault = fault;
    let result = (|| {
        let value = unsafe { c.encode(plan, payload, 0) }?;
        if value.encoded > OUTPUT {
            return Err(error(4, -1));
        }
        c.budget.work(value.encoded + value.nodes)?;
        c.budget.allocate(value.encoded + 1)?;
        let mut text = String::with_capacity(value.encoded);
        fern_json::encode(&value, &mut text);
        Ok(abi::string(&text) as i64)
    })();
    if unsafe { *fault } != 0 {
        return 0;
    }
    match c.locate(result) {
        Ok(value) => abi::result_ok(value),
        Err(e) => json::failure(e),
    }
}
/// Parse and decode without resetting sibling, parse or conversion allowances.
/// # Safety
/// Plan is a live validated descriptor graph; text is readable NUL-terminated native input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_codec_decode(plan: *const Codec, text: *const c_char) -> i64 {
    let mut fault = 0;
    let result = unsafe { fern_json_codec_decode_context(plan, text, &mut fault) };
    if fault == 0 { result } else { json::domain(4) }
}

/// Decode with the same live fault word used by generated source functions.
/// # Safety
/// Plan/text satisfy the codec ABI; fault addresses a live exclusive i64 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_codec_decode_context(
    plan: *const Codec,
    text: *const c_char,
    fault: *mut i64,
) -> i64 {
    if unsafe { *fault } != 0 {
        return 0;
    }
    let mut limits = json::limits();
    let mut c = Execution::new(&mut limits);
    c.fault = fault;
    let parsed = (|| {
        let text = unsafe { c.source(text) }.map_err(|e| Error {
            offset: INPUT as i64,
            ..e
        })?;
        c.budget.work(8 * text.len())?;
        parse::document_bytes_in(text, &mut c.budget)
    })();
    let value = match parsed {
        Ok(v) => v,
        Err(e) => return json::failure(e),
    };
    c.budget.at = 0;
    let result = unsafe { c.decode(plan, &value, 0) };
    if unsafe { *fault } != 0 {
        return 0;
    }
    match result {
        Ok(v) => abi::result_ok(v),
        Err(e) => json::failure(e),
    }
}
