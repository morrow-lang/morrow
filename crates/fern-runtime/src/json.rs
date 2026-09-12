//! Native JSON handles backed by the same safe, bounded engine as the REPL.
use crate::{
    abi::{self, List},
    collections, memory,
};
use fern_json::{Budget, Error, INPUT, Json, Kind, Limits, NODES, Node, OUTPUT, convert, parse};
use std::{
    ffi::{CStr, c_char},
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

pub(crate) fn limits() -> Limits {
    Limits::new(usize::MAX)
}
pub(crate) fn wrap(value: Json) -> *mut NativeJson {
    let retained = fern_json::retained_bytes(&value);
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
    let path = abi::string(error.path.as_deref().map(String::as_str).unwrap_or(""));
    abi::result_err(abi::owned(
        NativeError {
            code: i64::from(error.code.max(1)),
            offset: error.offset,
            path,
        },
        0,
    ) as i64)
}
pub(crate) fn domain(code: u8) -> i64 {
    failure(fern_json::error(code, -1))
}
pub(crate) fn published(result: fern_json::Result<Json>) -> i64 {
    match result {
        Ok(value) => abi::result_ok(wrap(value) as i64),
        Err(error) => failure(error),
    }
}
/// Bounded native byte borrow; permits malformed UTF-8 so the parser owns errors.
/// # Safety
/// Pointer must address a live NUL-terminated byte string.
pub(crate) unsafe fn input<'a>(pointer: *const c_char) -> fern_json::Result<&'a [u8]> {
    let length = unsafe { libc::strnlen(pointer, INPUT + 1) };
    if length > INPUT {
        return Err(fern_json::error(4, INPUT as i64));
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer.cast(), length) })
}
fn built(result: fern_json::Result<Json>) -> i64 {
    published(result.map_err(|error| Error {
        offset: -1,
        ..error
    }))
}
fn scalar(kind: Kind, encoded: usize) -> Json {
    Rc::new(Node {
        kind,
        offset: 0,
        height: 1,
        nodes: 1,
        encoded,
    })
}
fn text_node(text: &str, budget: &mut Budget<'_>) -> fern_json::Result<Json> {
    if text.len() > INPUT {
        return Err(fern_json::error(4, -1));
    }
    budget.node()?;
    budget.allocate(text.len() + 1)?;
    Ok(fern_json::text_node(text.to_owned(), 0))
}

/// Parse one bounded native JSON document with exact byte offsets.
/// # Safety
/// Input must be a live NUL-terminated byte string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_parse(text: *const c_char) -> i64 {
    published(unsafe { input(text) }.and_then(|text| parse::document_bytes(text, &mut limits())))
}
/// Encode an immutable JSON value in insertion order.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_stringify(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match fern_json::stringify(&value, &mut limits()) {
        Ok(text) => abi::result_ok(abi::string(&text) as i64),
        Err(e) => failure(e),
    }
}
/// Inspect null without allocating.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_is_null(value: *const NativeJson) -> i64 {
    i64::from(matches!(unsafe { node(value) }.kind, Kind::Null))
}
/// Look up a decoded object key.
/// # Safety
/// Value is a live opaque handle; key is a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_get(value: *const NativeJson, key: *const c_char) -> i64 {
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
    published(fern_json::get(members, index, key, &mut limits()))
}
/// Look up an array element with signed bounds checking.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_at(value: *const NativeJson, index: i64) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Array(values) = &value.kind else {
        return domain(5);
    };
    published(
        usize::try_from(index)
            .ok()
            .and_then(|i| values.get(i))
            .cloned()
            .ok_or_else(|| fern_json::error(7, -1)),
    )
}
/// Return the number of array elements or object members.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_length(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match &value.kind {
        Kind::Array(v) => abi::result_ok(v.len() as i64),
        Kind::Object(v, _) => abi::result_ok(v.len() as i64),
        _ => domain(5),
    }
}
/// Read a JSON Boolean as a checked result.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_as_bool(value: *const NativeJson) -> i64 {
    match unsafe { node(value) }.kind {
        Kind::Bool(v) => abi::result_ok(i64::from(v)),
        _ => domain(5),
    }
}
/// Read decoded text, rejecting embedded NUL at the native string boundary.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_as_string(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match &value.kind {
        Kind::String(v) if v.contains('\0') => domain(10),
        Kind::String(v) => abi::result_ok(abi::string(v) as i64),
        _ => domain(5),
    }
}
/// Read exact validated number spelling.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_number_text(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    match &value.kind {
        Kind::Number(v) => abi::result_ok(abi::string(v) as i64),
        _ => domain(5),
    }
}
/// Convert mathematically integral decimal text to signed64 without rounding.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_as_int(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Number(text) = &value.kind else {
        return domain(5);
    };
    match convert::integer(text) {
        Ok(value) => abi::result_ok(value),
        Err(e) => failure(e),
    }
}
/// Round decimal text once to binary64, rejecting overflow and nonzero underflow.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_as_float(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Number(text) = &value.kind else {
        return domain(5);
    };
    match convert::float(text) {
        Ok(value) => abi::result_ok(value.to_bits() as i64),
        Err(e) => failure(e),
    }
}
/// Construct an immutable JSON null.
#[unsafe(no_mangle)]
pub extern "C" fn fern_json_value_null() -> *mut NativeJson {
    wrap(scalar(Kind::Null, 4))
}
/// Construct an immutable JSON Boolean.
#[unsafe(no_mangle)]
pub extern "C" fn fern_json_value_from_bool(value: i64) -> *mut NativeJson {
    wrap(scalar(
        Kind::Bool(value != 0),
        if value != 0 { 4 } else { 5 },
    ))
}
/// Construct an exact signed64 JSON number.
#[unsafe(no_mangle)]
pub extern "C" fn fern_json_value_from_int(value: i64) -> *mut NativeJson {
    let text = value.to_string();
    let size = text.len();
    wrap(scalar(Kind::Number(text), size))
}
/// Construct a finite JSON number from binary64.
#[unsafe(no_mangle)]
pub extern "C" fn fern_json_value_from_float(value: f64) -> i64 {
    published(convert::format(value).map(|text| {
        let size = text.len();
        scalar(Kind::Number(text), size)
    }))
}
/// Validate and retain exactly one number token without whitespace.
/// # Safety
/// Input must be a live NUL-terminated byte string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_from_number_text(text: *const c_char) -> i64 {
    let result = (|| {
        let bytes = unsafe { input(text) }?;
        let text = std::str::from_utf8(bytes).map_err(|_| fern_json::error(1, -1))?;
        parse::number(text, Budget::new(&mut limits(), text.len())?)
    })();
    built(result)
}
/// Construct validated decoded JSON text.
/// # Safety
/// Input must be a live NUL-terminated byte string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_from_string(text: *const c_char) -> i64 {
    let result = (|| {
        let bytes = unsafe { input(text) }?;
        let text = std::str::from_utf8(bytes).map_err(|_| fern_json::error(2, -1))?;
        text_node(text, &mut Budget::new(&mut limits(), text.len())?)
    })();
    built(result)
}
/// Copy an array's outer storage while retaining immutable JSON children.
/// # Safety
/// List must contain live opaque JSON handles and have a valid native layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_from_array(list: *const List) -> i64 {
    let header = unsafe { &*list };
    if header.len < 0 || header.len as usize >= NODES || header.cap < header.len {
        return domain(4);
    }
    let result = (|| {
        let mut limits = limits();
        let mut budget = Budget::new(&mut limits, 0)?;
        budget.node()?;
        budget.allocate(header.len as usize * 8)?;
        let children = unsafe { collections::elements(list) }
            .iter()
            .map(|&v| unsafe { node(v as *const NativeJson) })
            .collect();
        fern_json::seal(children, false, 0, &mut budget)
    })();
    built(result)
}
/// Copy object keys and retain immutable JSON values, rejecting duplicate keys.
/// # Safety
/// Keys contain live CStrings; values contain live opaque handles. Lists are valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_from_object(
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
        let mut bytes = 0_usize;
        let mut names = Vec::with_capacity(keys.len());
        for &key in keys {
            let text = unsafe { input(key as *const c_char) }?;
            bytes = bytes
                .checked_add(text.len())
                .filter(|&n| n <= OUTPUT)
                .ok_or_else(|| fern_json::error(4, -1))?;
            names.push(std::str::from_utf8(text).map_err(|_| fern_json::error(2, -1))?);
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
        fern_json::seal(children, true, 0, &mut budget)
    })();
    built(result)
}
/// Copy array handles into fresh traced list storage.
/// # Safety
/// Value must be a live opaque JSON handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_elements(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Array(values) = &value.kind else {
        return domain(5);
    };
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
pub unsafe extern "C" fn fern_json_value_members(value: *const NativeJson) -> i64 {
    let value = unsafe { node(value) };
    let Kind::Object(values, _) = &value.kind else {
        return domain(5);
    };
    let mut words = vec![0_i64; values.len()];
    let _root = unsafe { memory::root_range(words.as_ptr().cast(), words.len()) };
    for (slot, (key, value)) in words.iter_mut().zip(values) {
        let key = wrap(key.clone());
        let value = wrap(value.clone());
        *slot = abi::owned(Member { key, value }, 0) as i64;
    }
    abi::result_ok(abi::list(&words) as i64)
}
/// Construct the adapter's standard resource-limit failure.
#[unsafe(no_mangle)]
pub extern "C" fn fern_json_value_limit_error() -> i64 {
    domain(4)
}
/// Read a stable error code.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_error_code(error: *const NativeError) -> i64 {
    unsafe { (*error).code }
}
/// Read an original input byte offset, or -1 for a builder/conversion failure.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_error_offset(error: *const NativeError) -> i64 {
    unsafe { (*error).offset }
}
/// Read the native JSON Pointer path.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_error_path(error: *const NativeError) -> *const c_char {
    unsafe { (*error).path }
}
/// Read a static message without allocating or retaining input text.
/// # Safety
/// Error must be a live native error allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_json_value_error_message(error: *const NativeError) -> *const c_char {
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
