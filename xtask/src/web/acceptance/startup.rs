//! Preserve bounded startup evidence before temporary process artifacts are removed.
use super::{Process, Result, WAIT};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    mem::MaybeUninit,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    thread,
    time::{Duration, Instant},
};
const READY_BYTES: u64 = 16 * 1024;
pub(super) const LOG_BYTES: u64 = 8 * 1024;

pub(super) fn readiness(
    process: &Process,
    path: &Path,
    log: &Path,
    predicate: impl Fn(&str) -> Option<String>,
) -> Result<String> {
    until(process, path, log, predicate, Instant::now() + WAIT)
}
pub(super) fn until(
    process: &Process,
    path: &Path,
    log: &Path,
    predicate: impl Fn(&str) -> Option<String>,
    deadline: Instant,
) -> Result<String> {
    loop {
        match observe(process) {
            Ok(Some(status)) => {
                return Err(diagnostic(
                    path,
                    log,
                    &format!("child exited before readiness: {status}"),
                ));
            }
            Err(error) => {
                return Err(diagnostic(
                    path,
                    log,
                    &format!("cannot observe startup child: {error}"),
                ));
            }
            Ok(None) => (),
        }
        if Instant::now() >= deadline {
            return Err(diagnostic(path, log, "readiness timeout"));
        }
        if let Ok(text) = read(path, READY_BYTES, false)
            && let Some(value) = predicate(&text)
        {
            if Instant::now() >= deadline {
                return Err(diagnostic(path, log, "readiness timeout"));
            }
            return Ok(value);
        }
        thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(25)),
        );
    }
}
fn diagnostic(path: &Path, log: &Path, reason: &str) -> String {
    let state =
        read(path, READY_BYTES, false).unwrap_or_else(|error| format!("<unavailable: {error}>"));
    let stderr =
        read(log, LOG_BYTES, true).unwrap_or_else(|error| format!("<unavailable: {error}>"));
    format!(
        "{reason}: {}\nReadiness observation (up to {READY_BYTES} bytes):\n{state}\nChild stderr tail (up to {LOG_BYTES} bytes):\n{stderr}",
        path.display()
    )
}
fn read(path: &Path, limit: u64, tail: bool) -> std::io::Result<String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other(
            "startup observation is not a regular file",
        ));
    }
    if tail {
        file.seek(SeekFrom::Start(metadata.len().saturating_sub(limit)))?;
    } else if metadata.len() > limit {
        return Err(std::io::Error::other(
            "readiness observation exceeds byte limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
fn observe(process: &Process) -> Result<Option<String>> {
    let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: this is our retained direct child; WNOWAIT observes without reaping,
    // preserving the process-group identity until Process::drop finishes signaling.
    let status = unsafe {
        libc::waitid(
            libc::P_PID,
            process.0.id() as libc::id_t,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if status != 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::Interrupted {
            Ok(None)
        } else {
            Err(error.to_string())
        };
    }
    // SAFETY: successful waitid initialized the zeroed siginfo record. A zero PID
    // is the specified WNOHANG result when the child has not exited.
    let info = unsafe { info.assume_init() };
    let pid = unsafe { info.si_pid() };
    if pid == 0 {
        return Ok(None);
    }
    if pid as u32 != process.0.id() {
        return Err("unexpected startup child identity".into());
    }
    let status = unsafe { info.si_status() };
    match info.si_code {
        libc::CLD_EXITED => Ok(Some(format!("exit status {status}"))),
        libc::CLD_KILLED | libc::CLD_DUMPED => Ok(Some(format!("signal {status}"))),
        _ => Err("unexpected startup child state".into()),
    }
}
