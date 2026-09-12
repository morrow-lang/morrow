use super::{abi, text};
use std::ffi::c_char;
use std::io::Write;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TermSize {
    pub cols: i64,
    pub rows: i64,
}

pub(super) fn dimensions() -> TermSize {
    let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut size) } == 0 {
        TermSize {
            cols: size.ws_col as i64,
            rows: size.ws_row as i64,
        }
    } else {
        TermSize { cols: 80, rows: 24 }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fern_term_size() -> *mut TermSize {
    abi::owned(dimensions(), 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn fern_term_is_tty() -> i64 {
    (unsafe { libc::isatty(1) } != 0) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn fern_term_color_support() -> i64 {
    if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
        || fern_term_is_tty() == 0
    {
        return 0;
    }
    if matches!(
        std::env::var("COLORTERM").as_deref(),
        Ok("truecolor" | "24bit")
    ) {
        return 16777216;
    }
    if std::env::var("TERM").is_ok_and(|value| value.contains("256")) {
        return 256;
    }
    16
}

pub(super) fn output(value: &str) {
    let mut output = std::io::stdout().lock();
    let _ = output.write_all(value.as_bytes());
    let _ = output.flush();
}

fn escape(value: &str) {
    if fern_term_is_tty() != 0 {
        output(value);
    }
}

macro_rules! movement { ($($name:ident => $direction:literal),* $(,)?) => { $(#[unsafe(no_mangle)] pub extern "C" fn $name(count: i64) { if count > 0 { escape(&format!("\x1b[{count}{}", $direction)); } })* }; }
movement!(fern_term_up => 'A', fern_term_down => 'B', fern_term_left => 'D', fern_term_right => 'C');

macro_rules! controls { ($($name:ident => $value:literal),* $(,)?) => { $(#[unsafe(no_mangle)] pub extern "C" fn $name() { escape($value); })* }; }
controls!(fern_term_clear => "\x1b[2J\x1b[H", fern_term_hide_cursor => "\x1b[?25l", fern_term_show_cursor => "\x1b[?25h", fern_term_save_cursor => "\x1b[s", fern_term_restore_cursor => "\x1b[u");

#[unsafe(no_mangle)]
pub extern "C" fn fern_term_move_to(row: i64, column: i64) {
    escape(&format!("\x1b[{};{}H", row.max(1), column.max(1)));
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_live_print(value: *const c_char) {
    output(unsafe { text(value) });
}
#[unsafe(no_mangle)]
pub extern "C" fn fern_live_clear_line() {
    output("\r\x1b[K");
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_live_update(value: *const c_char) {
    fern_live_clear_line();
    unsafe {
        fern_live_print(value);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn fern_live_done() {
    output("\n");
}
#[unsafe(no_mangle)]
pub extern "C" fn fern_sleep_ms(ms: i64) {
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}
