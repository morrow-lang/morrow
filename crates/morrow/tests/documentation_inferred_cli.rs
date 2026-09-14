use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow-inferred-doc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, source: &str) {
        fs::write(self.0.join(path), source).unwrap();
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_morrow"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inferred_cli_resolves_imports_and_directory_names_without_running_examples() {
    let p = Project::new();
    p.write("helper.fn", "pub fn identity(x:a)->a:x\n");
    p.write("library.fn","import helper as h\n@doc \"\"\"Library docs.\"\"\"\nfn answer():h.identity(42)\nfn main():println(\"DO NOT EXECUTE\")\n");
    let result = p.run(&["doc", "library.fn", "--inferred"]);
    assert!(result.status.success(), "{:?}", result);
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("fn answer() -> Int"), "{text}");
    assert!(!text.contains("DO NOT EXECUTE"));
    let result = p.run(&["doc", ".", "--inferred", "--html", "-o", "docs.html"]);
    assert!(result.status.success(), "{:?}", result);
    let text = fs::read_to_string(p.0.join("docs.html")).unwrap();
    assert!(text.contains("fn identity(x: a) -&gt; a"));
    assert!(text.contains("fn answer() -&gt; Int"));
    assert!(text.contains("browser’s Find command"));
    assert!(!text.contains("<script"));
}

#[test]
fn invalid_inferred_docs_preserve_output_and_source_only_docs_still_work() {
    let p = Project::new();
    p.write("library.fn", "fn broken(x:a)->a:42\n");
    p.write("docs.md", "prior docs");
    assert!(
        !p.run(&["doc", "library.fn", "--inferred", "-o", "docs.md"])
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(p.0.join("docs.md")).unwrap(),
        "prior docs"
    );
    assert!(p.run(&["doc", "library.fn"]).status.success());
    assert!(
        !p.run(&["doc", "library.fn", "--inferred", "--inferred"])
            .status
            .success()
    );
}

#[test]
fn inferred_output_cannot_replace_an_imported_dependency_or_its_hardlink() {
    let p = Project::new();
    let helper = "pub fn value()->Int:42\n";
    p.write("helper.fn", helper);
    p.write("library.fn", "import helper as h\nfn value():h.value()\n");
    fs::hard_link(p.0.join("helper.fn"), p.0.join("alias.md")).unwrap();
    for output in ["helper.fn", "alias.md"] {
        let result = p.run(&["doc", "library.fn", "--inferred", "-o", output]);
        assert!(!result.status.success(), "{:?}", result);
        assert_eq!(fs::read_to_string(p.0.join("helper.fn")).unwrap(), helper);
    }
}

#[test]
fn checked_directory_can_exceed_one_graphs_128_file_limit() {
    let p = Project::new();
    for index in 0..129 {
        p.write(&format!("m{index}.fn"), "fn value():42\n");
    }
    let result = p.run(&["doc", ".", "--inferred", "--html"]);
    assert!(result.status.success(), "{:?}", result);
    let text = String::from_utf8(result.stdout).unwrap();
    assert_eq!(text.matches("<p class=\"checked\">").count(), 129);
}

#[test]
fn independent_modules_do_not_copy_every_unrelated_snapshot_for_each_graph() {
    let p = Project::new();
    let source = format!("# {}\nfn value():42\n", "x".repeat(2048));
    for index in 0..129 {
        p.write(&format!("m{index}.fn"), &source);
    }
    let result = p.run(&["doc", ".", "--inferred", "--html"]);
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout)
            .matches("<p class=\"checked\">")
            .count(),
        129
    );
}
