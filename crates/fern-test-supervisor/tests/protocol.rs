use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Private(PathBuf);
impl Private {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern-rust-supervisor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(timeout: &str, directory: &Private, body: &str) -> Vec<u8> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fern-test-supervisor"))
        .args([
            timeout,
            directory.0.to_str().unwrap(),
            "--",
            "/bin/sh",
            "-c",
            body,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let alive = child.stdin.take();
    let mut packet = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut packet)
        .unwrap();
    assert!(child.wait().unwrap().success());
    drop(alive);
    packet
}
#[test]
fn framing_preserves_binary_streams_and_native_status_125() {
    let private = Private::new();
    let packet = run(
        "1000",
        &private,
        "printf 'out\\000'; printf err >&2; exit 125",
    );
    assert_eq!(
        packet,
        b"FERN_TEST 1 N 32000 4 3\nout\0err\nFERN_TEST_END 1\n"
    );
    assert_eq!(fs::read_dir(&private.0).unwrap().count(), 0);
}
#[test]
fn timeout_is_a_supervisor_error_and_removes_private_spools() {
    let private = Private::new();
    let packet = run("20", &private, "exec /bin/sleep 5");
    assert_eq!(packet, b"FERN_TEST 1 E 3 0 0\n\nFERN_TEST_END 1\n");
    assert_eq!(fs::read_dir(&private.0).unwrap().count(), 0);
}
#[test]
fn invalid_timeout_rejects_before_spawning() {
    let private = Private::new();
    assert_eq!(
        run("0", &private, "exit 0"),
        b"FERN_TEST 1 E 1 0 0\n\nFERN_TEST_END 1\n"
    );
}
#[test]
fn preexisting_capture_directory_is_not_owned_or_removed() {
    let private = Private::new();
    fs::create_dir(private.0.join("capture")).unwrap();
    fs::write(private.0.join("capture/foreign"), b"keep").unwrap();
    assert_eq!(
        run("1000", &private, "exit 0"),
        b"FERN_TEST 1 E 5 0 0\n\nFERN_TEST_END 1\n"
    );
    assert_eq!(
        fs::read(private.0.join("capture/foreign")).unwrap(),
        b"keep"
    );
}
