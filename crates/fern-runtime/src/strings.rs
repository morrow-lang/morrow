//! Native string operations implemented with Rust slices and owned buffers.
mod decimal;
mod format;
use crate::abi::{self, StringList};
pub use decimal::*;
use std::ffi::c_char;
use std::io::{self, Write};

const TEXT_LIMIT: usize = 16 * 1024 * 1024;

/// Release is a tracing-GC ownership hint; live string-list aliases remain usable.
#[unsafe(no_mangle)]
pub extern "C" fn fern_str_list_free(_list: *mut StringList) {}

fn output(value: &[u8], newline: bool) {
    let mut out = io::stdout().lock();
    let _ = out.write_all(value);
    if newline {
        let _ = out.write_all(b"\n");
    }
}

macro_rules! print_scalar {
    ($plain:ident, $line:ident, $convert:ident, $ty:ty, $format:expr) => {
        #[doc = "Write the scalar's canonical representation to stdout."]
        #[unsafe(no_mangle)]
        pub extern "C" fn $plain(value: $ty) {
            output(($format)(value).as_bytes(), false);
        }
        #[doc = "Write the scalar's canonical representation and a newline."]
        #[unsafe(no_mangle)]
        pub extern "C" fn $line(value: $ty) {
            output(($format)(value).as_bytes(), true);
        }
        #[doc = "Return the scalar's canonical representation in managed storage."]
        #[unsafe(no_mangle)]
        pub extern "C" fn $convert(value: $ty) -> *const c_char {
            abi::string(&($format)(value))
        }
    };
}
print_scalar!(
    fern_print_int,
    fern_println_int,
    fern_int_to_str,
    i64,
    |v: i64| v.to_string()
);

/// Parse exact ASCII decimal text with an optional sign into a full-width heap Option Int.
/// Whitespace, separators, radix prefixes, exponents and out-of-range values are `None`.
/// # Safety
/// Input must be a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_int_parse(text: *const c_char) -> i64 {
    let text = unsafe { abi::text(text) };
    abi::heap_option(parse_int(text))
}

/// Accept `[+-]?[0-9]+` within i64 range; Rust's parser already rejects other spellings.
fn parse_int(text: &str) -> Option<i64> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok()
}
print_scalar!(
    fern_print_float,
    fern_println_float,
    fern_float_to_str,
    f64,
    format::float
);
print_scalar!(
    fern_print_bool,
    fern_println_bool,
    fern_bool_to_str,
    i64,
    |v: i64| (v != 0).to_string()
);

macro_rules! unary_string {
    ($name:ident, $body:expr) => {
        /// Transform a native string into new managed storage.
        /// # Safety
        /// Input must be a live NUL-terminated UTF-8 string.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(value: *const c_char) -> *const c_char {
            // SAFETY: forwarded native CString contract.
            let text = unsafe { abi::text(value) };
            abi::string(&($body)(text))
        }
    };
}
fn whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}
unary_string!(fern_str_trim, |s: &str| s
    .trim_matches(whitespace)
    .to_owned());
unary_string!(fern_str_trim_start, |s: &str| s
    .trim_start_matches(whitespace)
    .to_owned());
unary_string!(fern_str_trim_end, |s: &str| s
    .trim_end_matches(whitespace)
    .to_owned());
unary_string!(fern_str_to_upper, |s: &str| s.to_ascii_uppercase());
unary_string!(fern_str_quote, |s: &str| quote(s));

/// Spell text as a Fern string literal: quotes plus `\" \\ \n \r \t` escapes, other bytes verbatim.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
unary_string!(fern_str_to_lower, |s: &str| s.to_ascii_lowercase());

macro_rules! compare_string {
    ($name:ident, $body:expr) => {
        /// Compare two native strings without allocation.
        /// # Safety
        /// Both inputs must be live NUL-terminated UTF-8 strings.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(left: *const c_char, right: *const c_char) -> i64 {
            // SAFETY: forwarded native CString contracts.
            let (left, right) = unsafe { (abi::text(left), abi::text(right)) };
            i64::from(($body)(left, right))
        }
    };
}
compare_string!(fern_str_eq, |a: &str, b: &str| a == b);
compare_string!(fern_str_compare, |a: &str, b: &str| match a.cmp(b) {
    std::cmp::Ordering::Less => -1,
    std::cmp::Ordering::Equal => 0,
    std::cmp::Ordering::Greater => 1,
});
compare_string!(fern_str_starts_with, |a: &str, b: &str| a.starts_with(b));
compare_string!(fern_str_ends_with, |a: &str, b: &str| a.ends_with(b));
compare_string!(fern_str_contains, |a: &str, b: &str| a.contains(b));

/// Write a native string to stdout.
/// # Safety
/// Input must be a live NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_print_str(value: *const c_char) {
    output(unsafe { abi::raw_bytes(value) }, false);
}
/// Write a native string and a newline to stdout.
/// # Safety
/// Input must be a live NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_println_str(value: *const c_char) {
    output(unsafe { abi::raw_bytes(value) }, true);
}
/// Return a string's byte length.
/// # Safety
/// Input must be a live NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_len(value: *const c_char) -> i64 {
    unsafe { abi::raw_bytes(value) }.len() as i64
}
/// Test whether a native string is empty.
/// # Safety
/// Input must be a live NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_is_empty(value: *const c_char) -> i64 {
    i64::from(unsafe { abi::raw_bytes(value) }.is_empty())
}
/// Concatenate two strings in managed storage.
/// # Safety
/// Inputs must be live NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_concat(a: *const c_char, b: *const c_char) -> *const c_char {
    let (a, b) = unsafe { (abi::raw_bytes(a), abi::raw_bytes(b)) };
    let mut result = Vec::with_capacity(
        a.len()
            .checked_add(b.len())
            .unwrap_or_else(|| abi::fault("string size limit exceeded")),
    );
    result.extend_from_slice(a);
    result.extend_from_slice(b);
    abi::bytes(&result)
}
/// Return the byte offset of a substring in the legacy scalar Option encoding.
/// # Safety
/// Inputs must be live NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_index_of(a: *const c_char, b: *const c_char) -> i64 {
    let (a, b) = unsafe { (abi::text(a), abi::text(b)) };
    a.find(b)
        .map_or_else(abi::option_none, |index| abi::option_some(index as i64))
}
fn endpoints(length: usize, start: i64, end: i64) -> (usize, usize) {
    let start = start.max(0);
    let end = end.max(start);
    (
        (start as u64).min(length as u64) as usize,
        (end as u64).min(length as u64) as usize,
    )
}
/// Validate clamped byte endpoints without allocating.
/// # Safety
/// Input must be a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_slice_is_valid(
    value: *const c_char,
    start: i64,
    end: i64,
) -> i64 {
    let text = unsafe { abi::text(value) };
    let (start, end) = endpoints(text.len(), start, end);
    i64::from(text.is_char_boundary(start) && text.is_char_boundary(end))
}
/// Copy a clamped byte slice, rejecting partial Unicode scalars.
/// # Safety
/// Input must be a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_slice(
    value: *const c_char,
    start: i64,
    end: i64,
) -> *const c_char {
    let text = unsafe { abi::text(value) };
    let (start, end) = endpoints(text.len(), start, end);
    let slice = text
        .get(start..end)
        .unwrap_or_else(|| abi::fault("String.slice indices must be UTF-8 character boundaries"));
    abi::string(slice)
}
/// Replace nonoverlapping substring occurrences; an empty pattern leaves text unchanged.
/// # Safety
/// Inputs must be live NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_replace(
    value: *const c_char,
    old: *const c_char,
    new: *const c_char,
) -> *const c_char {
    let (text, old, new) = unsafe { (abi::text(value), abi::text(old), abi::text(new)) };
    if old.is_empty() {
        abi::string(text)
    } else {
        abi::string(&text.replace(old, new))
    }
}
/// Validate Unicode scalar splitting; nonempty delimiters use byte splitting.
/// # Safety
/// Inputs must be live NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_split_is_valid(
    value: *const c_char,
    delimiter: *const c_char,
) -> i64 {
    let (text, delimiter) = unsafe { (abi::raw_bytes(value), abi::raw_bytes(delimiter)) };
    i64::from(!delimiter.is_empty() || std::str::from_utf8(text).is_ok())
}
/// Split on a delimiter, or into Unicode scalars when it is empty.
/// # Safety
/// Inputs must be live NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_split(
    value: *const c_char,
    delimiter: *const c_char,
) -> *mut StringList {
    let (text, delimiter) = unsafe { (abi::text(value), abi::text(delimiter)) };
    let parts: Vec<&str> = if delimiter.is_empty() {
        text.char_indices()
            .map(|(i, c)| &text[i..i + c.len_utf8()])
            .collect()
    } else {
        text.split(delimiter).collect()
    };
    abi::strings(&parts)
}
/// Split CR, LF or CRLF lines without adding a trailing empty line.
/// # Safety
/// Input must be a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_lines(value: *const c_char) -> *mut StringList {
    let text = unsafe { abi::text(value) };
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let parts: Vec<&str> = if normalized.is_empty() {
        vec![""]
    } else {
        normalized.split_terminator('\n').collect()
    };
    abi::strings(&parts)
}
/// Join native strings with a separator.
/// # Safety
/// List and each element must be live, valid native allocations; separator is UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_join(
    list: *const StringList,
    separator: *const c_char,
) -> *const c_char {
    let list = unsafe { &*list };
    let elements = unsafe { std::slice::from_raw_parts(list.data, list.len as usize) };
    let parts: Vec<&str> = elements.iter().map(|&p| unsafe { abi::text(p) }).collect();
    abi::string(&parts.join(unsafe { abi::text(separator) }))
}
/// Repeat text within the sixteen-MiB content limit.
/// # Safety
/// Input must be a live NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_repeat(value: *const c_char, count: i64) -> *const c_char {
    let text = unsafe { abi::raw_bytes(value) };
    if count <= 0 || text.is_empty() {
        return abi::string("");
    }
    if count as u64 > (TEXT_LIMIT / text.len()) as u64 {
        abi::fault("string size limit exceeded");
    }
    abi::bytes(&text.repeat(count as usize))
}
/// Return a byte at an index in the legacy scalar Option encoding.
/// # Safety
/// Input must be a live NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_str_char_at(value: *const c_char, index: i64) -> i64 {
    if index < 0 {
        return abi::option_none();
    }
    unsafe { abi::raw_bytes(value) }
        .get(index as usize)
        .map_or_else(abi::option_none, |&b| abi::option_some(i64::from(b)))
}
