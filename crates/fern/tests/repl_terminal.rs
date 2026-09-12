//! Exercise the shipping REPL through a real controlling terminal.
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fern-repl-terminal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn history(&self) -> PathBuf {
        self.0.join("history")
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Terminal {
    child: Child,
    master: File,
    pending: Vec<u8>,
    finished: bool,
}
impl Terminal {
    fn new(history: &Path) -> Self {
        let (master, slave) = unsafe {
            let mut master = -1;
            let mut slave = -1;
            let mut size = libc::winsize {
                ws_row: 24,
                ws_col: 120,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            assert_eq!(
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::addr_of_mut!(size)
                ),
                0
            );
            (File::from_raw_fd(master), File::from_raw_fd(slave))
        };
        let mut command = Command::new(env!("CARGO_BIN_EXE_fern"));
        command
            .arg("repl")
            .env("TERM", "xterm")
            .env("FERN_REPL_HISTORY", history)
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave);
        // The REPL receives its own controlling terminal, so Ctrl-C can never signal the test runner.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut terminal = Self {
            child: command.spawn().unwrap(),
            master,
            pending: Vec::new(),
            finished: false,
        };
        terminal.until(b"fern> ");
        terminal
    }
    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }
    fn read(&mut self) {
        let mut fd = libc::pollfd {
            fd: self.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut fd, 1, 20) } <= 0 {
            return;
        }
        let mut bytes = [0; 65536];
        match self.master.read(&mut bytes) {
            Ok(count) => self.pending.extend_from_slice(&bytes[..count]),
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
            Err(error) => panic!("PTY read: {error}"),
        }
        while let Some(start) = self
            .pending
            .windows(4)
            .position(|bytes| bytes == b"\x1b[6n")
        {
            self.pending.drain(start..start + 4);
            self.send(b"\x1b[1;1R");
        }
        assert!(
            self.pending.len() < 1024 * 1024,
            "unbounded terminal output"
        );
    }
    fn until(&mut self, expected: &[u8]) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(start) = self
                .pending
                .windows(expected.len())
                .position(|bytes| bytes == expected)
            {
                self.pending.drain(..start + expected.len());
                return;
            }
            assert!(
                Instant::now() < deadline,
                "missing {:?}: {:?}",
                String::from_utf8_lossy(expected),
                String::from_utf8_lossy(&self.pending)
            );
            self.read();
        }
    }
    fn close(mut self) {
        self.send(b":quit\n");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.finished = true;
                assert!(
                    status.success(),
                    "REPL exited {status}: {:?}",
                    String::from_utf8_lossy(&self.pending)
                );
                return;
            }
            assert!(Instant::now() < deadline, "REPL did not exit");
            self.read();
        }
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn editing_completion_and_history_survive_sessions() {
    let work = Workspace::new();
    let mut terminal = Terminal::new(&work.history());
    terminal.send(b"4 + 3\x1b[D\x1b[3~2\n");
    terminal.until(b"6 : Int");
    terminal.until(b"fern> ");
    terminal.send(b"printl\t(8)\n");
    terminal.until(b"8\r\n");
    terminal.until(b"fern> ");
    terminal.send(b"40 + 2\n");
    terminal.until(b"42 : Int");
    terminal.until(b"fern> ");
    terminal.close();
    assert!(work.history().is_file());
    let mut terminal = Terminal::new(&work.history());
    terminal.send(b"\x1b[A\n");
    terminal.until(b"42 : Int");
    terminal.until(b"fern> ");
    terminal.close();
}

#[test]
fn nonregular_history_never_blocks_the_terminal() {
    use std::os::unix::ffi::OsStrExt;
    let work = Workspace::new();
    let path = std::ffi::CString::new(work.history().as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut terminal = Terminal::new(&work.history());
    terminal.send(b"20 + 2\n");
    terminal.until(b"22 : Int");
    terminal.until(b"fern> ");
    terminal.close();
}

#[test]
fn control_c_discards_pending_block_and_preserves_existing_scope() {
    let work = Workspace::new();
    let mut terminal = Terminal::new(&work.history());
    terminal.send(b"let saved = 19\n");
    terminal.until(b"fern> ");
    terminal.send(b"fn unfinished():\n");
    terminal.until(b"...   ");
    terminal.send(b"\x03");
    terminal.until(b"fern> ");
    terminal.send(b"saved\n");
    terminal.until(b"19 : Int");
    terminal.until(b"fern> ");
    terminal.close();
}
