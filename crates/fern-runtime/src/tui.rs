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

unsafe fn text<'a>(value: *const c_char) -> &'a str {
    if value.is_null() {
        ""
    } else {
        unsafe { abi::text(value) }
    }
}

fn rendered(value: &str) -> *const c_char {
    if value.len() > crate::io::TEXT_LIMIT {
        abi::fault("terminal rendering exceeds 16 MiB");
    }
    abi::string(value)
}

fn repeat(value: &str, count: usize) -> String {
    if count > crate::io::TEXT_LIMIT / value.len().max(1) {
        abi::fault("terminal rendering exceeds 16 MiB");
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
            let root = fern_tree_new(c"root".as_ptr());
            let child = fern_tree_add(
                fern_tree_new(c"child".as_ptr()),
                fern_tree_new(c"leaf".as_ptr()),
            );
            let one = fern_tree_add(root, child);
            let two = fern_tree_add(one, fern_tree_new(c"last".as_ptr()));
            assert_eq!(abi::text(fern_tree_render(root)), "root");
            assert_eq!(
                abi::text(fern_tree_render(one)),
                "root\n└── child\n    └── leaf"
            );
            assert_eq!(
                abi::text(fern_tree_render(two)),
                "root\n├── child\n│   └── leaf\n└── last"
            );
            assert_eq!(
                abi::text(fern_log_info(c"hé\n\t\\\x1b".as_ptr())),
                "[INFO] hé\\n\\t\\\\\\x1b"
            );
        }
    }

    #[test]
    fn panel_table_and_progress_preserve_native_rendering() {
        unsafe {
            let panel = fern_panel_new(c"hello".as_ptr());
            assert_eq!(
                abi::text(fern_panel_render(panel)),
                "╭───────╮\n│ hello │\n╰───────╯"
            );
            let table = fern_table_add_column(fern_table_new(), c"A".as_ptr());
            fern_table_add_row(table, abi::strings(&["b"]));
            assert_eq!(
                abi::text(fern_table_render(table)),
                "╭───╮\n│ A │\n│───│\n│ b │\n╰───╯"
            );
            let progress = fern_progress_width(fern_progress_new(4), 4);
            fern_progress_set(progress, 2);
            assert_eq!(abi::text(fern_progress_render(progress)), "[██░░]  50%");
            let spinner = fern_spinner_style(fern_spinner_new(), c"line".as_ptr());
            fern_spinner_tick(spinner);
            assert_eq!(abi::text(fern_spinner_render(spinner)), "\\");
        }
    }

    #[test]
    fn explicit_styles_clamp_colors_and_reset_sequences() {
        unsafe {
            assert_eq!(
                abi::text(fern_style_rgb(c"x".as_ptr(), -1, 128, 300)),
                "\x1b[38;2;0;128;255mx\x1b[0m"
            );
            assert_eq!(
                abi::text(fern_style_reset(fern_style_red(c"hé".as_ptr()))),
                "hé"
            );
            assert_eq!(
                abi::text(fern_style_hex(c"x".as_ptr(), c"bad".as_ptr())),
                "x"
            );
        }
    }
}
