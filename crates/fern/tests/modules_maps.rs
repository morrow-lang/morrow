use fern_compiler::{check, lowering, modules};
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
            "fern-module-maps-{}-{}",
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
fn map_values_callbacks_and_record_updates_resolve_imports() {
    let project = Project::new();
    project.write("model.fn", "pub type Box(a):\n    value: a\npub fn make(value: Int) -> Box(Int): Box(value)\npub fn key() -> String: \"answer\"\npub fn number() -> Int: 42\n");
    let path = project.write("main.fn", "import model\nfn main():\n    let values: Map(String, model.Box(Int)) = %{model.key(): model.make(1)}\n    let callbacks: Map(String, (model.Box(Int)) -> Int) = %{\n        model.key(): (item) ->\n            let updated = %{item | value: model.number()}\n            updated.value\n    }\n    println(Map.len(values) + Map.len(callbacks))\n");
    let loaded = modules::load(&path).unwrap();
    lowering::emit(&check::check(&loaded.program).unwrap()).unwrap();
}

#[test]
fn map_types_and_update_expressions_cannot_bypass_private_exports() {
    for expression in [
        "let values: Map(String, model.Hidden) = %{}",
        "let values = %{model.secret(): 42}",
        "let value = %{model.make() | value: model.secret()}",
    ] {
        let project = Project::new();
        project.write("model.fn", "type Hidden:\n    value: Int\nfn secret() -> Int: 1\npub fn make() -> Hidden: Hidden(0)\n");
        let path = project.write(
            "main.fn",
            &format!("import model\nfn main():\n    {expression}\n    0\n"),
        );
        let error = modules::load(&path).unwrap_err();
        assert!(error.message.contains("private"), "{error:?}");
    }
}

#[test]
fn map_type_name_cannot_be_shadowed_by_source_declarations() {
    let project = Project::new();
    let path = project.write("main.fn", "type Map:\n    value: Int\nfn main(): 0\n");
    assert!(
        modules::load(&path)
            .unwrap_err()
            .message
            .contains("reserved")
    );
}
