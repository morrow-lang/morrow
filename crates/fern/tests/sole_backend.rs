//! Native compilation must never depend on the removed QBE executable.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern-sole-backend-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn default_native_compilation_does_not_resolve_qbe() {
    let fixture = Fixture::new();
    let source = fixture.0.join("program.fn");
    fs::write(&source, "fn main():\n    println(9223372036854775807)\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_fern"))
        .args(["run", source.to_str().unwrap()])
        .env("FERN_QBE", fixture.0.join("removed-qbe"))
        .env_remove("LIBRARY_PATH")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, b"9223372036854775807\n");
}
#[test]
fn removed_backend_rejects_before_source_access() {
    let result = Command::new(env!("CARGO_BIN_EXE_fern"))
        .args(["run", "--backend=qbe", "missing.fn"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("only cranelift"));
}
#[test]
fn help_advertises_cranelift_as_the_native_backend() {
    let result = Command::new(env!("CARGO_BIN_EXE_fern"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(result.status.success());
    let help = String::from_utf8(result.stdout).unwrap();
    assert!(help.contains("Cranelift is the native backend"));
    assert!(!help.contains("FERN_QBE"));
}
