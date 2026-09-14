use morrow_compiler::modules;
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
            "morrow-module-visibility-{}-{}",
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
fn public_origin_survives_qualification_reexports_and_loader_flattening() {
    let project = Project::new();
    project.write(
        "model.fn",
        "fn hidden(): 42\npub fn visible() -> Int: hidden()\n",
    );
    project.write("facade.fn", "pub import model.{visible}\nfn local(): 1\n");
    let main = project.write(
        "main.fn",
        "import facade\npub fn api() -> Int: facade.visible()\nfn main(): println(api())\n",
    );
    let loaded = modules::load(&main).unwrap();
    let functions = &loaded.program.functions;
    for name in ["model.hidden", "facade.local", "main"] {
        assert!(
            !functions.iter().find(|f| f.name == name).unwrap().public,
            "{name}"
        );
    }
    for name in ["model.visible", "main.api"] {
        assert!(
            functions.iter().find(|f| f.name == name).unwrap().public,
            "{name}"
        );
    }
    assert_eq!(
        functions
            .iter()
            .filter(|f| f.name == "model.visible")
            .count(),
        1
    );
}
