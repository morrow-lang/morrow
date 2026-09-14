//! Project formatting validates the entire bounded input set before publication.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
const DIRTY: &str = "fn main():println(1)\n";
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow fmt directory {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
    fn run(&self, flags_first: bool, check: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_morrow"));
        command.arg("fmt");
        if check && flags_first {
            command.arg("--check");
        }
        command.arg(&self.0);
        if check && !flags_first {
            command.arg("--check");
        }
        command
            .env("MORROW_QBE", "/absent/qbe")
            .env("MORROW_RUNTIME_LIB", "/absent/runtime")
            .output()
            .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn unchanged(path: &Path, text: &str, before: &fs::Metadata) {
    let after = fs::metadata(path).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), text);
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!((after.mode(), after.ino()), (before.mode(), before.ino()));
    }
}
#[test]
fn check_reports_all_dirty_paths_sorted_without_mutating_anything() {
    let dir = Directory::new();
    let z = dir.write("z.fn", DIRTY);
    let a = dir.write("nested/a.fn", DIRTY);
    let clean_text = morrow_compiler::format::format(DIRTY).unwrap();
    let clean = dir.write("clean.fn", &clean_text);
    let before: Vec<_> = [&a, &z, &clean, &dir.0, a.parent().unwrap()]
        .into_iter()
        .map(|p| fs::metadata(p).unwrap())
        .collect();
    for flags_first in [true, false] {
        let out = dir.run(flags_first, true);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(out.stdout.is_empty());
        assert_eq!(
            String::from_utf8(out.stderr).unwrap(),
            format!(
                "{}: formatting changes required\n{}: formatting changes required\n",
                a.display(),
                z.display()
            )
        );
        unchanged(&a, DIRTY, &before[0]);
        unchanged(&z, DIRTY, &before[1]);
        unchanged(&clean, &clean_text, &before[2]);
        assert_eq!(
            fs::metadata(&dir.0).unwrap().modified().unwrap(),
            before[3].modified().unwrap()
        );
        assert_eq!(
            fs::metadata(a.parent().unwrap())
                .unwrap()
                .modified()
                .unwrap(),
            before[4].modified().unwrap()
        );
    }
}
#[test]
fn formats_nested_sources_preserving_permissions_then_check_is_silent() {
    let dir = Directory::new();
    let a = dir.write("nested/a.fn", DIRTY);
    let z = dir.write("z.fn", DIRTY);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&a, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let clean = morrow_compiler::format::format(DIRTY).unwrap();
    let out = dir.run(false, false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("Formatted {}\nFormatted {}\n", a.display(), z.display())
    );
    assert_eq!(fs::read_to_string(&a).unwrap(), clean);
    assert_eq!(fs::read_to_string(&z).unwrap(), clean);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&a).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
    let before = fs::metadata(&a).unwrap();
    let out = dir.run(false, true);
    assert!(
        out.status.success() && out.stdout.is_empty() && out.stderr.is_empty(),
        "{out:?}"
    );
    assert!(dir.run(false, false).status.success());
    unchanged(&a, &clean, &before);
}
#[test]
fn ignores_non_sources_hidden_build_dependencies_and_child_symlinks() {
    let dir = Directory::new();
    let source = dir.write("main.fn", DIRTY);
    for name in [
        ".hidden.fn",
        ".git/a.fn",
        "target/a.fn",
        "build/a.fn",
        "bin/a.fn",
        "deps/a.fn",
        "node_modules/a.fn",
        "README.md",
    ] {
        dir.write(name, "malformed");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&dir.0, dir.0.join("cycle")).unwrap();
        symlink(dir.0.join(".hidden.fn"), dir.0.join("linked.fn")).unwrap();
    }
    let out = dir.run(false, false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        fs::read_to_string(source).unwrap(),
        morrow_compiler::format::format(DIRTY).unwrap()
    );
    assert_eq!(
        fs::read_to_string(dir.0.join(".hidden.fn")).unwrap(),
        "malformed"
    );
}
#[test]
fn later_malformed_input_prevents_all_writes_and_reports_its_path() {
    let dir = Directory::new();
    let a = dir.write("a.fn", DIRTY);
    let z = dir.write("z.fn", "fn main(:\n");
    let before = fs::metadata(&a).unwrap();
    for check in [true, false] {
        let out = dir.run(false, check);
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&format!("{}:", z.display())),
            "{out:?}"
        );
        unchanged(&a, DIRTY, &before);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
    }
}
#[test]
fn empty_directory_and_file_count_limit_have_formatting_diagnostics() {
    let dir = Directory::new();
    let out = dir.run(false, false);
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("formatting directory contains no Morrow source files"),
        "{out:?}"
    );
    let canonical = morrow_compiler::format::format(DIRTY).unwrap();
    for index in 0..256 {
        dir.write(&format!("{index:03}.fn"), &canonical);
    }
    assert!(dir.run(false, true).status.success());
    dir.write("overflow.fn", DIRTY);
    let out = dir.run(false, false);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("formatting file limit exceeds 256"),
        "{out:?}"
    );
    assert_eq!(
        fs::read_to_string(dir.0.join("overflow.fn")).unwrap(),
        DIRTY
    );
}
#[test]
fn source_and_aggregate_byte_limits_prevent_publication() {
    let dir = Directory::new();
    let a = dir.write("a.fn", DIRTY);
    let before = fs::metadata(&a).unwrap();
    dir.write("z.fn", &format!("#{}\n", "x".repeat(1024 * 1024)));
    let out = dir.run(false, false);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("1 MiB"),
        "{out:?}"
    );
    unchanged(&a, DIRTY, &before);
    fs::remove_file(dir.0.join("z.fn")).unwrap();
    let large = format!("#{}\n", "x".repeat(1024 * 1024 - 2));
    for index in 0..8 {
        dir.write(&format!("{index}.fn"), &large);
    }
    let out = dir.run(false, false);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("formatting source exceeds 8 MiB"),
        "{out:?}"
    );
    unchanged(&a, DIRTY, &before);
}
#[test]
fn excessive_directory_depth_prevents_writes() {
    let dir = Directory::new();
    let a = dir.write("a.fn", DIRTY);
    let nested = format!("{}z.fn", "d/".repeat(33));
    dir.write(&nested, DIRTY);
    let out = dir.run(false, false);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("formatting directory nesting exceeds 32"),
        "{out:?}"
    );
    assert_eq!(fs::read_to_string(a).unwrap(), DIRTY);
}
#[cfg(unix)]
#[test]
fn explicit_root_directory_link_is_supported_without_following_child_links() {
    use std::os::unix::fs::symlink;
    let outer = Directory::new();
    let source = outer.write("actual/main.fn", DIRTY);
    symlink(outer.0.join("actual"), outer.0.join("alias")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_morrow"))
        .arg("fmt")
        .arg(outer.0.join("alias"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(
        fs::symlink_metadata(outer.0.join("alias"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(source).unwrap(),
        morrow_compiler::format::format(DIRTY).unwrap()
    );
}

#[test]
fn source_at_exact_limit_and_aggregate_at_exact_limit_are_accepted() {
    let dir = Directory::new();
    let large = format!("#{}\n", "x".repeat(1024 * 1024 - 2));
    for index in 0..8 {
        dir.write(&format!("{index}.fn"), &large);
    }
    let out = dir.run(false, true);
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn directory_entry_budget_includes_skipped_entries() {
    let dir = Directory::new();
    let source = dir.write("source.fn", DIRTY);
    for index in 0..8192 {
        dir.write(&format!(".ignored{index}"), "");
    }
    let out = dir.run(false, false);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("formatting directory entry limit exceeded"),
        "{out:?}"
    );
    assert_eq!(fs::read_to_string(source).unwrap(), DIRTY);
}

#[test]
fn output_expansion_is_bounded_before_any_file_is_staged() {
    let dir = Directory::new();
    // Each source is just under one MiB; canonical indentation expands the total over eight.
    let body = "fn f():1\n".repeat(20);
    let source = format!("#{}\n{}", "x".repeat(1024 * 1024 - body.len() - 152), body);
    let canonical = morrow_compiler::format::format(&source).unwrap();
    assert!(canonical.len() > source.len());
    // Keep individual canonical files valid, then fill the remaining aggregate space.
    for index in 0..8 {
        dir.write(&format!("{index}.fn"), &source);
    }
    let remaining = 8 * 1024 * 1024 - source.len() * 8;
    dir.write("tail.fn", &format!("#{}\n", "x".repeat(remaining - 2)));
    let out = dir.run(false, false);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("formatting output exceeds 8 MiB"),
        "{out:?}"
    );
    assert_eq!(fs::read_to_string(dir.0.join("0.fn")).unwrap(), source);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 9);
}
