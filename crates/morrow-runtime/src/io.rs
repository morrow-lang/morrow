//! Bounded text I/O and Unix process environment, preserving the native ABI.
use crate::abi::{self, StringList};
use std::ffi::{CStr, OsStr, c_char};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::IntoRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Mutex;

pub(crate) const TEXT_LIMIT: usize = 16 * 1024 * 1024;

/// Read at most the specified number of bytes before requiring a CString terminator.
pub(crate) unsafe fn bounded_bytes<'a>(
    value: *const c_char,
    limit: usize,
) -> Result<&'a [u8], i64> {
    if value.is_null() {
        return Err(1);
    }
    let length = unsafe { libc::strnlen(value, limit + 1) };
    if length > limit {
        return Err(2);
    }
    Ok(unsafe { std::slice::from_raw_parts(value.cast(), length) })
}

pub(crate) fn valid_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

fn result(value: Result<i64, i64>) -> i64 {
    match value {
        Ok(value) => abi::result_ok(value),
        Err(error) => abi::result_err(error),
    }
}

unsafe fn path<'a>(name: *const c_char) -> Option<&'a Path> {
    if name.is_null() {
        None
    } else {
        Some(Path::new(OsStr::from_bytes(
            unsafe { CStr::from_ptr(name) }.to_bytes(),
        )))
    }
}

/// Close exactly once; an earlier read/write failure takes precedence over close failure.
fn close(file: File, value: Result<i64, i64>) -> Result<i64, i64> {
    let status = unsafe { libc::close(file.into_raw_fd()) };
    if status != 0 && value.is_ok() {
        Err(3)
    } else {
        value
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_read_file(name: *const c_char) -> i64 {
    let Some(name) = (unsafe { path(name) }) else {
        return abi::result_err(3);
    };
    let Ok(mut file) = File::open(name) else {
        return abi::result_err(1);
    };
    let value = (|| {
        let length = file.seek(SeekFrom::End(0)).map_err(|_| 3)?;
        if length > TEXT_LIMIT as u64 {
            return Err(3);
        }
        file.seek(SeekFrom::Start(0)).map_err(|_| 3)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length as usize + 1)
            .map_err(|_| 4)?;
        (&mut file)
            .take(length + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| 3)?;
        if bytes.len() != length as usize || !valid_text(&bytes) {
            return Err(3);
        }
        Ok(abi::bytes(&bytes) as i64)
    })();
    result(close(file, value))
}

unsafe fn write_file(name: *const c_char, contents: *const c_char, append: bool) -> i64 {
    let Some(name) = (unsafe { path(name) }) else {
        return abi::result_err(3);
    };
    let Ok(bytes) = (unsafe { bounded_bytes(contents, TEXT_LIMIT) }) else {
        return abi::result_err(3);
    };
    if !valid_text(bytes) {
        return abi::result_err(3);
    }
    let Ok(mut file) = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(!append)
        .append(append)
        .open(name)
    else {
        return abi::result_err(2);
    };
    let value = file
        .write_all(bytes)
        .map(|()| bytes.len() as i64)
        .map_err(|_| 3);
    result(close(file, value))
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_write_file(name: *const c_char, contents: *const c_char) -> i64 {
    unsafe { write_file(name, contents, false) }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_append_file(name: *const c_char, contents: *const c_char) -> i64 {
    unsafe { write_file(name, contents, true) }
}

/// Exact, fallible stderr output with only the caller thread's SIGPIPE masked.
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_write_stderr(text: *const c_char) -> i64 {
    let bytes = match unsafe { bounded_bytes(text, TEXT_LIMIT) } {
        Ok(bytes) => bytes,
        Err(code) => return abi::result_err(code),
    };
    if !valid_text(bytes) {
        return abi::result_err(1);
    }
    if bytes.is_empty() {
        return abi::result_ok(0);
    }
    result(stderr_output(bytes).map(|()| 0))
}

fn pending_pipe() -> Result<bool, i64> {
    let mut pending = unsafe { std::mem::zeroed::<libc::sigset_t>() };
    if unsafe { libc::sigpending(&mut pending) } != 0 {
        return Err(3);
    }
    match unsafe { libc::sigismember(&pending, libc::SIGPIPE) } {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(3),
    }
}

fn stderr_output(bytes: &[u8]) -> Result<(), i64> {
    unsafe {
        let mut set = std::mem::zeroed::<libc::sigset_t>();
        let mut old = std::mem::zeroed::<libc::sigset_t>();
        if libc::sigemptyset(&mut set) != 0
            || libc::sigaddset(&mut set, libc::SIGPIPE) != 0
            || libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old) != 0
        {
            return Err(3);
        }
        let value = (|| {
            let pending = pending_pipe()?;
            let mut offset = 0;
            let mut broken = false;
            for _ in 0..65536 {
                let count = (bytes.len() - offset).min(16384);
                if count == 0 {
                    break;
                }
                let written = libc::write(2, bytes.as_ptr().add(offset).cast(), count);
                if written > 0 {
                    offset += written as usize;
                } else if written == 0 {
                    break;
                } else {
                    let error = std::io::Error::last_os_error().raw_os_error();
                    if error == Some(libc::EINTR) {
                        continue;
                    }
                    broken = error == Some(libc::EPIPE);
                    break;
                }
            }
            if broken && !pending && pending_pipe()? {
                #[cfg(target_os = "macos")]
                {
                    let mut signal = 0;
                    if libc::sigwait(&set, &mut signal) != 0 || signal != libc::SIGPIPE {
                        return Err(3);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let zero = libc::timespec {
                        tv_sec: 0,
                        tv_nsec: 0,
                    };
                    let signal = libc::sigtimedwait(&set, std::ptr::null_mut(), &zero);
                    if signal != libc::SIGPIPE
                        && !(signal < 0
                            && std::io::Error::last_os_error().raw_os_error() == Some(libc::EAGAIN))
                    {
                        return Err(3);
                    }
                }
            }
            if offset == bytes.len() {
                Ok(())
            } else {
                Err(3)
            }
        })();
        if libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) != 0 {
            Err(3)
        } else {
            value
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_file_exists(name: *const c_char) -> i64 {
    unsafe { path(name) }.is_some_and(|name| File::open(name).is_ok()) as i64
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_delete_file(name: *const c_char) -> i64 {
    if !name.is_null() && unsafe { libc::remove(name) } == 0 {
        abi::result_ok(0)
    } else {
        abi::result_err(1)
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_file_size(name: *const c_char) -> i64 {
    let Some(name) = (unsafe { path(name) }) else {
        return abi::result_err(1);
    };
    let Ok(mut file) = File::open(name) else {
        return abi::result_err(1);
    };
    let size = file
        .seek(SeekFrom::End(0))
        .ok()
        .and_then(|n| i64::try_from(n).ok())
        .ok_or(3);
    result(close(file, size))
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_is_dir(name: *const c_char) -> i64 {
    unsafe { path(name) }.is_some_and(Path::is_dir) as i64
}

fn directory_error(error: std::io::Error, opening: bool) -> i64 {
    match error.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => 2,
        Some(libc::ENOENT) if opening => 1,
        Some(libc::ENOTDIR) if opening => 5,
        _ => 3,
    }
}

/// Enumerate through POSIX so a closedir failure remains observable; std::fs::ReadDir
/// intentionally discards that close result. The close callback also supplies a
/// deterministic failure oracle without replacing real filesystem enumeration.
unsafe fn directory_names(
    name: *const c_char,
    finish: impl FnOnce(*mut libc::DIR) -> std::io::Result<()>,
) -> Result<Vec<Vec<u8>>, i64> {
    if name.is_null() {
        return Err(3);
    }
    let directory = unsafe { libc::opendir(name) };
    if directory.is_null() {
        return Err(directory_error(std::io::Error::last_os_error(), true));
    }
    let mut names = Vec::new();
    let mut error = Some(3);
    for _ in 0..1048579 {
        #[cfg(target_os = "macos")]
        unsafe {
            *libc::__error() = 0;
        }
        #[cfg(not(target_os = "macos"))]
        unsafe {
            *libc::__errno_location() = 0;
        }
        let entry = unsafe { libc::readdir(directory) };
        if entry.is_null() {
            let last = std::io::Error::last_os_error();
            error = if last.raw_os_error() == Some(0) {
                None
            } else {
                Some(directory_error(last, false))
            };
            break;
        }
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        if names.len() == 1048576 {
            break;
        }
        names.push(bytes.to_vec());
    }
    if let Err(failure) = finish(directory)
        && error.is_none()
    {
        error = Some(directory_error(failure, false));
    }
    match error {
        Some(error) => Err(error),
        None => Ok(names),
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_read_dir_result(name: *const c_char) -> i64 {
    let names = unsafe {
        directory_names(name, |directory| {
            if libc::closedir(directory) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        })
    };
    let names = match names {
        Ok(names) => names,
        Err(error) => return abi::result_err(error),
    };
    // All native strings must remain rooted while subsequent entries allocate.
    let mut values = vec![0_i64; names.len()];
    let _root = unsafe { crate::memory::root_range(values.as_ptr().cast(), values.len()) };
    for (value, name) in values.iter_mut().zip(&names) {
        *value = abi::bytes(name) as i64;
    }
    abi::result_ok(abi::list(&values) as i64)
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_list_dir(name: *const c_char) -> *mut StringList {
    let value = unsafe { morrow_read_dir_result(name) };
    let result = unsafe { &*(value as *const abi::ResultValue) };
    if result.tag == 0 {
        result.value as *mut StringList
    } else {
        std::ptr::null_mut()
    }
}

static ARGS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_set_args(count: libc::c_int, args: *const *const c_char) {
    let mut values = ARGS.lock().unwrap_or_else(|error| error.into_inner());
    values.clear();
    if count > 0 && !args.is_null() {
        for i in 0..count as usize {
            let arg = unsafe { *args.add(i) };
            if !arg.is_null() {
                values.push(unsafe { CStr::from_ptr(arg) }.to_bytes().to_vec());
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_args_count() -> i64 {
    ARGS.lock().unwrap_or_else(|error| error.into_inner()).len() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_arg(index: i64) -> *const c_char {
    let args = ARGS.lock().unwrap_or_else(|error| error.into_inner());
    abi::bytes(
        usize::try_from(index)
            .ok()
            .and_then(|index| args.get(index))
            .map_or(&[], Vec::as_slice),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_args() -> *mut StringList {
    let args = ARGS.lock().unwrap_or_else(|error| error.into_inner());
    let mut values = vec![0_i64; args.len()];
    let _root = unsafe { crate::memory::root_range(values.as_ptr().cast(), values.len()) };
    for (value, arg) in values.iter_mut().zip(args.iter()) {
        *value = abi::bytes(arg) as i64;
    }
    abi::list(&values).cast()
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_exit(code: i64) {
    std::process::exit(code as i32);
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_getenv(name: *const c_char) -> *const c_char {
    if name.is_null() {
        return abi::string("");
    }
    let value = unsafe { libc::getenv(name) };
    if value.is_null() {
        abi::string("")
    } else {
        abi::bytes(unsafe { CStr::from_ptr(value) }.to_bytes())
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_setenv(name: *const c_char, value: *const c_char) -> i64 {
    if name.is_null() || value.is_null() {
        -1
    } else {
        unsafe { libc::setenv(name, value, 1) as i64 }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_cwd() -> *const c_char {
    let mut buffer = [0_u8; 4096];
    if unsafe { libc::getcwd(buffer.as_mut_ptr().cast(), buffer.len()) }.is_null() {
        abi::string("")
    } else {
        abi::bytes(unsafe { CStr::from_ptr(buffer.as_ptr().cast()) }.to_bytes())
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_chdir(name: *const c_char) -> i64 {
    if name.is_null() {
        -1
    } else {
        unsafe { libc::chdir(name) as i64 }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_hostname() -> *const c_char {
    let mut buffer = [0_u8; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return abi::string("");
    }
    buffer[255] = 0;
    abi::bytes(unsafe { CStr::from_ptr(buffer.as_ptr().cast()) }.to_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_user() -> *const c_char {
    unsafe {
        if libc::getenv(c"USER".as_ptr()).is_null() {
            morrow_getenv(c"LOGNAME".as_ptr())
        } else {
            morrow_getenv(c"USER".as_ptr())
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_home() -> *const c_char {
    unsafe { morrow_getenv(c"HOME".as_ptr()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString, c_char};
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn directory_close_failure_cannot_publish_partial_success() {
        let value = unsafe {
            directory_names(c".".as_ptr(), |directory| {
                assert_eq!(libc::closedir(directory), 0);
                Err(std::io::Error::from_raw_os_error(libc::EIO))
            })
        };
        assert_eq!(value, Err(3));
    }

    #[test]
    fn file_roundtrip_empty_unicode_and_append() {
        let path = std::env::temp_dir().join(format!("morrow-rust-file-{}", std::process::id()));
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        unsafe {
            assert_eq!(
                decode(morrow_write_file(name.as_ptr(), c"".as_ptr())),
                Ok(0)
            );
            let value = decode(morrow_read_file(name.as_ptr())).unwrap();
            assert_eq!(CStr::from_ptr(value as *const c_char).to_bytes(), b"");
            assert_eq!(
                decode(morrow_append_file(name.as_ptr(), c"hé\n".as_ptr())),
                Ok(4)
            );
            assert_eq!(decode(morrow_file_size(name.as_ptr())), Ok(4));
            let value = decode(morrow_read_file(name.as_ptr())).unwrap();
            assert_eq!(
                CStr::from_ptr(value as *const c_char).to_bytes(),
                "hé\n".as_bytes()
            );
            assert_eq!(decode(morrow_delete_file(name.as_ptr())), Ok(0));
            assert_eq!(decode(morrow_read_file(name.as_ptr())), Err(1));
        }
    }

    #[test]
    fn file_rejects_binary_and_oversize_without_truncating() {
        let path = std::env::temp_dir().join(format!("morrow-rust-binary-{}", std::process::id()));
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        for bytes in [b"a\0b".as_slice(), b"\xff", b"\xed\xa0\x80"] {
            std::fs::write(&path, bytes).unwrap();
            unsafe {
                assert_eq!(decode(morrow_read_file(name.as_ptr())), Err(3));
            }
        }
        let input = CString::new(vec![b'x'; TEXT_LIMIT + 1]).unwrap();
        std::fs::write(&path, b"keep").unwrap();
        unsafe {
            assert_eq!(
                decode(morrow_write_file(name.as_ptr(), input.as_ptr())),
                Err(3)
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len((TEXT_LIMIT + 1) as u64).unwrap();
        unsafe {
            assert_eq!(decode(morrow_read_file(name.as_ptr())), Err(3));
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stderr_validation_and_empty_write_do_not_require_descriptor() {
        unsafe {
            assert_eq!(decode(morrow_write_stderr(std::ptr::null())), Err(1));
            assert_eq!(decode(morrow_write_stderr(c"".as_ptr())), Ok(0));
            let invalid = [0xff_u8, 0];
            assert_eq!(decode(morrow_write_stderr(invalid.as_ptr().cast())), Err(1));
            let long = CString::new(vec![b'x'; TEXT_LIMIT + 1]).unwrap();
            assert_eq!(decode(morrow_write_stderr(long.as_ptr())), Err(2));
        }
    }

    unsafe fn decode(value: i64) -> Result<i64, i64> {
        let tag = unsafe { *(value as *const i32) };
        let payload = unsafe { *((value as *const u8).add(8).cast::<i64>()) };
        if tag == 0 { Ok(payload) } else { Err(payload) }
    }
}
