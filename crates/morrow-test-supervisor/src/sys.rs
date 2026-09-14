//! POSIX boundary. Owned descriptors and validated buffers outlive every syscall.
use super::Error;
use std::{
    ffi::{CStr, CString},
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    sync::atomic::{AtomicI32, Ordering},
};
static INTERRUPTED: AtomicI32 = AtomicI32::new(0);
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed) != 0
}
extern "C" fn interrupt(signal: i32) {
    let _ = INTERRUPTED.compare_exchange(0, signal, Ordering::Relaxed, Ordering::Relaxed);
}
pub fn errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}
pub fn fd(slot: &Option<OwnedFd>) -> i32 {
    slot.as_ref().map_or(-1, AsRawFd::as_raw_fd)
}
pub fn close(slot: &mut Option<OwnedFd>) -> bool {
    slot.take()
        .is_none_or(|owned| unsafe { libc::close(owned.into_raw_fd()) == 0 })
}
fn own(raw: i32) -> Option<OwnedFd> {
    if raw < 0 {
        None
    } else {
        Some(unsafe { OwnedFd::from_raw_fd(raw) })
    }
}
fn moved(raw: i32) -> Option<OwnedFd> {
    if raw < 0 {
        return None;
    }
    let next = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
    if unsafe { libc::close(raw) } != 0 {
        if next >= 0 {
            unsafe { libc::close(next) };
        }
        return None;
    }
    own(next)
}
pub fn open(path: &CStr, flags: i32, mode: libc::mode_t) -> Option<OwnedFd> {
    moved(unsafe { libc::open(path.as_ptr(), flags, mode as libc::c_uint) })
}
pub fn open_at(parent: i32, path: &CStr, flags: i32, mode: libc::mode_t) -> Option<OwnedFd> {
    own(unsafe { libc::openat(parent, path.as_ptr(), flags, mode as libc::c_uint) })
}
pub fn stat(fd: i32) -> Option<libc::stat> {
    let mut value = MaybeUninit::uninit();
    if unsafe { libc::fstat(fd, value.as_mut_ptr()) } == 0 {
        Some(unsafe { value.assume_init() })
    } else {
        None
    }
}
pub fn stat_at(fd: i32, path: &CStr) -> Option<libc::stat> {
    let mut value = MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            fd,
            path.as_ptr(),
            value.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        Some(unsafe { value.assume_init() })
    } else {
        None
    }
}
pub fn uid() -> libc::uid_t {
    unsafe { libc::geteuid() }
}
pub fn mkdir_at(fd: i32, name: &CStr) -> bool {
    unsafe { libc::mkdirat(fd, name.as_ptr(), 0o700) == 0 }
}
pub fn unlink_at(fd: i32, name: &CStr, directory: bool) -> bool {
    unsafe {
        libc::unlinkat(
            fd,
            name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        ) == 0
    }
}
pub fn nonblocking(fd: i32) -> bool {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
    }
}
pub fn pipe() -> Option<(OwnedFd, OwnedFd)> {
    let mut pair = [-1; 2];
    if unsafe { libc::pipe(pair.as_mut_ptr()) } != 0 {
        return None;
    }
    let reader = moved(pair[0]);
    let writer = moved(pair[1]);
    let reader = reader?;
    let writer = writer?;
    if !nonblocking(reader.as_raw_fd()) {
        return None;
    }
    Some((reader, writer))
}
pub fn read(fd: i32, buffer: &mut [u8]) -> isize {
    unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) }
}
pub fn pread(fd: i32, buffer: &mut [u8], offset: usize) -> isize {
    unsafe {
        libc::pread(
            fd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            offset as libc::off_t,
        )
    }
}
pub fn write(fd: i32, buffer: &[u8]) -> isize {
    unsafe { libc::write(fd, buffer.as_ptr().cast(), buffer.len()) }
}
pub fn poll(fds: &mut [libc::pollfd], timeout: i32) -> i32 {
    unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) }
}
pub fn protocol_channels() -> bool {
    // SIGPIPE must report EPIPE so publication failure cannot interrupt cleanup.
    if unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) } == libc::SIG_ERR {
        return false;
    }
    for (fd, access) in [(0, libc::O_RDONLY), (1, libc::O_WRONLY)] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        let Some(info) = stat(fd) else {
            return false;
        };
        if flags < 0
            || flags & libc::O_ACCMODE != access
            || info.st_mode & libc::S_IFMT != libc::S_IFIFO
        {
            return false;
        }
    }
    true
}
pub fn signals() -> bool {
    // The process is single-threaded. The handler only touches a lock-free atomic.
    unsafe {
        let mut current: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut current) != 0
            || current.sa_sigaction == libc::SIG_IGN
            || current.sa_flags & libc::SA_NOCLDWAIT != 0
        {
            return false;
        }
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = interrupt as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        let mut unblock: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut unblock);
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0
                || libc::sigaddset(&mut unblock, signal) != 0
            {
                return false;
            }
        }
        libc::sigprocmask(libc::SIG_UNBLOCK, &unblock, std::ptr::null_mut()) == 0
    }
}
unsafe extern "C" {
    static mut environ: *mut *mut libc::c_char;
}
/// Fence inherited descriptors in this single-threaded supervisor before spawn.
/// The signal handler only updates an atomic; no concurrent code opens/closes fds.
/// Keep descriptors usable here, but prevent publication to the executed child.
fn inherited_close_on_exec() -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        // Raw syscall keeps compatibility with older libc versions. Unsupported
        // kernels/flags or sandbox denial fall back to the complete fd inventory.
        // https://man7.org/linux/man-pages/man2/close_range.2.html
        let result = unsafe {
            libc::syscall(
                libc::SYS_close_range,
                3_u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            )
        };
        if result == 0 {
            return Ok(());
        }
    }
    #[cfg(target_os = "linux")]
    let directory = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    let directory = "/dev/fd";
    let mut entries = std::fs::read_dir(directory).map_err(|_| Error::Io)?;
    // Bound work by actual open descriptors, not the soft fd limit: a launcher
    // can lower that limit while retaining descriptors above it. Fail closed if
    // the complete inventory is unavailable or exceeds our admission allowance.
    const MAX_ENTRIES: usize = 65_536;
    for (index, entry) in entries.by_ref().enumerate() {
        if index == MAX_ENTRIES {
            return Err(Error::Io);
        }
        let entry = entry.map_err(|_| Error::Io)?;
        let name = entry.file_name();
        let descriptor = name
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
            .ok_or(Error::Io)?;
        if descriptor < 0 {
            return Err(Error::Io);
        }
        if descriptor < 3 {
            continue;
        }
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        if flags == -1 {
            if errno() == libc::EBADF {
                continue;
            }
            return Err(Error::Io);
        }
        if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
            return Err(Error::Io);
        }
    }
    // The iterator stays alive during fcntl calls, so its own inventory fd is
    // never mistaken for a foreign descriptor which has already been closed.
    drop(entries);
    Ok(())
}

pub fn spawn(args: &[CString], input: i32, pipes: [i32; 4]) -> Result<libc::pid_t, Error> {
    inherited_close_on_exec()?;
    // posix_spawn copies the exclusively owned argument/attribute buffers before
    // returning. No Rust code runs in a fork child. dup2 actions clear CLOEXEC on
    // intended stdin/out/err; all other inherited descriptors close during exec.
    unsafe {
        let mut actions = MaybeUninit::<libc::posix_spawn_file_actions_t>::uninit();
        if libc::posix_spawn_file_actions_init(actions.as_mut_ptr()) != 0 {
            return Err(Error::Io);
        }
        let mut actions = actions.assume_init();
        let mut attrs = MaybeUninit::<libc::posix_spawnattr_t>::uninit();
        if libc::posix_spawnattr_init(attrs.as_mut_ptr()) != 0 {
            libc::posix_spawn_file_actions_destroy(&mut actions);
            return Err(Error::Io);
        }
        let mut attrs = attrs.assume_init();
        let mut error = 0;
        for (to, from) in [input, pipes[1], pipes[3]].into_iter().enumerate() {
            if error == 0 {
                error = libc::posix_spawn_file_actions_adddup2(&mut actions, from, to as i32);
            }
        }
        for source in [input, pipes[0], pipes[1], pipes[2], pipes[3]] {
            if error == 0 {
                error = libc::posix_spawn_file_actions_addclose(&mut actions, source);
            }
        }
        let mut mask: libc::sigset_t = std::mem::zeroed();
        let mut defaults: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut mask);
        libc::sigfillset(&mut defaults);
        libc::sigdelset(&mut defaults, libc::SIGKILL);
        libc::sigdelset(&mut defaults, libc::SIGSTOP);
        if error == 0 {
            error = libc::posix_spawnattr_setpgroup(&mut attrs, 0);
        }
        if error == 0 {
            error = libc::posix_spawnattr_setsigmask(&mut attrs, &mask);
        }
        if error == 0 {
            error = libc::posix_spawnattr_setsigdefault(&mut attrs, &defaults);
        }
        if error == 0 {
            error = libc::posix_spawnattr_setflags(
                &mut attrs,
                (libc::POSIX_SPAWN_SETPGROUP
                    | libc::POSIX_SPAWN_SETSIGMASK
                    | libc::POSIX_SPAWN_SETSIGDEF) as i16,
            );
        }
        let mut pid = 0;
        let mut argv: Vec<_> = args.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
        argv.push(std::ptr::null_mut());
        let result = if error == 0 {
            if libc::posix_spawn(
                &mut pid,
                args[0].as_ptr(),
                &actions,
                &attrs,
                argv.as_ptr(),
                environ,
            ) == 0
            {
                Ok(pid)
            } else {
                Err(Error::Spawn)
            }
        } else {
            Err(Error::Io)
        };
        let actions_error = libc::posix_spawn_file_actions_destroy(&mut actions);
        let attrs_error = libc::posix_spawnattr_destroy(&mut attrs);
        // A successfully spawned child must always be returned to its owner for cleanup.
        if result.is_err() && (actions_error != 0 || attrs_error != 0) {
            Err(Error::Io)
        } else {
            result
        }
    }
}
pub fn observe(pid: libc::pid_t) -> Result<bool, i32> {
    let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
    let status = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if status != 0 {
        return Err(errno());
    }
    let info = unsafe { info.assume_init() };
    let observed = unsafe { info.si_pid() };
    if observed == 0 {
        return Ok(false);
    }
    if observed != pid
        || ![libc::CLD_EXITED, libc::CLD_KILLED, libc::CLD_DUMPED].contains(&info.si_code)
    {
        return Err(libc::EINVAL);
    }
    Ok(true)
}
#[cfg(target_os = "macos")]
fn singleton_zombie(pid: libc::pid_t) -> bool {
    let mut members = [0 as libc::pid_t; 2];
    // PROC_PGRP_ONLY=2 from Darwin libproc.h; require an exact, complete singleton.
    unsafe {
        *libc::__error() = 0;
        let count = libc::proc_listpids(
            2,
            pid as u32,
            members.as_mut_ptr().cast(),
            std::mem::size_of_val(&members) as i32,
        );
        errno() == 0 && count == std::mem::size_of::<libc::pid_t>() as i32 && members[0] == pid
    }
}
#[cfg(not(target_os = "macos"))]
fn singleton_zombie(_: libc::pid_t) -> bool {
    false
}
pub fn kill_group(pid: libc::pid_t, finished: bool) -> bool {
    if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 {
        return true;
    }
    let code = errno();
    code == libc::ESRCH || (code == libc::EPERM && finished && singleton_zombie(pid))
}
pub fn reap(pid: libc::pid_t) -> Option<i32> {
    loop {
        let mut status = 0;
        let result = unsafe { libc::waitpid(pid, &mut status, 0) };
        if result == pid {
            return Some(status);
        }
        if result < 0 && errno() == libc::EINTR {
            continue;
        }
        return None;
    }
}
