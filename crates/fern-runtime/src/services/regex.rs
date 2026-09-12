//! POSIX extended regular expressions, retaining leftmost-longest and byte-offset semantics.
use crate::{abi, memory};
use std::ffi::{CStr, CString, c_char};

struct Regex {
    native: libc::regex_t,
    groups: usize,
}
impl Regex {
    fn new(pattern: &CStr) -> Option<Self> {
        let mut regex = std::mem::MaybeUninit::uninit();
        if unsafe { libc::regcomp(regex.as_mut_ptr(), pattern.as_ptr(), libc::REG_EXTENDED) } != 0 {
            return None;
        }
        Some(Self {
            native: unsafe { regex.assume_init() },
            groups: group_count(pattern.to_bytes()),
        })
    }
    fn find(&self, text: &CStr) -> Option<[libc::regmatch_t; 10]> {
        let mut matches = [libc::regmatch_t {
            rm_so: -1,
            rm_eo: -1,
        }; 10];
        if unsafe {
            libc::regexec(
                &self.native,
                text.as_ptr(),
                matches.len(),
                matches.as_mut_ptr(),
                0,
            )
        } == 0
        {
            Some(matches)
        } else {
            None
        }
    }
}
impl Drop for Regex {
    fn drop(&mut self) {
        unsafe {
            libc::regfree(&mut self.native);
        }
    }
}

// POSIX ERE has only capturing parentheses; escapes and bracket expressions are
// excluded. Count only after regcomp accepted the complete pattern. libc keeps
// re_nsub private, so no platform-specific regex_t layout is assumed.
fn group_count(pattern: &[u8]) -> usize {
    let mut groups = 0;
    let mut i = 0;
    while i < pattern.len() {
        match pattern[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'(' => groups += 1,
            b'[' => {
                i += 1;
                if pattern.get(i) == Some(&b'^') {
                    i += 1;
                }
                if pattern.get(i) == Some(&b']') {
                    i += 1;
                }
                while i < pattern.len() && pattern[i] != b']' {
                    if pattern[i] == b'[' && matches!(pattern.get(i + 1), Some(b':' | b'.' | b'='))
                    {
                        let delimiter = pattern[i + 1];
                        i += 2;
                        while i + 1 < pattern.len()
                            && !(pattern[i] == delimiter && pattern[i + 1] == b']')
                        {
                            i += 1;
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    groups
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct RegexMatch {
    pub start: i64,
    pub end: i64,
    pub matched: *const c_char,
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Captures {
    pub count: i64,
    pub captures: *mut RegexMatch,
}

unsafe fn input(value: *const c_char) -> CString {
    if value.is_null() {
        CString::default()
    } else {
        unsafe { CStr::from_ptr(value) }.to_owned()
    }
}

fn strings(values: &[&[u8]]) -> *mut abi::StringList {
    let mut words = vec![0_i64; values.len()];
    let _root = unsafe { memory::root_range(words.as_ptr().cast(), words.len()) };
    for (word, bytes) in words.iter_mut().zip(values) {
        *word = abi::bytes(bytes) as i64;
    }
    abi::list(&words).cast()
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_regex_is_match(text: *const c_char, pattern: *const c_char) -> i64 {
    Regex::new(&unsafe { input(pattern) })
        .is_some_and(|regex| regex.find(&unsafe { input(text) }).is_some()) as i64
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_regex_find(
    text: *const c_char,
    pattern: *const c_char,
) -> *mut RegexMatch {
    let text = unsafe { input(text) };
    let value = Regex::new(&unsafe { input(pattern) }).and_then(|regex| regex.find(&text));
    let value = value.map_or(
        RegexMatch {
            start: -1,
            end: -1,
            matched: std::ptr::null(),
        },
        |matches| {
            let value = matches[0];
            RegexMatch {
                start: offset(value.rm_so),
                end: offset(value.rm_eo),
                matched: abi::bytes(&text.as_bytes()[value.rm_so as usize..value.rm_eo as usize]),
            }
        },
    );
    abi::owned(value, 0)
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_regex_find_all(
    text: *const c_char,
    pattern: *const c_char,
) -> *mut abi::StringList {
    let text = unsafe { input(text) };
    let Some(regex) = Regex::new(&unsafe { input(pattern) }) else {
        return strings(&[]);
    };
    let bytes = text.as_bytes();
    let mut offset = 0;
    let mut values = Vec::new();
    while offset <= bytes.len() {
        let Some(matches) = regex.find(unsafe { CStr::from_ptr(text.as_ptr().add(offset)) }) else {
            break;
        };
        let (start, end) = (matches[0].rm_so as usize, matches[0].rm_eo as usize);
        values.push(&bytes[offset + start..offset + end]);
        offset += end;
        if offset == bytes.len() {
            break;
        }
        if start == end {
            offset += 1;
            if offset == bytes.len() {
                break;
            }
        }
    }
    strings(&values)
}

unsafe fn replace(
    text: *const c_char,
    pattern: *const c_char,
    replacement: *const c_char,
    all: bool,
) -> *const c_char {
    let text = unsafe { input(text) };
    let replacement = unsafe { input(replacement) };
    let Some(regex) = Regex::new(&unsafe { input(pattern) }) else {
        return abi::bytes(text.as_bytes());
    };
    let bytes = text.as_bytes();
    let mut output = Vec::new();
    let mut offset = 0;
    while offset <= bytes.len() {
        let Some(matches) = regex.find(unsafe { CStr::from_ptr(text.as_ptr().add(offset)) }) else {
            break;
        };
        let (start, end) = (matches[0].rm_so as usize, matches[0].rm_eo as usize);
        output.extend_from_slice(&bytes[offset..offset + start]);
        output.extend_from_slice(replacement.as_bytes());
        if output.len() > crate::io::TEXT_LIMIT {
            abi::fault("regex replacement exceeds 16 MiB");
        }
        offset += end;
        if !all {
            break;
        }
        if start == end {
            if offset == bytes.len() {
                break;
            }
            output.push(bytes[offset]);
            offset += 1;
        }
    }
    output.extend_from_slice(&bytes[offset..]);
    abi::bytes(&output)
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_regex_replace(
    text: *const c_char,
    pattern: *const c_char,
    replacement: *const c_char,
) -> *const c_char {
    unsafe { replace(text, pattern, replacement, false) }
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_regex_replace_all(
    text: *const c_char,
    pattern: *const c_char,
    replacement: *const c_char,
) -> *const c_char {
    unsafe { replace(text, pattern, replacement, true) }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_regex_split(
    text: *const c_char,
    pattern: *const c_char,
) -> *mut abi::StringList {
    let text = unsafe { input(text) };
    let bytes = text.as_bytes();
    let Some(regex) = Regex::new(&unsafe { input(pattern) }) else {
        return strings(&[bytes]);
    };
    let mut offset = 0;
    let mut values = Vec::new();
    while offset <= bytes.len() {
        let Some(matches) = regex.find(unsafe { CStr::from_ptr(text.as_ptr().add(offset)) }) else {
            break;
        };
        let (start, end) = (matches[0].rm_so as usize, matches[0].rm_eo as usize);
        values.push(&bytes[offset..offset + start]);
        offset += end;
        if start == end {
            if offset == bytes.len() {
                break;
            }
            offset += 1;
        }
    }
    values.push(&bytes[offset..]);
    strings(&values)
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_regex_captures(
    text: *const c_char,
    pattern: *const c_char,
) -> *mut Captures {
    let text = unsafe { input(text) };
    let found = Regex::new(&unsafe { input(pattern) }).and_then(|regex| {
        regex
            .find(&text)
            .map(|matches| (matches, (regex.groups + 1).min(10)))
    });
    let Some((matches, count)) = found else {
        return abi::owned(
            Captures {
                count: 0,
                captures: std::ptr::null_mut(),
            },
            0,
        );
    };
    let mut captures = vec![
        RegexMatch {
            start: -1,
            end: -1,
            matched: std::ptr::null()
        };
        count
    ];
    let _root = unsafe { memory::root_range(captures.as_ptr().cast(), count * 3) };
    for (capture, matched) in captures.iter_mut().zip(matches) {
        let bytes = if matched.rm_so < 0 {
            &[]
        } else {
            &text.as_bytes()[matched.rm_so as usize..matched.rm_eo as usize]
        };
        *capture = RegexMatch {
            start: offset(matched.rm_so),
            end: offset(matched.rm_eo),
            matched: abi::bytes(bytes),
        };
    }
    let data = memory::alloc(count * size_of::<RegexMatch>(), false).cast::<RegexMatch>();
    unsafe {
        std::ptr::copy_nonoverlapping(captures.as_ptr(), data, count);
    }
    abi::owned(
        Captures {
            count: count as i64,
            captures: data,
        },
        0,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn fern_regex_match_free(_: *mut RegexMatch) {}
#[unsafe(no_mangle)]
pub extern "C" fn fern_regex_captures_free(_: *mut Captures) {}

// Darwin regoff_t is i64, while Linux uses i32; both enter Fern's i64 ABI.
#[allow(clippy::unnecessary_cast)]
fn offset(value: libc::regoff_t) -> i64 {
    value as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn posix_longest_match_and_optional_capture_slots() {
        unsafe {
            let value = &*fern_regex_find(c"ab".as_ptr(), c"a|ab".as_ptr());
            assert_eq!((value.start, value.end), (0, 2));
            assert_eq!(crate::abi::text(value.matched), "ab");
            let groups = &*fern_regex_captures(c"b".as_ptr(), c"(a)?(b)".as_ptr());
            assert_eq!(groups.count, 3);
            let captures = std::slice::from_raw_parts(groups.captures, 3);
            assert_eq!((captures[1].start, captures[1].end), (-1, -1));
            assert_eq!(crate::abi::text(captures[1].matched), "");
            assert_eq!(crate::abi::text(captures[2].matched), "b");
            assert_eq!(
                crate::abi::text(fern_regex_replace_all(
                    c"a1b2".as_ptr(),
                    c"[0-9]".as_ptr(),
                    c"$1".as_ptr()
                )),
                "a$1b$1"
            );
        }
    }
}
