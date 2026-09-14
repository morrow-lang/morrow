//! Finite child oracles for supervisor integration tests; never installed.
use std::{
    io::{Read, Write},
    os::unix::ffi::OsStrExt,
    time::Duration,
};
fn bytes(fd: i32, bytes: &[u8]) {
    let mut offset = 0;
    for _ in 0..65536 {
        if offset == bytes.len() {
            return;
        }
        let n = unsafe { libc::write(fd, bytes[offset..].as_ptr().cast(), bytes.len() - offset) };
        if n < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        assert!(n > 0);
        offset += n as usize;
    }
    panic!("write retry budget");
}
fn descendant(path: &std::ffi::OsStr, mode: u8) {
    // The fixture is single-threaded and forks before creating any Rust threads.
    let mut ready = [-1; 2];
    assert_eq!(unsafe { libc::pipe(ready.as_mut_ptr()) }, 0);
    let child = unsafe { libc::fork() };
    assert!(child >= 0);
    if child == 0 {
        if mode == 2 {
            assert!(unsafe { libc::setsid() } >= 0);
        }
        unsafe {
            libc::close(ready[0]);
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        writeln!(file, "{}", std::process::id()).unwrap();
        drop(file);
        bytes(ready[1], b"r");
        unsafe {
            libc::close(ready[1]);
        }
        std::thread::sleep(Duration::from_secs(3));
        unsafe {
            libc::_exit(0);
        }
    }
    unsafe {
        libc::close(ready[1]);
    }
    let mut marker = 0u8;
    assert_eq!(
        unsafe { libc::read(ready[0], (&mut marker as *mut u8).cast(), 1) },
        1
    );
    unsafe {
        libc::close(ready[0]);
    }
    if mode == 1 {
        std::thread::sleep(Duration::from_secs(3));
    }
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let number = |i: usize| args[i].to_str().unwrap().parse::<usize>().unwrap();
    let status = match args[1].to_str().unwrap() {
        "exit" => number(2) as u8,
        "streams" => {
            bytes(1, b"out\0\xf0\x9f\x8c\xbf");
            bytes(2, b"err\n");
            7
        }
        "args" => {
            assert_eq!(std::io::stdin().read(&mut [0]).unwrap(), 0);
            for arg in &args[2..] {
                bytes(1, format!("{}:", arg.as_bytes().len()).as_bytes());
                bytes(1, arg.as_bytes());
                bytes(1, b"\n");
            }
            0
        }
        "emit" => {
            let size = number(2);
            assert!(size <= 1000000);
            bytes(number(3) as i32, &vec![b'x'; size]);
            0
        }
        "dual" => {
            for _ in 0..100 {
                bytes(1, &[b'x'; 4096]);
                bytes(2, &[b'x'; 4096]);
            }
            0
        }
        "stdin" => {
            let mut buffer = [0; 32];
            let n = std::io::stdin().read(&mut buffer).unwrap();
            bytes(1, &buffer[..n]);
            7
        }
        "fds" => {
            let mut mask = 0;
            for fd in 0..3 {
                if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
                    mask |= 1 << fd;
                }
            }
            mask
        }
        "leaks" => {
            for fd in 3..4096 {
                assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EBADF)
                );
            }
            0
        }
        "die" => {
            unsafe {
                libc::raise(libc::SIGTERM);
            }
            90
        }
        "close_wait" => {
            unsafe {
                libc::close(1);
                libc::close(2);
            }
            std::thread::sleep(Duration::from_secs(3));
            0
        }
        "descendant" => {
            descendant(&args[2], 0);
            0
        }
        "descendant_wait" => {
            descendant(&args[2], 1);
            0
        }
        "escaped" => {
            descendant(&args[2], 2);
            0
        }
        "wait_exit" => {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&args[2])
                .unwrap();
            file.write_all(b"ready\n").unwrap();
            drop(file);
            let mut status = 90;
            for _ in 0..2000 {
                if std::path::Path::new(&args[3]).exists() {
                    status = number(4) as u8;
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            status
        }
        _ => 90,
    };
    std::process::ExitCode::from(status)
}
