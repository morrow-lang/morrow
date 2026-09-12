//! Real process/PTY boundary tests; all fixtures execute the Rust runtime directly.
use fern_runtime::{abi, io, tui};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn child(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "isolated_child", "--nocapture"])
        .env("FERN_IO_CHILD", mode);
    command
}

fn payload(value: i64) -> Result<i64, i64> {
    let value = unsafe { &*(value as *const abi::ResultValue) };
    if value.tag == 0 {
        Ok(value.value)
    } else {
        Err(value.value)
    }
}

fn isolated_child() {
    let Ok(mode) = std::env::var("FERN_IO_CHILD") else {
        return;
    };
    match mode.as_str() {
        "input" | "password" => {
            let value = unsafe {
                if mode == "input" {
                    tui::fern_prompt_input(c"name> ".as_ptr())
                } else {
                    tui::fern_prompt_password(c"secret> ".as_ptr())
                }
            };
            println!("RESULT:{}", unsafe { abi::text(value) });
        }
        "cursor" => {
            print!("CURSOR_BEGIN");
            std::io::stdout().flush().unwrap();
            tui::fern_term_move_to(2, 3);
            tui::fern_term_up(1);
            tui::fern_term_down(2);
            tui::fern_term_left(3);
            tui::fern_term_right(4);
            tui::fern_term_up(0);
            tui::fern_term_left(-1);
            tui::fern_term_hide_cursor();
            tui::fern_term_show_cursor();
            tui::fern_term_save_cursor();
            tui::fern_term_restore_cursor();
            tui::fern_term_clear();
            println!("CURSOR_END");
        }
        "stderr-closed" => unsafe {
            libc::close(2);
            assert_eq!(payload(io::fern_write_stderr(c"".as_ptr())), Ok(0));
            assert_eq!(payload(io::fern_write_stderr(c"x".as_ptr())), Err(3));
            assert_eq!(libc::fcntl(2, libc::F_GETFD), -1);
        },
        "stderr-broken" | "stderr-pending" => unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            let mut pipe = [0; 2];
            assert_eq!(libc::pipe(pipe.as_mut_ptr()), 0);
            assert_eq!(libc::dup2(pipe[1], 2), 2);
            libc::close(pipe[0]);
            libc::close(pipe[1]);
            let mut mask = std::mem::zeroed::<libc::sigset_t>();
            let mut old = std::mem::zeroed::<libc::sigset_t>();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, libc::SIGPIPE);
            if mode == "stderr-pending" {
                assert_eq!(libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut old), 0);
                assert_eq!(libc::raise(libc::SIGPIPE), 0);
            }
            let result = payload(io::fern_write_stderr(c"x".as_ptr()));
            assert_eq!(result, Err(3));
            let mut pending = std::mem::zeroed::<libc::sigset_t>();
            assert_eq!(libc::sigpending(&mut pending), 0);
            assert_eq!(
                libc::sigismember(&pending, libc::SIGPIPE),
                (mode == "stderr-pending") as i32
            );
            let mut current = std::mem::zeroed::<libc::sigset_t>();
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut current),
                0
            );
            assert_eq!(
                libc::sigismember(&current, libc::SIGPIPE),
                (mode == "stderr-pending") as i32
            );
            if mode == "stderr-pending" {
                let mut signal = 0;
                assert_eq!(libc::sigwait(&mask, &mut signal), 0);
                assert_eq!(signal, libc::SIGPIPE);
                assert_eq!(
                    libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()),
                    0
                );
            }
        },
        _ => panic!("unknown fixture mode"),
    }
}

fn terminal(mode: &str, keys: &[u8], prompt: &[u8], term: &str) -> Vec<u8> {
    let (mut master, slave) = unsafe {
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
        (
            std::fs::File::from_raw_fd(master),
            std::fs::File::from_raw_fd(slave),
        )
    };
    let settings = || {
        let mut settings = unsafe { std::mem::zeroed::<libc::termios>() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut settings) },
            0
        );
        (
            settings.c_iflag,
            settings.c_oflag,
            settings.c_cflag,
            settings.c_lflag,
            settings.c_cc,
        )
    };
    let original = settings();
    let mut child = child(mode)
        .env("TERM", term)
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave.try_clone().unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    let mut sent = prompt.is_empty();
    let status = loop {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("PTY timeout: {}", String::from_utf8_lossy(&output));
        }
        let mut fd = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut fd, 1, 20) } > 0 {
            let mut bytes = [0; 65536];
            match master.read(&mut bytes) {
                Ok(count) => output.extend_from_slice(&bytes[..count]),
                Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
                Err(error) => panic!("PTY read: {error}"),
            }
            if !sent && output.windows(prompt.len()).any(|part| part == prompt) {
                master.write_all(keys).unwrap();
                sent = true;
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
    };
    assert!(status.success(), "{}", String::from_utf8_lossy(&output));
    assert_eq!(settings(), original, "terminal settings leaked");
    output
}

fn terminal_cursor_editing_unicode_delete_and_restoration() {
    for (keys, expected) in [
        (b"ac\x1b[Db\x01X\x05!\r".as_slice(), "RESULT:Xabc!\r\n"),
        ("aéZ\x1b[D\x7f\x1b[3~b\r".as_bytes(), "RESULT:ab\r\n"),
        (b"abc\x1b[D\x15x\r", "RESULT:x\r\n"),
        ("aéZ\x1b[D\x14\r".as_bytes(), "RESULT:aZé\r\n"),
        (b"discard\x03", "RESULT:\r\n"),
        (b"\x04", "RESULT:\r\n"),
    ] {
        let output = terminal("input", keys, b"name> ", "xterm");
        assert!(
            String::from_utf8_lossy(&output).contains(expected),
            "{}",
            String::from_utf8_lossy(&output)
        );
    }
}

fn passwords_mask_both_interactive_and_dumb_terminals() {
    for term in ["xterm", "dumb"] {
        let output = terminal("password", b"never-echo-this\r", b"secret> ", term);
        let output = String::from_utf8_lossy(&output);
        assert_eq!(output.matches("never-echo-this").count(), 1, "{output}");
        assert!(output.contains("RESULT:never-echo-this\r\n"), "{output}");
    }
}

fn cursor_sequences_are_exact_and_silent_in_pipes() {
    let output = terminal("cursor", b"", b"", "xterm");
    assert!(String::from_utf8_lossy(&output).contains("CURSOR_BEGIN\x1b[2;3H\x1b[1A\x1b[2B\x1b[3D\x1b[4C\x1b[?25l\x1b[?25h\x1b[s\x1b[u\x1b[2J\x1b[HCURSOR_END\r\n"));
    let output = child("cursor").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("CURSOR_BEGINCURSOR_END\n"));
}

fn stderr_preserves_closed_descriptors_and_pending_sigpipe() {
    for mode in ["stderr-closed", "stderr-broken", "stderr-pending"] {
        let output = child(mode).stdin(Stdio::null()).output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {:?} {} {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn canonical_cleanup_exports_link() {
    unsafe extern "C" {
        fn fern_regex_match_free(value: *mut std::ffi::c_void);
        fn fern_regex_captures_free(value: *mut std::ffi::c_void);
        fn fern_panel_free(value: *mut std::ffi::c_void);
        fn fern_table_free(value: *mut std::ffi::c_void);
        fn fern_progress_free(value: *mut std::ffi::c_void);
        fn fern_spinner_free(value: *mut std::ffi::c_void);
    }
    unsafe {
        fern_regex_match_free(std::ptr::null_mut());
        fern_regex_captures_free(std::ptr::null_mut());
        fern_panel_free(std::ptr::null_mut());
        fern_table_free(std::ptr::null_mut());
        fern_progress_free(std::ptr::null_mut());
        fern_spinner_free(std::ptr::null_mut());
    }
}

fn main() {
    if std::env::var_os("FERN_IO_CHILD").is_some() {
        isolated_child();
        return;
    }
    for (name, test) in [
        (
            "terminal cursor editing and restoration",
            terminal_cursor_editing_unicode_delete_and_restoration as fn(),
        ),
        (
            "password masking and restoration",
            passwords_mask_both_interactive_and_dumb_terminals,
        ),
        (
            "cursor sequences and nonterminal silence",
            cursor_sequences_are_exact_and_silent_in_pipes,
        ),
        (
            "stderr descriptors and SIGPIPE",
            stderr_preserves_closed_descriptors_and_pending_sigpipe,
        ),
        ("canonical cleanup exports", canonical_cleanup_exports_link),
    ] {
        test();
        println!("test {name} ... ok");
    }
    println!("5 protocol tests passed");
}
