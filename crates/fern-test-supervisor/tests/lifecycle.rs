//! Independent process, stream, cleanup, and publication contract oracles.
use std::{
    fs,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
const LIMIT: usize = 262144;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Private(PathBuf);
impl Private {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn command(&self, timeout: u32, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fern-test-supervisor"));
        command
            .arg(timeout.to_string())
            .arg(&self.0)
            .arg("--")
            .arg(env!("CARGO_BIN_EXE_fern-test-fixture"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    fn empty(&self) {
        assert_eq!(fs::read_dir(&self.0).unwrap().count(), 0);
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(6);
    for _ in 0..6000 {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("supervisor exceeded bounded lifecycle");
}
fn decode(packet: Vec<u8>) -> (char, u32, Vec<u8>, Vec<u8>) {
    assert!(packet.len() <= 2 * LIMIT + 256);
    let newline = packet.iter().position(|&b| b == b'\n').unwrap();
    assert!(newline < 128);
    let fields: Vec<_> = std::str::from_utf8(&packet[..newline])
        .unwrap()
        .split(' ')
        .collect();
    assert_eq!(fields.len(), 6);
    assert_eq!(&fields[..2], &["FERN_TEST", "1"]);
    let status = fields[3].parse::<u32>().unwrap();
    let out = fields[4].parse::<usize>().unwrap();
    let err = fields[5].parse::<usize>().unwrap();
    assert!(out <= LIMIT && err <= LIMIT);
    assert!(["N", "E"].contains(&fields[2]));
    let body = &packet[newline + 1..];
    assert_eq!(&body[out + err..], b"\nFERN_TEST_END 1\n");
    (
        fields[2].chars().next().unwrap(),
        status,
        body[..out].to_vec(),
        body[out..out + err].to_vec(),
    )
}
fn collect(mut child: Child, disconnect: bool) -> (char, u32, Vec<u8>, Vec<u8>) {
    let alive = child.stdin.take();
    if disconnect {
        drop(alive);
    } else {
        return collect_alive(child, alive);
    }
    collect_alive(child, None)
}
fn collect_alive(
    mut child: Child,
    alive: Option<std::process::ChildStdin>,
) -> (char, u32, Vec<u8>, Vec<u8>) {
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut data = Vec::new();
        stdout
            .take((2 * LIMIT + 257) as u64)
            .read_to_end(&mut data)
            .unwrap();
        data
    });
    let err = thread::spawn(move || {
        let mut data = Vec::new();
        stderr.take(4096).read_to_end(&mut data).unwrap();
        data
    });
    let status = wait(&mut child);
    drop(alive);
    let data = out.join().unwrap();
    let errors = err.join().unwrap();
    assert!(status.success(), "{status:?} {errors:?}");
    assert!(errors.is_empty(), "{errors:?}");
    decode(data)
}
fn run(private: &Private, args: &[&str]) -> (char, u32, Vec<u8>, Vec<u8>) {
    collect(private.command(2000, args).spawn().unwrap(), false)
}
fn ready(path: &Path) {
    for _ in 0..2000 {
        if fs::read(path).is_ok_and(|bytes| bytes.ends_with(b"\n")) {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("child never became ready");
}
fn stopped(pid: &str) {
    for _ in 0..100 {
        let status = Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .unwrap();
        let status = String::from_utf8_lossy(&status.stdout);
        if status.trim().is_empty() || status.trim().starts_with('Z') {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("owned descendant still executing");
}
#[test]
fn status_bytes_arguments_eof_and_descriptors() {
    let p = Private::new();
    for status in [0, 7, 125, 127, 255] {
        assert_eq!(
            run(&p, &["exit", &status.to_string()]),
            ('N', status << 8, vec![], vec![])
        );
    }
    assert_eq!(
        run(&p, &["streams"]),
        (
            'N',
            7 << 8,
            b"out\0\xf0\x9f\x8c\xbf".to_vec(),
            b"err\n".to_vec()
        )
    );
    assert_eq!(
        run(
            &p,
            &["args", "", "a b", "é🌿", "$(touch nope)", "line\nbreak"]
        ),
        (
            'N',
            0,
            b"0:\n3:a b\n6:\xc3\xa9\xf0\x9f\x8c\xbf\n13:$(touch nope)\n10:line\nbreak\n".to_vec(),
            vec![]
        )
    );
    assert_eq!(run(&p, &["stdin"]), ('N', 7 << 8, vec![], vec![]));
    assert_eq!(run(&p, &["leaks"]), ('N', 0, vec![], vec![]));
    let signal = run(&p, &["die"]);
    assert_eq!(signal.0, 'N');
    assert_eq!(libc::WTERMSIG(signal.1 as i32), libc::SIGTERM);
    p.empty();
}
#[test]
fn ambient_descriptor_is_not_inherited_by_the_supervised_child() {
    let p = Private::new();
    let file = fs::File::open("/dev/null").unwrap();
    // Allocate a known descriptor without exposing it to concurrent test spawns.
    let raw = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
    assert!((3..4096).contains(&raw));
    let inherited = unsafe { OwnedFd::from_raw_fd(raw) };
    for lowered_limit in [false, true] {
        let mut command = p.command(2000, &["leaks"]);
        unsafe {
            command.pre_exec(move || {
                // Only the fork child's descriptor table loses CLOEXEC. This
                // models a CI launcher passing an ambient fd into the supervisor.
                if libc::fcntl(raw, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if lowered_limit {
                    // Existing descriptors remain valid above a lowered soft
                    // limit. Isolation cannot simply scan 3..rlim_cur.
                    let mut limit = std::mem::zeroed::<libc::rlimit>();
                    if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    limit.rlim_cur = 32;
                    if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        assert_eq!(
            collect(command.spawn().unwrap(), false),
            ('N', 0, vec![], vec![])
        );
        let flags = unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
        p.empty();
    }
}

#[test]
fn exact_stream_caps_and_overflow_are_independent() {
    let p = Private::new();
    for fd in [1, 2] {
        for length in [0, 4096, LIMIT] {
            let result = run(&p, &["emit", &length.to_string(), &fd.to_string()]);
            assert_eq!((result.0, result.1), ('N', 0));
            assert_eq!(result.2, if fd == 1 { vec![b'x'; length] } else { vec![] });
            assert_eq!(result.3, if fd == 2 { vec![b'x'; length] } else { vec![] });
        }
        let result = run(&p, &["emit", &(LIMIT + 1).to_string(), &fd.to_string()]);
        assert_eq!((result.0, result.1), ('E', 4));
    }
    let result = run(&p, &["dual"]);
    assert_eq!((result.0, result.1), ('E', 4));
    p.empty();
}
#[test]
fn cancellation_kills_only_owned_descendants() {
    let p = Private::new();
    let mut sibling = Command::new("/bin/sleep").arg("10").spawn().unwrap();
    for signal in [false, true] {
        let path = p.0.join("ready");
        let mut child = p
            .command(3000, &["descendant_wait", path.to_str().unwrap()])
            .spawn()
            .unwrap();
        ready(&path);
        if signal {
            assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
        } else {
            drop(child.stdin.take());
        }
        let result = collect(child, false);
        assert_eq!((result.0, result.1), ('E', if signal { 6 } else { 8 }));
        stopped(&fs::read_to_string(&path).unwrap());
        fs::remove_file(path).unwrap();
        assert!(sibling.try_wait().unwrap().is_none());
    }
    sibling.kill().unwrap();
    sibling.wait().unwrap();
    p.empty();
}
#[test]
fn exited_leader_is_retained_until_group_cleanup_and_escaped_pipe_has_deadline() {
    let p = Private::new();
    for (mode, expected) in [
        ("descendant", ('N', 0)),
        ("descendant_wait", ('E', 3)),
        ("escaped", ('E', 3)),
    ] {
        let path = p.0.join("ready");
        let result = collect(
            p.command(500, &[mode, path.to_str().unwrap()])
                .spawn()
                .unwrap(),
            false,
        );
        assert_eq!((result.0, result.1), expected);
        ready(&path);
        if mode != "escaped" {
            stopped(&fs::read_to_string(&path).unwrap());
        }
        fs::remove_file(path).unwrap();
        p.empty();
    }
}
#[test]
fn unavailable_reaper_and_spawn_failure_are_distinct() {
    let p = Private::new();
    let mut command = p.command(1000, &["exit", "0"]);
    unsafe {
        command.pre_exec(|| {
            libc::signal(libc::SIGCHLD, libc::SIG_IGN);
            Ok(())
        });
    }
    let result = collect(command.spawn().unwrap(), false);
    assert_eq!((result.0, result.1), ('E', 7));
    let mut command = Command::new(env!("CARGO_BIN_EXE_fern-test-supervisor"));
    command
        .arg("1000")
        .arg(&p.0)
        .args(["--", "/nonexistent/fern-child"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = collect(command.spawn().unwrap(), false);
    assert_eq!((result.0, result.1), ('E', 2));
    p.empty();
}
#[test]
fn blocked_publication_has_separate_bounded_deadline() {
    let p = Private::new();
    let mut child = p
        .command(2000, &["emit", &LIMIT.to_string(), "1"])
        .spawn()
        .unwrap();
    let alive = child.stdin.take();
    assert_eq!(wait(&mut child).code(), Some(125));
    drop(alive);
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() < LIMIT);
    p.empty();
}
#[test]
fn concurrent_invocations_have_independent_spool_ownership() {
    thread::scope(|scope| {
        let jobs: Vec<_> = (0..16)
            .map(|i| {
                scope.spawn(move || {
                    let p = Private::new();
                    assert_eq!(
                        run(&p, &["exit", &i.to_string()]),
                        ('N', i << 8, vec![], vec![])
                    );
                    p.empty();
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
    });
}

#[test]
fn replaced_spool_and_directory_names_remain_owned_by_the_replacer() {
    for replace_directory in [false, true] {
        let p = Private::new();
        let ready_path = p.0.join("ready");
        let release = p.0.join("release");
        let child = p
            .command(
                2000,
                &[
                    "wait_exit",
                    ready_path.to_str().unwrap(),
                    release.to_str().unwrap(),
                    "0",
                ],
            )
            .spawn()
            .unwrap();
        ready(&ready_path);
        let (original, displaced) = if replace_directory {
            (p.0.join("capture"), p.0.join("displaced"))
        } else {
            (p.0.join("capture/stdout"), p.0.join("capture/displaced"))
        };
        fs::rename(&original, &displaced).unwrap();
        let foreign = if replace_directory {
            fs::create_dir(&original).unwrap();
            original.join("foreign")
        } else {
            original
        };
        fs::write(&foreign, b"owned by replacer").unwrap();
        fs::write(&release, b"release").unwrap();
        let result = collect(child, false);
        assert_eq!((result.0, result.1), ('E', 5));
        assert_eq!(fs::read(&foreign).unwrap(), b"owned by replacer");
        assert!(displaced.exists());
    }
}

#[test]
fn unsupported_filesystem_inputs_and_non_executable_formats_never_run_a_shell() {
    let p = Private::new();
    let invoke = |parent: &Path, executable: &Path| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fern-test-supervisor"));
        command
            .arg("1000")
            .arg(parent)
            .arg("--")
            .arg(executable)
            .args(["exit", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let result = collect(command.spawn().unwrap(), false);
        (result.0, result.1)
    };
    let fixture = Path::new(env!("CARGO_BIN_EXE_fern-test-fixture"));
    let link = p.0.join("link");
    std::os::unix::fs::symlink(&p.0, &link).unwrap();
    assert_eq!(invoke(&link, fixture), ('E', 5));
    let fifo = p.0.join("fifo");
    let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(invoke(&fifo, fixture), ('E', 5));
    let plain = p.0.join("plain");
    fs::write(
        &plain,
        format!("touch '{}'\n", p.0.join("unexpected-shell").display()),
    )
    .unwrap();
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(invoke(&p.0, &plain), ('E', 2));
    assert!(!p.0.join("unexpected-shell").exists());
}
