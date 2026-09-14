use super::{abi, rendered, text};
use std::ffi::c_char;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Tree {
    label: *const c_char,
    branches: *const c_char,
    last: *const c_char,
}

fn indent(value: &str, last: bool) -> String {
    let first = if last { "└── " } else { "├── " };
    let next = if last { "    " } else { "│   " };
    format!("{first}{}", value.replace('\n', &format!("\n{next}")))
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_tree_new(label: *const c_char) -> *mut Tree {
    let label = abi::string(unsafe { text(label) });
    abi::owned(
        Tree {
            label,
            branches: c"".as_ptr(),
            last: std::ptr::null(),
        },
        0,
    )
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_tree_render(tree: *const Tree) -> *const c_char {
    let Some(tree) = (unsafe { tree.as_ref() }) else {
        return abi::string("");
    };
    let label = unsafe { text(tree.label) };
    if tree.last.is_null() {
        return abi::string(label);
    }
    rendered(&format!(
        "{label}\n{}{}",
        unsafe { text(tree.branches) },
        indent(unsafe { text(tree.last) }, true)
    ))
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_tree_add(tree: *const Tree, child: *const Tree) -> *mut Tree {
    let Some(tree) = (unsafe { tree.as_ref() }) else {
        return std::ptr::null_mut();
    };
    if child.is_null() {
        return std::ptr::null_mut();
    }
    let mut result = *tree;
    let _root = unsafe { crate::memory::root_range((&result as *const Tree).cast(), 3) };
    let child_word = [child as usize];
    let _child_root = unsafe { crate::memory::root_range(child_word.as_ptr(), 1) };
    if !result.last.is_null() {
        result.branches = rendered(&format!(
            "{}{}\n",
            unsafe { text(result.branches) },
            indent(unsafe { text(result.last) }, false)
        ));
    }
    result.last = unsafe { morrow_tree_render(child) };
    abi::owned(result, 0)
}

fn record(level: &str, value: &str) -> *const c_char {
    let mut line = format!("[{level}] ");
    for ch in value.chars() {
        match ch {
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            '\t' => line.push_str("\\t"),
            '\\' => line.push_str("\\\\"),
            ch if ch < ' ' || ch == '\x7f' => {
                use std::fmt::Write;
                let _ = write!(line, "\\x{:02x}", ch as u32);
            }
            _ => line.push(ch),
        }
    }
    rendered(&line)
}

/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
macro_rules! logs { ($($name:ident => $level:literal),* $(,)?) => { $(#[doc = "# Safety\nNon-null strings must reference readable NUL-terminated storage for the call."] #[unsafe(no_mangle)] pub unsafe extern "C" fn $name(message: *const c_char) -> *const c_char { record($level, unsafe { text(message) }) })* }; }
logs!(morrow_log_debug => "DEBUG", morrow_log_info => "INFO", morrow_log_warn => "WARN", morrow_log_error => "ERROR");
