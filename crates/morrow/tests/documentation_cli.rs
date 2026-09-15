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
            "morrow-docs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("library.mr"),
            "@doc \"\"\"A helper.\"\"\"\npub fn helper(value: Int) -> Int: value + 1\n",
        )
        .unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_morrow"))
            .current_dir(&self.0)
            .args(args)
            .env("MORROW_QBE", self.0.join("missing-backend"))
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
fn doc_prints_markdown_without_main_or_backend_and_writes_html() {
    let dir = Directory::new();
    let result = dir.run(&["doc", "library.mr"]);
    assert!(result.status.success(), "{:?}", result);
    assert!(String::from_utf8_lossy(&result.stdout).contains("A helper."));
    assert!(result.stderr.is_empty());
    let result = dir.run(&["doc", "--html", "-o", "docs.html", "library.mr"]);
    assert!(result.status.success(), "{:?}", result);
    assert!(
        fs::read_to_string(dir.0.join("docs.html"))
            .unwrap()
            .starts_with("<!doctype html>")
    );
    assert!(result.stdout.is_empty());
}
#[test]
fn doc_invalid_source_preserves_output_and_reports_source_location() {
    let dir = Directory::new();
    fs::write(dir.0.join("bad.mr"), "fn bad():\n    (\n").unwrap();
    fs::write(dir.0.join("output.md"), "preserved").unwrap();
    let result = dir.run(&["doc", "bad.mr", "-o", "output.md"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("bad.mr:"));
    assert_eq!(
        fs::read_to_string(dir.0.join("output.md")).unwrap(),
        "preserved"
    );
}
#[test]
fn doc_rejects_source_aliases_and_malformed_options() {
    let dir = Directory::new();
    let original = fs::read(dir.0.join("library.mr")).unwrap();
    fs::hard_link(dir.0.join("library.mr"), dir.0.join("hard.mr")).unwrap();
    for destination in ["library.mr", "hard.mr"] {
        let result = dir.run(&["doc", "library.mr", "-o", destination]);
        assert_eq!(result.status.code(), Some(1), "{:?}", result);
        assert_eq!(fs::read(dir.0.join("library.mr")).unwrap(), original);
    }
    for args in [
        vec!["doc", "library.mr", "-o"],
        vec!["doc", "--html", "--html", "library.mr"],
        vec!["doc", "library.mr", "extra.mr"],
        vec!["doc", "--unknown", "library.mr"],
    ] {
        assert_eq!(dir.run(&args).status.code(), Some(1));
    }
}
#[cfg(unix)]
#[test]
fn doc_preserves_source_when_output_is_a_symlink_to_it() {
    let dir = Directory::new();
    std::os::unix::fs::symlink("library.mr", dir.0.join("alias.mr")).unwrap();
    let original = fs::read(dir.0.join("library.mr")).unwrap();
    let result = dir.run(&["doc", "library.mr", "-o", "alias.mr"]);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read(dir.0.join("library.mr")).unwrap(), original);
}

#[test]
fn doc_help_describes_formats_and_source_only_generation() {
    let dir = Directory::new();
    let result = dir.run(&["doc", "--help"]);
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("--html"));
    assert!(text.contains("-o"));
    assert!(text.contains("source"));
}

#[test]
fn doc_directory_orders_modules_and_excludes_hidden_build_and_symlink_entries() {
    let dir = Directory::new();
    fs::create_dir(dir.0.join("nested")).unwrap();
    fs::write(dir.0.join("nested/second.mr"), "fn second(): ()\n").unwrap();
    for name in [".hidden", "target", "build", "deps", "node_modules"] {
        fs::create_dir(dir.0.join(name)).unwrap();
        fs::write(dir.0.join(name).join("bad.mr"), "fn broken(:").unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(".", dir.0.join("cycle")).unwrap();
    let result = dir.run(&["doc", ".", "--html", "-o", "docs.html"]);
    assert!(result.status.success(), "{:?}", result);
    let html = fs::read_to_string(dir.0.join("docs.html")).unwrap();
    assert!(html.contains("nested/second.mr"));
    assert!(html.contains("browser’s Find command"));
    assert!(html.contains("<kbd>Ctrl+F</kbd>"));
    assert!(html.contains("<kbd>Command+F</kbd>"));
    assert!(html.contains("<h3>second</h3>"));
    assert!(!html.contains("<script"));
    assert!(!html.contains(" hidden"));
    assert!(html.find("library.mr").unwrap() < html.find("nested/second.mr").unwrap());
}

#[test]
fn doc_directory_error_preserves_output_and_all_source_aliases() {
    let dir = Directory::new();
    fs::write(dir.0.join("output.html"), "preserved").unwrap();
    fs::write(dir.0.join("bad.mr"), "fn bad(:").unwrap();
    let result = dir.run(&["doc", ".", "--html", "-o", "output.html"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("bad.mr:1:"));
    assert_eq!(
        fs::read_to_string(dir.0.join("output.html")).unwrap(),
        "preserved"
    );
    fs::remove_file(dir.0.join("bad.mr")).unwrap();
    fs::hard_link(dir.0.join("library.mr"), dir.0.join("alias.html")).unwrap();
    let original = fs::read(dir.0.join("library.mr")).unwrap();
    for output in ["library.mr", "alias.html"] {
        assert_eq!(dir.run(&["doc", ".", "-o", output]).status.code(), Some(1));
        assert_eq!(fs::read(dir.0.join("library.mr")).unwrap(), original);
    }
}

#[test]
fn doc_directory_rejects_empty_and_excessive_sources_without_partial_output() {
    let dir = Directory::new();
    fs::create_dir(dir.0.join("empty")).unwrap();
    assert_eq!(dir.run(&["doc", "empty"]).status.code(), Some(1));
    for i in 0..256 {
        fs::write(dir.0.join(format!("file{i}.mr")), "fn helper(): ()\n").unwrap();
    }
    let result = dir.run(&["doc", ".", "-o", "output.html"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!dir.0.join("output.html").exists());
}

#[cfg(unix)]
#[test]
fn directory_paths_preserve_literal_backslashes_in_unix_filenames() {
    let dir = Directory::new();
    fs::write(dir.0.join(r"literal\name.mr"), "fn helper(): ()\n").unwrap();
    let result = dir.run(&["doc", ".", "--html"]);
    assert!(result.status.success(), "{:?}", result);
    assert!(String::from_utf8_lossy(&result.stdout).contains(r"literal\name.mr"));
}
