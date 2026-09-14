use super::{abi, rendered, text};
use std::ffi::c_char;

unsafe fn wrap(prefix: &str, value: *const c_char) -> *const c_char {
    if value.is_null() {
        std::ptr::null()
    } else {
        rendered(&format!("{prefix}{}\x1b[0m", unsafe { text(value) }))
    }
}

/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
macro_rules! styles { ($($name:ident => $code:literal),* $(,)?) => { $(#[doc = "# Safety\nNon-null strings must reference readable NUL-terminated storage for the call."] #[unsafe(no_mangle)] pub unsafe extern "C" fn $name(value: *const c_char) -> *const c_char { unsafe { wrap(concat!("\x1b[", $code, "m"), value) } })* }; }
styles!(morrow_style_black => "30", morrow_style_red => "31", morrow_style_green => "32", morrow_style_yellow => "33", morrow_style_blue => "34", morrow_style_magenta => "35", morrow_style_cyan => "36", morrow_style_white => "37", morrow_style_bright_black => "90", morrow_style_bright_red => "91", morrow_style_bright_green => "92", morrow_style_bright_yellow => "93", morrow_style_bright_blue => "94", morrow_style_bright_magenta => "95", morrow_style_bright_cyan => "96", morrow_style_bright_white => "97", morrow_style_on_black => "40", morrow_style_on_red => "41", morrow_style_on_green => "42", morrow_style_on_yellow => "43", morrow_style_on_blue => "44", morrow_style_on_magenta => "45", morrow_style_on_cyan => "46", morrow_style_on_white => "47", morrow_style_bold => "1", morrow_style_dim => "2", morrow_style_italic => "3", morrow_style_underline => "4", morrow_style_blink => "5", morrow_style_reverse => "7", morrow_style_strikethrough => "9");

macro_rules! palette {
    ($name:ident, $prefix:literal) => {
        #[unsafe(no_mangle)]
        /// # Safety
        /// Non-null string pointers must reference readable NUL-terminated storage.
        /// Object and list pointers must use the declared runtime ABI, remain live for
        /// this call, and allow any requested mutation without aliasing or concurrent access.
        pub unsafe extern "C" fn $name(value: *const c_char, code: i64) -> *const c_char {
            unsafe {
                wrap(
                    &format!("\x1b[{};5;{}m", $prefix, code.clamp(0, 255)),
                    value,
                )
            }
        }
    };
}
palette!(morrow_style_color, 38);
palette!(morrow_style_on_color, 48);

macro_rules! rgb {
    ($name:ident, $prefix:literal) => {
        #[unsafe(no_mangle)]
        /// # Safety
        /// Non-null string pointers must reference readable NUL-terminated storage.
        /// Object and list pointers must use the declared runtime ABI, remain live for
        /// this call, and allow any requested mutation without aliasing or concurrent access.
        pub unsafe extern "C" fn $name(
            value: *const c_char,
            r: i64,
            g: i64,
            b: i64,
        ) -> *const c_char {
            unsafe {
                wrap(
                    &format!(
                        "\x1b[{};2;{};{};{}m",
                        $prefix,
                        r.clamp(0, 255),
                        g.clamp(0, 255),
                        b.clamp(0, 255)
                    ),
                    value,
                )
            }
        }
    };
}
rgb!(morrow_style_rgb, 38);
rgb!(morrow_style_on_rgb, 48);

pub(super) fn hex(value: &str) -> Option<[u8; 3]> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    Some([
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
    ])
}

macro_rules! hex_style {
    ($name:ident, $rgb:ident) => {
        #[unsafe(no_mangle)]
        /// # Safety
        /// Non-null string pointers must reference readable NUL-terminated storage.
        /// Object and list pointers must use the declared runtime ABI, remain live for
        /// this call, and allow any requested mutation without aliasing or concurrent access.
        pub unsafe extern "C" fn $name(
            value: *const c_char,
            color: *const c_char,
        ) -> *const c_char {
            match hex(unsafe { text(color) }) {
                Some([r, g, b]) => unsafe { $rgb(value, r.into(), g.into(), b.into()) },
                None => abi::string(unsafe { text(value) }),
            }
        }
    };
}
hex_style!(morrow_style_hex, morrow_style_rgb);
hex_style!(morrow_style_on_hex, morrow_style_on_rgb);

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_style_reset(value: *const c_char) -> *const c_char {
    if value.is_null() {
        return std::ptr::null();
    }
    let mut escaped = false;
    let value: String = unsafe { text(value) }
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
                true
            }
        })
        .collect();
    rendered(&value)
}

/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
macro_rules! status { ($($name:ident => ($label:literal, $background:literal, $foreground:literal)),* $(,)?) => { $(#[doc = "# Safety\nNon-null strings must reference readable NUL-terminated storage for the call."] #[unsafe(no_mangle)] pub unsafe extern "C" fn $name(value: *const c_char) -> *const c_char { let mut badge = concat!("\x1b[", $background, "m\x1b[", $foreground, "m\x1b[1m ", $label, " \x1b[0m").to_owned(); let message = unsafe { text(value) }; if !message.is_empty() { badge.push(' '); badge.push_str(message); } rendered(&badge) })* }; }
status!(morrow_status_warn => ("WARN", "48;5;214", "30"), morrow_status_ok => ("OKAY", "42", "30"), morrow_status_info => ("INFO", "44", "37"), morrow_status_error => ("FAIL", "41", "37"), morrow_status_debug => ("DBUG", "45", "37"));

pub(super) fn named_color(value: &str) -> String {
    if value.starts_with('#')
        && let Some([r, g, b]) = hex(value)
    {
        return format!("\x1b[38;2;{r};{g};{b}m");
    }
    let (prefix, name) = value
        .strip_prefix("bright_")
        .map_or((30, value), |name| (90, name));
    [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ]
    .iter()
    .position(|name2| *name2 == name)
    .map_or_else(String::new, |offset| format!("\x1b[{}m", prefix + offset))
}
