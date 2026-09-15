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
            "morrow-module-sequences-{}-{}",
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
fn sequence_patterns_qualify_nested_constructors_and_keep_suffix_bindings_local() {
    let project = Project::new();
    project.write(
        "model.mr",
        "pub type Item:\n    value: Int\npub fn tail() -> Int: 99\n",
    );
    let path=project.write("main.mr", "import model.{Item,tail}\nfn read() -> Int:\n    match [Item(1),Item(2)]:\n        [Item(value), ..tail] if List.len(tail)>0 -> value\n        _ -> 0\nfn main():\n    println(read())\n    println(tail())\n");
    let loaded = modules::load(&path).unwrap();
    lowering::emit(&check::check(&loaded.program).unwrap()).unwrap();
}

#[test]
fn let_else_failure_sees_outer_names_and_suffix_constructor_privacy_is_preserved() {
    let project = Project::new();
    project.write("model.mr", "type Hidden:\n    value: Int\npub fn tail() -> Int: 99\npub fn values() -> List(Int): [1,2]\n");
    let path=project.write("main.mr", "import model.{tail,values}\nfn read() -> Int:\n    let [first,..tail]=values() else: return tail()\n    first+List.len(tail)\nfn main(): println(read())\n");
    let loaded = modules::load(&path).unwrap();
    lowering::emit(&check::check(&loaded.program).unwrap()).unwrap();
    let path=project.write("main.mr", "import model\nfn main(): match []:\n    [model.Hidden(value),..rest] -> value\n    _ -> 0\n");
    assert!(
        modules::load(&path)
            .unwrap_err()
            .message
            .contains("private")
    );
}
