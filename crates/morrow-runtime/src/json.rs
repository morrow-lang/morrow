//! Native JSON handles backed by the same safe, bounded engine as the REPL.
use crate::{
    abi::{self, List},
    collections, memory,
};
use morrow_json::{Budget, Error, INPUT, Json, Kind, Limits, NODES, Node, OUTPUT, convert, parse};
use std::{
    ffi::{CStr, c_char},
    mem::size_of,
    rc::Rc,
};

/// Opaque native handle; its Rust owner is finalized by the collector.
pub struct NativeJson(pub(crate) Json);
/// Stable native error layout, including an immutable JSON Pointer path.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NativeError {
    pub code: i64,
    pub offset: i64,
    pub path: *const c_char,
}
/// Native member bridge: key and value are opaque JSON handles.
#[repr(C)]
#[derive(Clone, Copy)]
struct Member {
    key: *mut NativeJson,
    value: *mut NativeJson,
}

pub(crate) fn limits() -> morrow_json::scope::ScopedLimits {
    morrow_json::scope::limits()
}
/// Reserve callback work and fresh storage before native projection or publication.
fn reserve(work: usize, allocated: usize) -> morrow_json::Result<()> {
    let mut allowance = limits();
    Limits::charge(&mut allowance.work, work)?;
    Limits::charge(&mut allowance.allocated, allocated)
}
fn scalar_result(value: i64) -> i64 {
    match reserve(1, size_of::<abi::ResultValue>()) {
        Ok(()) => abi::result_ok(value),
        Err(error) => failure(error),
    }
}
pub(crate) fn wrap(value: Json) -> *mut NativeJson {
    let retained = morrow_json::retained_bytes(&value);
    // SAFETY: Rc JSON owns only Rust allocations, never managed pointers. Drop is
    // nonreentrant and bounded by the sealed 128-level JSON depth.
    unsafe { memory::managed(NativeJson(value), retained) }
}
pub(crate) unsafe fn node(value: *const NativeJson) -> Json {
    // SAFETY: native callers provide a live opaque handle. Clone before allocations
    // so collection of the handle cannot invalidate borrowed Rust-owned contents.
    unsafe { (*value).0.clone() }
}
pub(crate) fn failure(error: Error) -> i64 {
    let path = error.path.as_deref().map(String::as_str).unwrap_or("");
    // Reporting is still possible after exhaustion. Charge its bounded storage
    // without recursively reporting a failed reservation; the enclosing callback
    // observes sticky exhaustion even when source code handles this error.
    let _ = reserve(
        path.len() + 1,
        path.len() + 1 + size_of::<NativeError>() + size_of::<abi::ResultValue>(),
    );
    let path = abi::string(path);
    abi::result_err(abi::owned(
        NativeError {
            code: i64::from(if error.code == 0 { 4 } else { error.code }),
            offset: error.offset,
            path,
        },
        0,
    ) as i64)
}
pub(crate) fn domain(code: u8) -> i64 {
    failure(morrow_json::error(code, -1))
}
pub(crate) fn published(result: morrow_json::Result<Json>) -> i64 {
    match result.and_then(|value| {
        // wrap traverses the retained subtree, even when its Rc was shared.
        reserve(
            value.nodes,
            size_of::<NativeJson>() + size_of::<abi::ResultValue>(),
        )?;
        Ok(value)
    }) {
        Ok(value) => abi::result_ok(wrap(value) as i64),
        Err(error) => failure(error),
    }
}
/// Bounded native byte borrow; permits malformed UTF-8 so the parser owns errors.
/// # Safety
/// Pointer must address a live NUL-terminated byte string.
pub(crate) unsafe fn input<'a>(pointer: *const c_char) -> morrow_json::Result<&'a [u8]> {
    let length = unsafe { libc::strnlen(pointer, INPUT + 1) };
    reserve((length + 1).min(INPUT + 1), 0)?;
    if length > INPUT {
        return Err(morrow_json::error(4, INPUT as i64));
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer.cast(), length) })
}
fn built(result: morrow_json::Result<Json>) -> i64 {
    published(result.map_err(|error| Error {
        offset: -1,
        ..error
    }))
}
fn scalar(kind: Kind, encoded: usize) -> Json {
    // Infallible source constructors retain their json.Value ABI. The generated
    // callback checkpoint turns exhausted scoped work into the enclosing codec's
    // resource error before source code can consume a fabricated value.
    let mut allowance = limits();
    let _ = Limits::charge(&mut allowance.work, 256);
    let _ = Limits::charge(
        &mut allowance.allocated,
        encoded + size_of::<Node>() + 2 * size_of::<usize>() + size_of::<NativeJson>(),
    );
    let _ = morrow_json::scope::charge_nodes(1);
    Rc::new(Node {
        kind,
        offset: 0,
        height: 1,
        nodes: 1,
        encoded,
    })
}
fn text_node(text: &str, budget: &mut Budget<'_>) -> morrow_json::Result<Json> {
    if text.len() > INPUT {
        return Err(morrow_json::error(4, -1));
    }
    budget.node()?;
    budget.allocate(text.len() + 1)?;
    Ok(morrow_json::text_node(text.to_owned(), 0))
}

/// Parse one bounded native JSON document with exact byte offsets.
/// # Safety
/// Input must be a live NUL-terminated byte string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_parse(text: *const c_char) -> i64 {
    published(unsafe { input(text) }.and_then(|text| parse::document_bytes(text, &mut limits())))
}
/// Encode an immutable JSON value in insertion order.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_stringify(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match morrow_json::stringify(&value, &mut limits()) {
        Ok(text) => match reserve(0, size_of::<abi::ResultValue>()) {
            Ok(()) => abi::result_ok(abi::string(&text) as i64),
            Err(error) => failure(error),
        },
        Err(e) => failure(e),
    }
}
/// Inspect null without allocating.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_is_null(value: *const NativeJson) -> i64 {
    i64::from(matches!(unsafe { node(value) }.kind, Kind::Null))
}
/// Look up a decoded object key.
/// # Safety
/// Value is a live opaque handle; key is a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_get(
    value: *const NativeJson,
    key: *const c_char,
) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Object(members, index) = &value.kind else {
        return domain(5);
    };
    let key = match unsafe { input(key) } {
        Ok(bytes) => bytes,
        Err(_) => return domain(4),
    };
    let Ok(key) = std::str::from_utf8(key) else {
        return domain(6);
    };
    published(morrow_json::get(members, index, key, &mut limits()))
}
/// Look up an array element with signed bounds checking.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_at(value: *const NativeJson, index: i64) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Array(values) = &value.kind else {
        return domain(5);
    };
    published(
        usize::try_from(index)
            .ok()
            .and_then(|i| values.get(i))
            .cloned()
            .ok_or_else(|| morrow_json::error(7, -1)),
    )
}
/// Return the number of array elements or object members.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_length(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match &value.kind {
        Kind::Array(v) => scalar_result(v.len() as i64),
        Kind::Object(v, _) => scalar_result(v.len() as i64),
        _ => domain(5),
    }
}
/// Read a JSON Boolean as a checked result.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_as_bool(value: *const NativeJson) -> i64 {
    match unsafe { node(value) }.kind {
        Kind::Bool(v) => scalar_result(i64::from(v)),
        _ => domain(5),
    }
}
/// Read decoded text, rejecting embedded NUL at the native string boundary.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_as_string(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match &value.kind {
        Kind::String(v) => {
            match reserve(v.len() * 2, v.len() + 1 + size_of::<abi::ResultValue>()) {
                Err(error) => failure(error),
                Ok(()) if v.contains('\0') => domain(10),
                Ok(()) => abi::result_ok(abi::string(v) as i64),
            }
        }
        _ => domain(5),
    }
}
/// Read exact validated number spelling.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_number_text(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match &value.kind {
        Kind::Number(v) => match reserve(v.len(), v.len() + 1 + size_of::<abi::ResultValue>()) {
            Ok(()) => abi::result_ok(abi::string(v) as i64),
            Err(error) => failure(error),
        },
        _ => domain(5),
    }
}
/// Convert mathematically integral decimal text to signed64 without rounding.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_as_int(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Number(text) = &value.kind else {
        return domain(5);
    };
    match reserve(text.len() * 2, size_of::<abi::ResultValue>())
        .and_then(|()| convert::integer(text))
    {
        Ok(value) => abi::result_ok(value),
        Err(e) => failure(e),
    }
}
/// Round decimal text once to binary64, rejecting overflow and nonzero underflow.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_as_float(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Number(text) = &value.kind else {
        return domain(5);
    };
    match reserve(text.len(), size_of::<abi::ResultValue>()).and_then(|()| convert::float(text)) {
        Ok(value) => abi::result_ok(value.to_bits() as i64),
        Err(e) => failure(e),
    }
}
/// Construct an immutable JSON null.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_json_value_null() -> *mut NativeJson {
    wrap(scalar(Kind::Null, 4))
}
/// Construct an immutable JSON Boolean.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_json_value_from_bool(value: i64) -> *mut NativeJson {
    wrap(scalar(
        Kind::Bool(value != 0),
        if value != 0 { 4 } else { 5 },
    ))
}
/// Construct an exact signed64 JSON number.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_json_value_from_int(value: i64) -> *mut NativeJson {
    let text = value.to_string();
    let size = text.len();
    wrap(scalar(Kind::Number(text), size))
}
/// Construct a finite JSON number from binary64.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_json_value_from_float(value: f64) -> i64 {
    published(
        reserve(64, 64)
            .and_then(|()| convert::format(value))
            .map(|text| {
                let size = text.len();
                scalar(Kind::Number(text), size)
            }),
    )
}
/// Validate and retain exactly one number token without whitespace.
/// # Safety
/// Input must be a live NUL-terminated byte string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_from_number_text(text: *const c_char) -> i64 {
    let result = (|| {
        let bytes = unsafe { input(text) }?;
        let text = std::str::from_utf8(bytes).map_err(|_| morrow_json::error(1, -1))?;
        parse::number(text, Budget::new(&mut limits(), text.len())?)
    })();
    built(result)
}
/// Construct validated decoded JSON text.
/// # Safety
/// Input must be a live NUL-terminated byte string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_from_string(text: *const c_char) -> i64 {
    let result = (|| {
        let bytes = unsafe { input(text) }?;
        let text = std::str::from_utf8(bytes).map_err(|_| morrow_json::error(2, -1))?;
        text_node(text, &mut Budget::new(&mut limits(), text.len())?)
    })();
    built(result)
}
/// Copy an array's outer storage while retaining immutable JSON children.
/// # Safety
/// List must contain live opaque JSON handles and have a valid native layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_from_array(list: *const List) -> i64 {
    let header = unsafe { &*list };
    if header.len < 0 || header.len as usize >= NODES || header.cap < header.len {
        return domain(4);
    }
    let result = (|| {
        let mut limits = limits();
        let mut budget = Budget::new(&mut limits, 0)?;
        budget.node()?;
        budget.allocate(header.len as usize * 8)?;
        budget.work(header.len as usize)?;
        let children = unsafe { collections::elements(list) }
            .iter()
            .map(|&v| unsafe { node(v as *const NativeJson) })
            .collect();
        morrow_json::seal(children, false, 0, &mut budget)
    })();
    built(result)
}
/// Copy object keys and retain immutable JSON values, rejecting duplicate keys.
/// # Safety
/// Keys contain live CStrings; values contain live opaque handles. Lists are valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_from_object(
    keys: *const List,
    values: *const List,
) -> i64 {
    let (kh, vh) = unsafe { (&*keys, &*values) };
    if kh.len < 0
        || kh.len as usize > (NODES - 1) / 2
        || kh.cap < kh.len
        || vh.len != kh.len
        || vh.cap < vh.len
    {
        return domain(4);
    }
    let result = (|| {
        let (keys, values) =
            unsafe { (collections::elements(keys), collections::elements(values)) };
        reserve(keys.len(), keys.len() * size_of::<&str>())?;
        let mut bytes = 0_usize;
        let mut names = Vec::with_capacity(keys.len());
        for &key in keys {
            let text = unsafe { input(key as *const c_char) }?;
            bytes = bytes
                .checked_add(text.len())
                .filter(|&n| n <= OUTPUT)
                .ok_or_else(|| morrow_json::error(4, -1))?;
            names.push(std::str::from_utf8(text).map_err(|_| morrow_json::error(2, -1))?);
        }
        let mut limits = limits();
        let mut budget = Budget::new(&mut limits, bytes)?;
        budget.allocate(keys.len().max(1) * 16 + 48)?;
        budget.node()?;
        budget.allocate(keys.len() * 16)?;
        let mut children = Vec::with_capacity(keys.len() * 2);
        for (name, &value) in names.into_iter().zip(values) {
            children.push(text_node(name, &mut budget)?);
            children.push(unsafe { node(value as *const NativeJson) });
        }
        morrow_json::seal(children, true, 0, &mut budget)
    })();
    built(result)
}
/// Copy array handles into fresh traced list storage.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_elements(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Array(values) = &value.kind else {
        return domain(5);
    };
    let bytes = values.len() * (size_of::<i64>() + size_of::<NativeJson>())
        + values.len().max(1) * size_of::<i64>()
        + size_of::<List>()
        + size_of::<abi::ResultValue>();
    if let Err(error) = reserve(value.nodes, bytes) {
        return failure(error);
    }
    let mut words = vec![0_i64; values.len()];
    let _root = unsafe { memory::root_range(words.as_ptr().cast(), words.len()) };
    for (slot, value) in words.iter_mut().zip(values) {
        *slot = wrap(value.clone()) as i64;
    }
    abi::result_ok(abi::list(&words) as i64)
}
/// Copy ordered object members into native key/value bridge records.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_members(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Object(values, _) = &value.kind else {
        return domain(5);
    };
    let bytes = values.len()
        * (size_of::<i64>() + 2 * size_of::<NativeJson>() + size_of::<Member>())
        + values.len().max(1) * size_of::<i64>()
        + size_of::<List>()
        + size_of::<abi::ResultValue>();
    if let Err(error) = reserve(value.nodes, bytes) {
        return failure(error);
    }
    let mut words = vec![0_i64; values.len()];
    let _root = unsafe { memory::root_range(words.as_ptr().cast(), words.len()) };
    for (slot, (key, value)) in words.iter_mut().zip(values) {
        let mut pair = [wrap(key.clone()) as usize, 0];
        // The first fresh handle must survive allocation of the second and member.
        let _pair_root = unsafe { memory::root_range(pair.as_ptr(), pair.len()) };
        pair[1] = wrap(value.clone()) as usize;
        *slot = abi::owned(
            Member {
                key: pair[0] as *mut NativeJson,
                value: pair[1] as *mut NativeJson,
            },
            0,
        ) as i64;
    }
    abi::result_ok(abi::list(&words) as i64)
}
/// Construct the adapter's standard resource-limit failure.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_json_value_limit_error() -> i64 {
    domain(4)
}
/// Read a stable error code.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_error_code(error: *const NativeError) -> i64 {
    unsafe { (*error).code }
}
/// Read an original input byte offset, or -1 for a builder/conversion failure.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_error_offset(error: *const NativeError) -> i64 {
    unsafe { (*error).offset }
}
/// Read the native JSON Pointer path.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_error_path(error: *const NativeError) -> *const c_char {
    unsafe { (*error).path }
}
/// Read a static message without allocating or retaining input text.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_value_error_message(
    error: *const NativeError,
) -> *const c_char {
    const MESSAGES: &[&CStr] = &[
        c"",
        c"invalid JSON syntax",
        c"invalid JSON Unicode",
        c"duplicate JSON object key",
        c"JSON resource limit exceeded",
        c"JSON value has wrong type",
        c"JSON object key not found",
        c"JSON array index out of bounds",
        c"JSON number out of range",
        c"JSON number is not an integer",
        c"JSON string contains NUL",
        c"JSON number is not finite",
        c"unknown JSON object field",
        c"unknown JSON variant",
        c"no unique JSON union member",
    ];
    MESSAGES
        .get(unsafe { (*error).code } as usize)
        .unwrap_or(&c"invalid JSON error")
        .as_ptr()
}
