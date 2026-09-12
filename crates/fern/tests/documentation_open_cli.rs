#![cfg(unix)]
use std::{
    ffi::OsString,
    fs,
    os::unix::{ffi::OsStringExt, fs::symlink},
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern-doc-open-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("library.fn"),
            "@doc \"\"\"A helper.\"\"\"\nfn helper(value:Int):value+1\n",
        )
        .unwrap();
        fs::create_dir(path.join("tools")).unwrap();
        let result = Self(path);
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_fern"));
        let helper = binary.parent().unwrap().join("fern-opener-fixture");
        assert!(
            helper.is_file(),
            "build Rust supervisor fixtures before tests"
        );
        for name in ["open", "xdg-open"] {
            symlink(&helper, result.0.join("tools").join(name)).unwrap();
        }
        result
    }
    fn opener_failure(&self, code: u8) {
        fs::write(self.0.join("tools/exit-code"), code.to_string()).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        self.run_os(args.iter().map(OsString::from).collect())
    }
    fn run_os(&self, args: Vec<OsString>) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fern"))
            .current_dir(&self.0)
            .args(args)
            .env("PATH", self.0.join("tools"))
            .env("CALLS", self.0.join("calls"))
            .env("SEEN", self.0.join("seen"))
            .output()
            .unwrap()
    }
    fn assert_opened(&self, path: PathBuf) {
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"<!doctype html>"));
        assert!(bytes.ends_with(b"</main></body></html>\n"));
        assert_eq!(bytes, fs::read(self.0.join("seen")).unwrap());
        let mut expected = path.canonicalize().unwrap().into_os_string().into_vec();
        expected.push(0);
        assert_eq!(fs::read(self.0.join("calls")).unwrap(), expected);
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn open_implies_complete_retained_html_and_literal_absolute_argument() {
    let dir = Directory::new();
    let result = dir.run(&["doc", "library.fn", "--open"]);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("fern-docs.html"));
    dir.assert_opened(dir.0.join("fern-docs.html"));
    let name = "-docs $(touch INJECTED); ' λ.html";
    let result = dir.run(&["doc", "--open", "--html", "library.fn", "-o", name]);
    assert!(result.status.success(), "{result:?}");
    dir.assert_opened(dir.0.join(name));
    assert!(!dir.0.join("INJECTED").exists());
}
#[test]
fn directory_and_inferred_modes_keep_generation_and_source_protection() {
    let dir = Directory::new();
    for args in [
        vec!["doc", ".", "--open"],
        vec!["doc", "library.fn", "--inferred", "--open"],
        vec!["doc", ".", "--inferred", "--open"],
    ] {
        let result = dir.run(&args);
        assert!(result.status.success(), "{result:?}");
        dir.assert_opened(dir.0.join("fern-docs.html"));
    }
    fs::remove_file(dir.0.join("calls")).unwrap();
    let before = fs::read(dir.0.join("library.fn")).unwrap();
    let result = dir.run(&["doc", ".", "--inferred", "--open", "-o", "library.fn"]);
    assert!(!result.status.success());
    assert!(!dir.0.join("calls").exists());
    assert_eq!(before, fs::read(dir.0.join("library.fn")).unwrap());
}
#[test]
fn parse_check_write_and_option_failures_never_launch() {
    let dir = Directory::new();
    fs::write(dir.0.join("bad.fn"), "fn broken(:\n").unwrap();
    fs::write(dir.0.join("wrong.fn"), "fn wrong()->Int:true\n").unwrap();
    for args in [
        vec!["doc", "bad.fn", "--open"],
        vec!["doc", "wrong.fn", "--inferred", "--open"],
        vec!["doc", "library.fn", "--open", "-o", "absent/out.html"],
        vec!["doc", "library.fn", "--open", "--open"],
    ] {
        let result = dir.run(&args);
        assert!(!result.status.success(), "{result:?}");
        assert!(!dir.0.join("calls").exists());
    }
    assert!(!dir.0.join("fern-docs.html").exists());
}
#[test]
fn failed_or_missing_opener_is_visible_even_when_quiet_and_keeps_artifact() {
    let dir = Directory::new();
    dir.opener_failure(7);
    for flag in ["--quiet", "--verbose"] {
        let result = dir.run(&["doc", "library.fn", "--open", flag]);
        assert!(result.status.success(), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("open"));
        assert!(String::from_utf8_lossy(&result.stderr).contains('7'));
        assert!(dir.0.join("fern-docs.html").exists());
    }
    fs::remove_dir_all(dir.0.join("tools")).unwrap();
    let result = dir.run(&["doc", "library.fn", "--open"]);
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stderr).contains("note:"));
}
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_output_is_passed_as_literal_os_path() {
    let dir = Directory::new();
    let name = OsString::from_vec(b"docs-\xff.html".to_vec());
    let args = vec![
        "doc".into(),
        "library.fn".into(),
        "--open".into(),
        "-o".into(),
        name.clone(),
    ];
    let result = dir.run_os(args);
    assert!(result.status.success(), "{result:?}");
    dir.assert_opened(dir.0.join(name));
}
#[test]
fn byte_cap_precedes_utf8_validation_in_every_doc_mode() {
    let dir = Directory::new();
    let mut bytes = vec![b'#'; 1024 * 1024];
    bytes.extend_from_slice("🌿".as_bytes());
    fs::write(dir.0.join("library.fn"), bytes).unwrap();
    for args in [
        vec!["doc", "library.fn"],
        vec!["doc", "."],
        vec!["doc", "library.fn", "--inferred"],
        vec!["doc", ".", "--inferred"],
    ] {
        let result = dir.run(&args);
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("1 MiB"), "{error}");
        assert!(!error.contains("UTF-8"), "{error}");
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn output_budget_failure_preserves_previous_artifact_and_never_launches() {
    let dir = Directory::new();
    for index in 0..5 {
        fs::write(
            dir.0.join(format!("large{index}.fn")),
            format!(
                "@doc \"\"\"{}\"\"\"\nfn helper{index}():()\n",
                "&".repeat(700_000)
            ),
        )
        .unwrap();
    }
    fs::write(dir.0.join("fern-docs.html"), "retained").unwrap();
    let result = dir.run(&["doc", ".", "--open"]);
    assert!(!result.status.success(), "{result:?}");
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("output exceeds"),
        "{result:?}"
    );
    assert!(result.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(dir.0.join("fern-docs.html")).unwrap(),
        "retained"
    );
    assert!(!dir.0.join("calls").exists());
}
#[test]
fn exact_source_cap_and_invalid_utf8_retain_independent_diagnostics() {
    let dir = Directory::new();
    fs::write(dir.0.join("library.fn"), vec![b'#'; 1024 * 1024]).unwrap();
    let result = dir.run(&["doc", "library.fn", "--open"]);
    assert!(result.status.success(), "{result:?}");
    dir.assert_opened(dir.0.join("fern-docs.html"));
    fs::remove_file(dir.0.join("calls")).unwrap();
    let before = fs::read(dir.0.join("fern-docs.html")).unwrap();
    fs::write(dir.0.join("library.fn"), [0xff]).unwrap();
    let result = dir.run(&["doc", "library.fn", "--open"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid UTF-8"));
    assert_eq!(before, fs::read(dir.0.join("fern-docs.html")).unwrap());
    assert!(!dir.0.join("calls").exists());
}
