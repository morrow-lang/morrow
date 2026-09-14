use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow-native-doc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, code: &str, body: &str) {
        fs::write(
            self.0.join("library.fn"),
            format!("@doc \"\"\"\n```morrow\n{code}\n```\n\"\"\"\n{body}\n"),
        )
        .unwrap();
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_morrow"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn doc_cli_help_and_empty_discovery_need_no_native_backend() {
    let dir = Directory::new();
    dir.write("", "fn library(): ()");
    let help = dir.run(&["test", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("execute user code"));
    let result = dir.run(&["test", "--doc", "."]);
    assert!(result.status.success(), "{:?}", result);
    assert!(String::from_utf8_lossy(&result.stdout).contains("0/0 passed"));
}
#[test]
fn doc_cli_rejects_unsupported_modes_and_invalid_timeouts() {
    let dir = Directory::new();
    for args in [
        vec!["test", "--coverage"],
        vec!["test", "--watch"],
        vec!["test", "--timeout", "0"],
        vec!["test", "--timeout", "61"],
        vec!["test", "--timeout"],
        vec!["test", "--timeout", "1.5"],
        vec!["test", "--doc", "--doc"],
        vec!["test", "one", "two"],
    ] {
        let result = dir.run(&args);
        assert_eq!(result.status.code(), Some(1), "{:?}", result);
        assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
    }
}
#[test]
fn malformed_documentation_fails_before_native_compilation() {
    let dir = Directory::new();
    dir.write("let x = 3 # => 3", "fn library(): ()");
    let result = dir.run(&["test", "--doc", "library.fn"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("example 1"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("0/1 passed"));
}
