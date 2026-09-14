//! Small bounded line editor with RAII terminal restoration and private password input.
use super::terminal::output;
use super::{abi, text};
use std::ffi::{CString, c_char};
use std::io::BufRead;

struct Terminal {
    original: libc::termios,
}
impl Terminal {
    fn configure(raw: bool, masked: bool) -> Option<Self> {
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(0, &mut original) } != 0 {
            return None;
        }
        let mut settings = original;
        if raw {
            unsafe {
                libc::cfmakeraw(&mut settings);
            }
            settings.c_cc[libc::VMIN] = 1;
            settings.c_cc[libc::VTIME] = 0;
        }
        if masked {
            settings.c_lflag &= !(libc::ECHO | libc::ECHONL);
        }
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &settings) } != 0 {
            return None;
        }
        Some(Self { original })
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(0, libc::TCSAFLUSH, &self.original);
        }
    }
}

fn byte(wait: bool) -> Option<u8> {
    if !wait {
        let mut fd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut fd, 1, 50) } <= 0 {
            return None;
        }
    }
    let mut byte = 0;
    loop {
        let count = unsafe { libc::read(0, (&mut byte as *mut u8).cast(), 1) };
        if count == 1 {
            return Some(byte);
        }
        if count < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return None;
    }
}

#[derive(Default)]
struct Line {
    value: String,
    cursor: usize,
}
impl Line {
    fn left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            while !self.value.is_char_boundary(self.cursor) {
                self.cursor -= 1;
            }
        }
    }
    fn right(&mut self) {
        if self.cursor < self.value.len() {
            self.cursor += 1;
            while !self.value.is_char_boundary(self.cursor) {
                self.cursor += 1;
            }
        }
    }
    fn delete(&mut self) {
        if self.cursor < self.value.len() {
            self.value.remove(self.cursor);
        }
    }
    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.left();
            self.delete();
        }
    }
    fn insert(&mut self, ch: char) {
        if self.value.len() + ch.len_utf8() < 4096 {
            self.value.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
        }
    }
    fn transpose(&mut self) {
        if self.cursor == 0 || self.cursor == self.value.len() {
            return;
        }
        let current = self.value.remove(self.cursor);
        self.left();
        self.insert(current);
        self.right();
    }
    fn refresh(&self, prompt: &str, masked: bool) {
        let visible = if masked {
            "*".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };
        let after = self.value[self.cursor..].chars().count();
        output(&format!(
            "\r{prompt}{visible}\x1b[0K{}",
            if after == 0 {
                String::new()
            } else {
                format!("\x1b[{after}D")
            }
        ));
    }
    fn escape(&mut self) {
        if !matches!(byte(false), Some(b'[' | b'O')) {
            return;
        }
        match byte(false) {
            Some(b'D') => self.left(),
            Some(b'C') => self.right(),
            Some(b'H') => self.cursor = 0,
            Some(b'F') => self.cursor = self.value.len(),
            Some(code @ (b'1' | b'3' | b'4' | b'7' | b'8')) if byte(false) == Some(b'~') => {
                match code {
                    b'3' => self.delete(),
                    b'1' | b'7' => self.cursor = 0,
                    _ => self.cursor = self.value.len(),
                }
            }
            _ => {}
        }
    }
}

fn edited(prompt: &str, masked: bool) -> String {
    let Some(_terminal) = Terminal::configure(true, masked) else {
        return String::new();
    };
    output(prompt);
    let mut line = Line::default();
    while let Some(ch) = byte(true) {
        match ch {
            b'\r' | b'\n' => {
                output("\r\n");
                return line.value;
            }
            3 => {
                output("\r\n");
                return String::new();
            }
            4 if line.value.is_empty() => return String::new(),
            4 => line.delete(),
            1 => line.cursor = 0,
            5 => line.cursor = line.value.len(),
            2 => line.left(),
            6 => line.right(),
            8 | 127 => line.backspace(),
            11 => line.value.truncate(line.cursor),
            20 => line.transpose(),
            21 => {
                line.value.clear();
                line.cursor = 0;
            }
            23 => {
                while line.cursor > 0 && line.value[..line.cursor].ends_with(' ') {
                    line.backspace();
                }
                while line.cursor > 0 && !line.value[..line.cursor].ends_with(' ') {
                    line.backspace();
                }
            }
            27 => line.escape(),
            32..=126 => line.insert(ch as char),
            0xc2..=0xf4 => {
                let length = if ch < 0xe0 {
                    2
                } else if ch < 0xf0 {
                    3
                } else {
                    4
                };
                let mut bytes = [0_u8; 4];
                bytes[0] = ch;
                for slot in &mut bytes[1..length] {
                    let Some(next) = byte(true) else {
                        return String::new();
                    };
                    *slot = next;
                }
                if let Ok(value) = std::str::from_utf8(&bytes[..length])
                    && let Some(ch) = value.chars().next()
                {
                    line.insert(ch);
                }
            }
            _ => {}
        }
        line.refresh(prompt, masked);
    }
    String::new()
}

fn plain_line() -> String {
    let mut line = Vec::new();
    if std::io::stdin()
        .lock()
        .read_until(b'\n', &mut line)
        .is_err()
    {
        return String::new();
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if let Some(nul) = line.iter().position(|byte| *byte == 0) {
        line.truncate(nul);
    }
    String::from_utf8(line).unwrap_or_default()
}

fn prompt(prompt: &str, masked: bool) -> *const c_char {
    let input_tty = unsafe { libc::isatty(0) } != 0;
    let interactive = input_tty
        && unsafe { libc::isatty(1) } != 0
        && !matches!(
            std::env::var("TERM").as_deref(),
            Ok("dumb" | "cons25" | "emacs")
        );
    let line = if interactive {
        edited(prompt, masked)
    } else {
        let _hidden = if input_tty && masked {
            let Some(terminal) = Terminal::configure(false, true) else {
                return abi::string("");
            };
            Some(terminal)
        } else {
            None
        };
        output(prompt);
        plain_line()
    };
    abi::string(&line)
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_prompt_input(value: *const c_char) -> *const c_char {
    prompt(unsafe { text(value) }, false)
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_prompt_password(value: *const c_char) -> *const c_char {
    prompt(unsafe { text(value) }, true)
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_prompt_confirm(value: *const c_char) -> i32 {
    if !value.is_null() {
        output(&format!("{} [y/N] ", unsafe { text(value) }));
    }
    matches!(plain_line().as_bytes().first(), Some(b'y' | b'Y')) as i32
}

fn number(line: &str) -> i64 {
    let line = CString::new(line).unwrap_or_default();
    unsafe { libc::strtoll(line.as_ptr(), std::ptr::null_mut(), 10) as i64 }
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_prompt_select(
    value: *const c_char,
    choices: *const abi::StringList,
) -> i32 {
    let Some(choices) = (unsafe { choices.as_ref() }) else {
        return -1;
    };
    if choices.len <= 0 || choices.len > choices.cap || choices.data.is_null() {
        return -1;
    }
    if !value.is_null() {
        output(&format!("{}\n", unsafe { text(value) }));
    }
    for i in 0..choices.len as usize {
        output(&format!("  {}. {}\n", i + 1, unsafe {
            text(*choices.data.add(i))
        }));
    }
    output(&format!("Enter choice (1-{}): ", choices.len));
    let choice = number(&plain_line());
    if choice < 1 || choice > choices.len {
        -1
    } else {
        (choice - 1) as i32
    }
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_prompt_int(value: *const c_char, min: i64, max: i64) -> i64 {
    if !value.is_null() {
        output(&format!("{} ({min}-{max}): ", unsafe { text(value) }));
    }
    let line = plain_line();
    if line.is_empty() {
        min
    } else {
        let number = number(&line);
        if number < min {
            min
        } else if number > max {
            max
        } else {
            number
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_preserves_utf8_boundaries_and_limit() {
        let mut line = Line::default();
        line.insert('é');
        line.insert('x');
        line.left();
        line.insert('λ');
        assert_eq!(line.value, "éλx");
        line.backspace();
        assert_eq!(line.value, "éx");
        line.delete();
        assert_eq!(line.value, "é");
        for _ in 0..5000 {
            line.insert('x');
        }
        assert_eq!(line.value.len(), 4095);
    }
}
