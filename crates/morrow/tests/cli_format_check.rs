//! Formatter check mode is a read-only CI gate, including failure and alias paths.
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow fmt check {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn run(&self, args: &[&OsStr]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_morrow"))
            .current_dir(&self.0)
            .args(args)
            .env("MORROW_QBE", self.0.join("absent backend"))
            .env("MORROW_RUNTIME_LIB", self.0.join("absent runtime"))
            .output()
            .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn unchanged(path: &Path, bytes: &[u8], before: &fs::Metadata) {
    let after = fs::metadata(path).unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(after.mode(), before.mode());
        assert_eq!(after.ino(), before.ino());
    }
}

#[test]
fn clean_format_check_is_silent_read_only_and_accepts_both_flag_positions() {
    let directory = Directory::new();
    let source = directory.0.join("🌿 $(literal); source.fn");
    let canonical = morrow_compiler::format::format("fn main():println(\"🌿\")\n").unwrap();
    fs::write(&source, &canonical).unwrap();
    let before = fs::metadata(&source).unwrap();
    for args in [
        [OsStr::new("fmt"), OsStr::new("--check"), source.as_os_str()],
        [OsStr::new("fmt"), source.as_os_str(), OsStr::new("--check")],
    ] {
        let output = directory.run(&args);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
        unchanged(&source, canonical.as_bytes(), &before);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }
}

#[test]
fn drift_returns_one_with_source_diagnostic_and_never_formats_the_input() {
    let directory = Directory::new();
    let source = directory.0.join("drift.fn");
    let bytes = b"fn main():println(1)\n";
    fs::write(&source, bytes).unwrap();
    let before = fs::metadata(&source).unwrap();
    let output = directory.run(&["fmt".as_ref(), source.as_os_str(), "--check".as_ref()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("{}: formatting changes required\n", source.display())
    );
    unchanged(&source, bytes, &before);
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    assert!(
        directory
            .run(&["fmt".as_ref(), source.as_os_str()])
            .status
            .success()
    );
    assert!(
        directory
            .run(&["fmt".as_ref(), "--check".as_ref(), source.as_os_str()])
            .status
            .success()
    );
}

#[test]
fn malformed_and_invalid_utf8_sources_are_not_modified() {
    let directory = Directory::new();
    let source = directory.0.join("invalid.fn");
    for bytes in [b"fn main(:\n".as_slice(), &[0xff, b'\n']] {
        fs::write(&source, bytes).unwrap();
        let before = fs::metadata(&source).unwrap();
        let output = directory.run(&["fmt".as_ref(), "--check".as_ref(), source.as_os_str()]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty() && !output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("unknown option"));
        unchanged(&source, bytes, &before);
    }
}

#[test]
fn check_flag_is_scoped_to_fmt_and_rejects_duplicates_and_missing_sources() {
    let directory = Directory::new();
    let source = directory.0.join("source.fn");
    fs::write(&source, "fn main(): 0\n").unwrap();
    for command in ["check", "emit", "build", "run"] {
        let output = directory.run(&[command.as_ref(), "--check".as_ref(), source.as_os_str()]);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("--check is only valid for fmt"));
    }
    let duplicate = directory.run(&[
        "fmt".as_ref(),
        "--check".as_ref(),
        "--check".as_ref(),
        source.as_os_str(),
    ]);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&duplicate.stderr).contains("--check specified more than once")
    );
    let missing = directory.run(&["fmt".as_ref(), "--check".as_ref()]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing source file"));
}

#[cfg(unix)]
#[test]
fn check_preserves_symlink_and_read_only_target_identity() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = Directory::new();
    let source = directory.0.join("target.fn");
    let link = directory.0.join("link.fn");
    let bytes = b"fn main():println(1)\n";
    fs::write(&source, bytes).unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o444)).unwrap();
    symlink(&source, &link).unwrap();
    let before = fs::metadata(&source).unwrap();
    let output = directory.run(&["fmt".as_ref(), link.as_os_str(), "--check".as_ref()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("formatting changes required"));
    unchanged(&source, bytes, &before);
    assert_eq!(fs::read_link(&link).unwrap(), source);
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
}

#[test]
fn help_explains_non_writing_check_mode() {
    let output = Directory::new().run(&["--help".as_ref()]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("fmt --check"));
    assert!(help.contains("without writing"));
}
