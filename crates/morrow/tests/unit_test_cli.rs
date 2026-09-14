use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn run(source: &str, doc_only: bool) -> std::process::Output {
    let path = std::env::temp_dir().join(format!(
        "morrow-unit-cli-{}-{}.fn",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_morrow"));
    command.args(["test"]);
    if doc_only {
        command.arg("--doc");
    }
    let result = command
        .arg(&path)
        .env("MORROW_QBE", "/missing/morrow-test-backend")
        .output()
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    fs::remove_file(path).unwrap();
    result
}
#[test]
fn normal_run_rejects_invalid_test_signatures_before_native_build() {
    for source in ["fn test_bad():false\n", "fn test_bad(value:Int):()\n"] {
        let result = run(source, false);
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("test_bad"), "{error}");
        assert!(!error.contains("/missing/morrow-test-backend"), "{error}");
    }
}
#[test]
fn explicit_doc_mode_does_not_select_unit_tests() {
    let result = run("fn test_unit():()\n", true);
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stdout).contains("doc tests: 0/0 passed"));
}
#[test]
fn normal_run_reports_no_tests_without_backend_when_only_helpers_exist() {
    let result = run("fn helper():()\nfn bench_work():()\n", false);
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stdout).contains("tests: 0/0 passed"));
}
