//! Literal bounded process capture with retained-child process-group ownership.
use crate::abi::{self, StringList};
use crate::io::{bounded_bytes, valid_text};
use std::ffi::{CStr, CString, OsStr, c_char};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct ExecResult {
    pub exit_code: i64,
    pub stdout_str: *const c_char,
    pub stderr_str: *const c_char,
}

fn native_result(code: i64, output: &[u8], error: &[u8]) -> *mut ExecResult {
    let mut values = [0_usize; 2];
    let _root = unsafe { crate::memory::root_range(values.as_ptr(), 2) };
    values[0] = abi::bytes(output) as usize;
    values[1] = abi::bytes(error) as usize;
    abi::owned(
        ExecResult {
            exit_code: code,
            stdout_str: values[0] as *const c_char,
            stderr_str: values[1] as *const c_char,
        },
        0,
    )
}

unsafe fn arguments(args: *const StringList) -> Result<Vec<CString>, i64> {
    let Some(args) = (unsafe { args.as_ref() }) else {
        return Err(1);
    };
    if args.len < 1 || args.len > 4096 || args.cap < args.len || args.data.is_null() {
        return Err(1);
    }
    let mut remaining = 1024 * 1024;
    let mut values = Vec::with_capacity(args.len as usize);
    for i in 0..args.len as usize {
        if remaining == 0 {
            return Err(1);
        }
        let bytes = unsafe { bounded_bytes(*args.data.add(i), remaining - 1) }.map_err(|_| 1)?;
        if (i == 0 && bytes.is_empty()) || !valid_text(bytes) {
            return Err(1);
        }
        remaining -= bytes.len() + 1;
        values.push(CString::new(bytes).map_err(|_| 1)?);
    }
    Ok(values)
}

fn errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

fn move_fd(raw: i32) -> Result<OwnedFd, i64> {
    let moved = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
    let closed = unsafe { libc::close(raw) };
    if moved < 0 {
        return Err(5);
    }
    let fd = unsafe { OwnedFd::from_raw_fd(moved) };
    if closed != 0 { Err(5) } else { Ok(fd) }
}

fn pipe() -> Result<(OwnedFd, OwnedFd), i64> {
    let mut descriptors = [-1; 2];
    if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
        return Err(5);
    }
    let reader = move_fd(descriptors[0]);
    let writer = move_fd(descriptors[1]);
    let reader = reader?;
    let writer = writer?;
    let flags = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(5);
    }
    Ok((reader, writer))
}

struct SpawnSetup {
    actions: libc::posix_spawn_file_actions_t,
    attributes: libc::posix_spawnattr_t,
}

impl SpawnSetup {
    fn new(input: &OwnedFd, pipes: &[(&OwnedFd, &OwnedFd); 2]) -> Result<Self, i64> {
        unsafe {
            let mut actions = std::mem::zeroed();
            let mut attributes = std::mem::zeroed();
            if libc::posix_spawn_file_actions_init(&mut actions) != 0 {
                return Err(5);
            }
            if libc::posix_spawnattr_init(&mut attributes) != 0 {
                libc::posix_spawn_file_actions_destroy(&mut actions);
                return Err(5);
            }
            let mut setup = Self {
                actions,
                attributes,
            };
            let sources = [
                input.as_raw_fd(),
                pipes[0].1.as_raw_fd(),
                pipes[1].1.as_raw_fd(),
            ];
            for (target, source) in sources.iter().enumerate() {
                if libc::posix_spawn_file_actions_adddup2(
                    &mut setup.actions,
                    *source,
                    target as i32,
                ) != 0
                {
                    return Err(5);
                }
            }
            for source in [
                input.as_raw_fd(),
                pipes[0].0.as_raw_fd(),
                pipes[0].1.as_raw_fd(),
                pipes[1].0.as_raw_fd(),
                pipes[1].1.as_raw_fd(),
            ] {
                if libc::posix_spawn_file_actions_addclose(&mut setup.actions, source) != 0 {
                    return Err(5);
                }
            }
            let mut mask = std::mem::zeroed();
            let mut defaults = std::mem::zeroed();
            if libc::sigemptyset(&mut mask) != 0
                || libc::sigfillset(&mut defaults) != 0
                || libc::sigdelset(&mut defaults, libc::SIGKILL) != 0
                || libc::sigdelset(&mut defaults, libc::SIGSTOP) != 0
            {
                return Err(5);
            }
            if libc::posix_spawnattr_setpgroup(&mut setup.attributes, 0) != 0
                || libc::posix_spawnattr_setsigmask(&mut setup.attributes, &mask) != 0
                || libc::posix_spawnattr_setsigdefault(&mut setup.attributes, &defaults) != 0
                || libc::posix_spawnattr_setflags(
                    &mut setup.attributes,
                    (libc::POSIX_SPAWN_SETPGROUP
                        | libc::POSIX_SPAWN_SETSIGMASK
                        | libc::POSIX_SPAWN_SETSIGDEF) as i16,
                ) != 0
            {
                return Err(5);
            }
            Ok(setup)
        }
    }
}

impl Drop for SpawnSetup {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawnattr_destroy(&mut self.attributes);
            libc::posix_spawn_file_actions_destroy(&mut self.actions);
        }
    }
}

struct Child {
    pid: libc::pid_t,
    retained: bool,
    finished: bool,
    code: i64,
}

impl Child {
    fn observe(&mut self) -> Result<(), i64> {
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                self.pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            let error = errno();
            if error == libc::EINTR {
                return Ok(());
            }
            if error == libc::ECHILD {
                self.retained = false;
            }
            return Err(5);
        }
        if unsafe { info.si_pid() } == 0 {
            return Ok(());
        }
        self.finished = true;
        match info.si_code {
            libc::CLD_EXITED => {
                self.code = unsafe { info.si_status() } as i64;
                Ok(())
            }
            libc::CLD_KILLED | libc::CLD_DUMPED => Err(7),
            _ => Err(5),
        }
    }

    fn zombie_only(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn proc_listpids(kind: u32, info: u32, buffer: *mut libc::c_void, size: i32)
                -> i32;
            }
            let mut members = [0_i32; 2];
            self.finished
                && unsafe { proc_listpids(2, self.pid as u32, members.as_mut_ptr().cast(), 8) } == 4
                && members[0] == self.pid
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn cleanup(&mut self) -> Result<(), i64> {
        if !self.retained {
            return Ok(());
        }
        let mut value = Ok(());
        if unsafe { libc::kill(-self.pid, libc::SIGKILL) } != 0 {
            let error = errno();
            if error != libc::ESRCH && !(error == libc::EPERM && self.zombie_only()) {
                value = Err(5);
            }
        }
        loop {
            let reaped = unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) };
            if reaped == self.pid {
                break;
            }
            if reaped < 0 && errno() == libc::EINTR {
                continue;
            }
            value = Err(5);
            break;
        }
        self.retained = false;
        value
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn remaining(deadline: Instant) -> Result<Duration, i64> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or(3)
}

fn spawn(args: &[CString], setup: &SpawnSetup, deadline: Instant) -> Result<Child, i64> {
    let mut argv: Vec<*mut c_char> = args.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let environment: Vec<CString> = std::env::vars_os()
        .map(|(name, value)| {
            let mut bytes = name.as_bytes().to_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(value.as_bytes());
            CString::new(bytes).expect("OS environment is NUL-free")
        })
        .collect();
    let mut env: Vec<*mut c_char> = environment
        .iter()
        .map(|value| value.as_ptr().cast_mut())
        .collect();
    env.push(std::ptr::null_mut());
    let executable = args[0].as_bytes();
    let attempt = |candidate: &CStr| -> Result<Option<Child>, i64> {
        remaining(deadline)?;
        let mut pid = -1;
        let status = unsafe {
            libc::posix_spawn(
                &mut pid,
                candidate.as_ptr(),
                &setup.actions,
                &setup.attributes,
                argv.as_ptr(),
                env.as_ptr(),
            )
        };
        if status == 0 {
            return Ok(Some(Child {
                pid,
                retained: true,
                finished: false,
                code: -1,
            }));
        }
        if !matches!(status, libc::ENOENT | libc::ENOTDIR | libc::EACCES) {
            return Err(2);
        }
        Ok(None)
    };
    if executable.contains(&b'/') {
        return attempt(&args[0])?.ok_or(2);
    }
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
    let bytes = path.as_bytes();
    if bytes.len() > 1024 * 1024 || bytes.split(|b| *b == b':').count() > 4096 {
        return Err(1);
    }
    // Retain one candidate at a time: a large argv[0] must not be copied once
    // per PATH component before the first bounded spawn attempt.
    for component in bytes.split(|b| *b == b':') {
        let mut name = component.to_vec();
        if !name.is_empty() {
            name.push(b'/');
        }
        name.extend_from_slice(executable);
        let name = CString::new(name).map_err(|_| 1)?;
        if let Some(child) = attempt(&name)? {
            return Ok(child);
        }
    }
    Err(2)
}

struct Stream {
    fd: Option<OwnedFd>,
    bytes: Vec<u8>,
}

impl Stream {
    fn read(&mut self, limit: usize, deadline: Instant) -> Result<bool, i64> {
        let Some(fd) = &self.fd else { return Ok(false) };
        let mut bytes = [0_u8; 4096];
        let count = (limit - self.bytes.len())
            .saturating_add(1)
            .min(bytes.len());
        let read = unsafe { libc::read(fd.as_raw_fd(), bytes.as_mut_ptr().cast(), count) };
        remaining(deadline)?;
        if read > 0 {
            if read as usize > limit - self.bytes.len() {
                return Err(4);
            }
            self.bytes.try_reserve(read as usize).map_err(|_| 5)?;
            self.bytes.extend_from_slice(&bytes[..read as usize]);
            Ok(true)
        } else if read == 0 {
            self.close()?;
            Ok(false)
        } else if matches!(errno(), libc::EAGAIN | libc::EINTR) {
            Ok(false)
        } else {
            Err(5)
        }
    }

    fn close(&mut self) -> Result<(), i64> {
        if let Some(fd) = self.fd.take()
            && unsafe { libc::close(fd.into_raw_fd()) } != 0
        {
            return Err(5);
        }
        Ok(())
    }
}

fn capture(args: &[CString], timeout: i64, limit: usize) -> Result<*mut ExecResult, i64> {
    let mut policy = unsafe { std::mem::zeroed::<libc::sigaction>() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut policy) } != 0 {
        return Err(5);
    }
    if policy.sa_sigaction == libc::SIG_IGN || policy.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(1);
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(timeout as u64))
        .ok_or(5)?;
    let raw = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if raw < 0 {
        return Err(5);
    }
    let input = move_fd(raw)?;
    let (output_read, output_write) = pipe()?;
    let (error_read, error_write) = pipe()?;
    let setup = SpawnSetup::new(
        &input,
        &[(&output_read, &output_write), (&error_read, &error_write)],
    )?;
    let mut child = spawn(args, &setup, deadline)?;
    drop(setup);
    drop(input);
    drop(output_write);
    drop(error_write);
    let mut streams = [
        Stream {
            fd: Some(output_read),
            bytes: Vec::new(),
        },
        Stream {
            fd: Some(error_read),
            bytes: Vec::new(),
        },
    ];
    let value: Result<(), i64> = (|| {
        while !child.finished {
            remaining(deadline)?;
            child.observe()?;
            if child.finished {
                break;
            }
            for stream in &mut streams {
                stream.read(limit, deadline)?;
            }
            let mut polls = streams.each_ref().map(|stream| libc::pollfd {
                fd: stream.fd.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                events: libc::POLLIN,
                revents: 0,
            });
            let wait = remaining(deadline)?.as_millis().min(10) as i32;
            let status = unsafe { libc::poll(polls.as_mut_ptr(), 2, wait) };
            if (status < 0 && errno() != libc::EINTR)
                || polls.iter().any(|p| p.revents & libc::POLLNVAL != 0)
            {
                return Err(5);
            }
        }
        Ok(())
    })();
    let cleanup = child.cleanup();
    value?;
    cleanup?;
    let mut active = [true; 2];
    for _ in 0..=limit + 1 {
        for i in 0..2 {
            if active[i] {
                active[i] = streams[i].read(limit, deadline)?;
            }
        }
        if !active[0] && !active[1] {
            break;
        }
    }
    for stream in &mut streams {
        stream.close()?;
        if !valid_text(&stream.bytes) {
            return Err(6);
        }
    }
    Ok(native_result(
        child.code,
        &streams[0].bytes,
        &streams[1].bytes,
    ))
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_exec_args_bounded(
    args: *const StringList,
    timeout: i64,
    limit: i64,
) -> i64 {
    if !(1..=600000).contains(&timeout) || !(0..=16 * 1024 * 1024).contains(&limit) {
        return abi::result_err(1);
    }
    let value = unsafe { arguments(args) }.and_then(|args| capture(&args, timeout, limit as usize));
    match value {
        Ok(value) => abi::result_ok(value as i64),
        Err(error) => abi::result_err(error),
    }
}

fn legacy_command(command: &mut Command) -> *mut ExecResult {
    // Seekable anonymous files retain the legacy nonblocking-on-descendants contract.
    fn temporary() -> std::io::Result<std::fs::File> {
        let stream = unsafe { libc::tmpfile() };
        if stream.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { libc::fcntl(libc::fileno(stream), libc::F_DUPFD_CLOEXEC, 3) };
        unsafe {
            libc::fclose(stream);
        }
        if fd < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(unsafe { std::fs::File::from_raw_fd(fd) })
        }
    }
    let (Ok(mut output), Ok(mut error)) = (temporary(), temporary()) else {
        return native_result(-1, b"", b"Failed to create process capture files");
    };
    let (Ok(output_child), Ok(error_child)) = (output.try_clone(), error.try_clone()) else {
        return native_result(-1, b"", b"Failed to create process capture files");
    };
    let status = command
        .stdout(Stdio::from(output_child))
        .stderr(Stdio::from(error_child))
        .status();
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(libc::EIO);
            let message = unsafe { CStr::from_ptr(libc::strerror(code)) };
            return native_result(-1, b"", message.to_bytes());
        }
    };
    fn contents(file: &mut std::fs::File) -> Vec<u8> {
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(0)).is_ok() {
            let _ = file.read_to_end(&mut bytes);
        }
        if let Some(nul) = bytes.iter().position(|b| *b == 0) {
            bytes.truncate(nul);
        }
        bytes
    }
    native_result(
        status.code().map_or(-1, i64::from),
        &contents(&mut output),
        &contents(&mut error),
    )
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_exec(command: *const c_char) -> *mut ExecResult {
    if command.is_null() {
        return native_result(-1, b"", b"Failed to execute command");
    }
    legacy_command(Command::new("/bin/sh").arg("-c").arg(OsStr::from_bytes(
        unsafe { CStr::from_ptr(command) }.to_bytes(),
    )))
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_exec_args(args: *const StringList) -> *mut ExecResult {
    let Ok(args) = (unsafe { arguments(args) }) else {
        return native_result(-1, b"", b"No command specified");
    };
    let mut command = Command::new(OsStr::from_bytes(args[0].as_bytes()));
    command.args(
        args[1..]
            .iter()
            .map(|arg| OsStr::from_bytes(arg.as_bytes())),
    );
    legacy_command(&mut command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execute(args: &[&str], timeout: i64, limit: i64) -> Result<(i64, String, String), i64> {
        unsafe {
            let args = abi::strings(args);
            let value = morrow_exec_args_bounded(args, timeout, limit);
            let result = &*(value as *const abi::ResultValue);
            if result.tag != 0 {
                return Err(result.value);
            }
            let result = &*(result.value as *const ExecResult);
            Ok((
                result.exit_code,
                abi::text(result.stdout_str).to_owned(),
                abi::text(result.stderr_str).to_owned(),
            ))
        }
    }

    #[test]
    fn bounded_process_keeps_literal_arguments_streams_and_exit_status() {
        assert_eq!(
            execute(
                &[
                    "/bin/sh",
                    "-c",
                    "printf '%s' \"$1\"; printf error >&2; exit 19",
                    "sh",
                    "$(not executed); hé"
                ],
                1000,
                100
            ),
            Ok((19, "$(not executed); hé".into(), "error".into()))
        );
    }

    #[test]
    fn bounded_process_enforces_independent_caps_and_text_validation() {
        assert_eq!(
            execute(&["/bin/sh", "-c", "printf ab; printf cd >&2"], 1000, 2),
            Ok((0, "ab".into(), "cd".into()))
        );
        assert_eq!(execute(&["/bin/sh", "-c", "printf abc"], 1000, 2), Err(4));
        assert_eq!(
            execute(&["/bin/sh", "-c", "printf '\\377'"], 1000, 20),
            Err(6)
        );
        assert_eq!(
            execute(&["/bin/sh", "-c", "printf '\\000'"], 1000, 20),
            Err(6)
        );
    }

    #[test]
    fn bounded_process_distinguishes_invalid_spawn_timeout_and_signal() {
        assert_eq!(execute(&[], 1000, 20), Err(1));
        assert_eq!(execute(&["/bin/true"], 0, 20), Err(1));
        assert_eq!(execute(&["/bin/true"], 1000, -1), Err(1));
        assert_eq!(execute(&["/morrow-not-a-command"], 1000, 20), Err(2));
        assert_eq!(execute(&["/bin/sleep", "2"], 20, 20), Err(3));
        assert_eq!(
            execute(&["/bin/sh", "-c", "kill -TERM $$"], 1000, 20),
            Err(7)
        );
        assert_eq!(
            execute(&["/bin/sh", "-c", "exit 0"], 1000, 0),
            Ok((0, "".into(), "".into()))
        );
    }

    #[test]
    fn successful_direct_child_does_not_wait_for_inherited_descendant_streams() {
        let start = std::time::Instant::now();
        assert_eq!(
            execute(&["/bin/sh", "-c", "sleep 5 & printf done"], 500, 100),
            Ok((0, "done".into(), "".into()))
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
