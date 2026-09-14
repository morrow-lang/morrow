use morrow_compiler::{check, lowering, modules};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow-module-control-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn let_else_failure_uses_outer_imports_while_success_names_stay_local() {
    let project = Project::new();
    project.write(
        "model.fn",
        "pub fn number() -> Int: 0\npub fn cleanup(value: Int) -> Unit: println(value)\n",
    );
    let path=project.write("main.fn","import model.{number, cleanup}\nfn unwrap(value: Option(Int)) -> Int:\n    defer cleanup(number())\n    let Some(number) = value else: return number()\n    match:\n        number > 0 -> return number\n        _ -> 0\nfn main(): println(unwrap(Some(42)))\n");
    let loaded = modules::load(&path).unwrap();
    lowering::emit(&check::check(&loaded.program).unwrap()).unwrap();
}

#[test]
fn control_children_cannot_bypass_private_module_names() {
    for statement in [
        "return model.secret()",
        "defer println(model.secret())",
        "println(1) if model.secret() == 0",
        "let Some(value) = None else: return model.secret()",
        "match:\n        model.secret() == 0 -> return 0\n        _ -> return 1",
    ] {
        let project = Project::new();
        project.write(
            "model.fn",
            "fn secret() -> Int: 1\npub fn visible() -> Int: 0\n",
        );
        let path = project.write(
            "main.fn",
            &format!("import model\nfn main() -> Int:\n    {statement}\n"),
        );
        assert!(
            modules::load(&path)
                .unwrap_err()
                .message
                .contains("private")
        );
    }
}
