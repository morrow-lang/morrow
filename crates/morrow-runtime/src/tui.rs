//! Terminal rendering and bounded interactive prompts.
mod prompt;
mod style;
mod terminal;
mod tree;
mod widgets;
pub use prompt::*;
pub use style::*;
pub use terminal::*;
pub use tree::*;
pub use widgets::*;

use crate::abi;
use std::ffi::c_char;

thread_local! { static CHECKED_FAULT: std::cell::Cell<*mut i64> = const { std::cell::Cell::new(std::ptr::null_mut()) }; }

/// Enter a checked terminal call without unwinding through generated frames.
/// # Safety
/// Fault is writable until the matching leave; both calls occur on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_tui_fault_enter(fault: *mut i64) -> *mut i64 {
    CHECKED_FAULT.with(|slot| slot.replace(fault))
}

/// Restore the previous checked terminal scope after the native call returns.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_tui_fault_leave(previous: *mut i64) {
    CHECKED_FAULT.with(|slot| slot.set(previous));
}

fn limit_fault(message: &str) {
    let fault = CHECKED_FAULT.with(|slot| slot.get());
    if fault.is_null() {
        abi::fault(message);
    }
    // SAFETY: the compiler's matching enter/leave pair owns this writable cell.
    unsafe {
        if *fault == 0 {
            *fault = 14;
        }
    }
}

unsafe fn text<'a>(value: *const c_char) -> &'a str {
    if value.is_null() {
        ""
    } else {
        unsafe { abi::text(value) }
    }
}

fn rendered(value: &str) -> *const c_char {
    if value.len() > crate::io::TEXT_LIMIT {
        limit_fault("terminal rendering exceeds 16 MiB");
        return std::ptr::null();
    }
    abi::string(value)
}

fn repeat(value: &str, count: usize) -> String {
    if count > crate::io::TEXT_LIMIT / value.len().max(1) {
        limit_fault("terminal rendering exceeds 16 MiB");
        return String::new();
    }
    value.repeat(count)
}

fn display_width(value: &str) -> usize {
    let mut escaped = false;
    value
        .chars()
        .filter(|ch| {
            if escaped {
                if *ch == 'm' {
                    escaped = false;
                }
                false
            } else if *ch == '\x1b' {
                escaped = true;
                false
            } else {
                *ch != '\n' && *ch != '\r'
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi;

    #[test]
    fn tree_snapshots_and_log_escaping_are_exact() {
        unsafe {
            let root = morrow_tree_new(c"root".as_ptr());
            let child = morrow_tree_add(
                morrow_tree_new(c"child".as_ptr()),
                morrow_tree_new(c"leaf".as_ptr()),
            );
            let one = morrow_tree_add(root, child);
            let two = morrow_tree_add(one, morrow_tree_new(c"last".as_ptr()));
            assert_eq!(abi::text(morrow_tree_render(root)), "root");
            assert_eq!(
                abi::text(morrow_tree_render(one)),
                "root\n└── child\n    └── leaf"
            );
            assert_eq!(
                abi::text(morrow_tree_render(two)),
                "root\n├── child\n│   └── leaf\n└── last"
            );
            assert_eq!(
                abi::text(morrow_log_info(c"hé\n\t\\\x1b".as_ptr())),
                "[INFO] hé\\n\\t\\\\\\x1b"
            );
        }
    }

    #[test]
    fn panel_table_and_progress_preserve_native_rendering() {
        unsafe {
            let panel = morrow_panel_new(c"hello".as_ptr());
            assert_eq!(
                abi::text(morrow_panel_render(panel)),
                "╭───────╮\n│ hello │\n╰───────╯"
            );
            let table = morrow_table_add_column(morrow_table_new(), c"A".as_ptr());
            morrow_table_add_row(table, abi::strings(&["b"]));
            assert_eq!(
                abi::text(morrow_table_render(table)),
                "╭───╮\n│ A │\n│───│\n│ b │\n╰───╯"
            );
            let progress = morrow_progress_width(morrow_progress_new(4), 4);
            morrow_progress_set(progress, 2);
            assert_eq!(abi::text(morrow_progress_render(progress)), "[██░░]  50%");
            let spinner = morrow_spinner_style(morrow_spinner_new(), c"line".as_ptr());
            morrow_spinner_tick(spinner);
            assert_eq!(abi::text(morrow_spinner_render(spinner)), "\\");
        }
    }

    #[test]
    fn explicit_styles_clamp_colors_and_reset_sequences() {
        unsafe {
            assert_eq!(
                abi::text(morrow_style_rgb(c"x".as_ptr(), -1, 128, 300)),
                "\x1b[38;2;0;128;255mx\x1b[0m"
            );
            assert_eq!(
                abi::text(morrow_style_reset(morrow_style_red(c"hé".as_ptr()))),
                "hé"
            );
            assert_eq!(
                abi::text(morrow_style_hex(c"x".as_ptr(), c"bad".as_ptr())),
                "x"
            );
        }
    }
}
