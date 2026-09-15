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
            "morrow-module-iteration-{}-{}",
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
fn with_error_types_and_loop_bindings_resolve_in_their_own_scopes() {
    let project = Project::new();
    project.write("model.mr","pub type First:\n    FirstError(Int)\npub type Second:\n    SecondError(String)\npub fn start() -> Result(Int, First): Ok(1)\npub fn next(value: Int) -> Result(Int, Second): Ok(value + 1)\npub fn numbers() -> Range: 0..2\n");
    let path=project.write("main.mr","import model\nfn run() -> Int:\n    with\n        value <- model.start(),\n        next <- model.next(value)\n    do\n        for model in model.numbers():\n            println(model)\n        next\n    else\n        Err(model.FirstError(code)) -> code\n        Err(model.SecondError(text)) -> String.len(text)\nfn main(): println(run())\n");
    let loaded = modules::load(&path).unwrap();
    lowering::emit(&check::check(&loaded.program).unwrap()).unwrap();
}

#[test]
fn private_range_endpoints_with_handlers_and_iterables_remain_inaccessible() {
    for body in [
        "for value in model.secret()..2: println(value)",
        "with value <- model.start() do value else Err(_) -> model.secret()",
        "for value in [model.secret()]: println(value)",
    ] {
        let project = Project::new();
        project.write(
            "model.mr",
            "fn secret() -> Int: 0\npub fn start() -> Result(Int, String): Ok(1)\n",
        );
        let path = project.write("main.mr", &format!("import model\nfn main(): {body}\n"));
        assert!(
            modules::load(&path)
                .unwrap_err()
                .message
                .contains("private")
        );
    }
}
