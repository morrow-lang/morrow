use morrow_compiler::{ast::PatternKind, modules};
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
            "morrow-module-clauses-{}-{}",
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
fn clause_group_identity_and_scopes_survive_module_qualification() {
    let project = Project::new();
    project.write(
        "model.mr",
        "pub type Item:\n    value: Int\npub fn fallback() -> Int: 99\n",
    );
    project.write("api.mr", "import model.{Item,fallback}\n@doc \"\"\"All clauses.\"\"\"\npub fn choose([Item(fallback),..tail]: List(Item)) if fallback>0 -> Int: fallback+List.len(tail)\npub fn choose(_: List(Item)) -> Int: fallback()\n");
    let path = project.write(
        "main.mr",
        "import api.{choose}\nfn main(): println(choose([]))\n",
    );
    let loaded = modules::load(&path).unwrap();
    let clauses = loaded
        .program
        .functions
        .iter()
        .filter(|f| f.name.ends_with(".choose"))
        .collect::<Vec<_>>();
    assert_eq!(clauses.len(), 2);
    assert_eq!(clauses[0].group_start, clauses[1].group_start);
    assert_eq!(clauses[0].group_start, clauses[0].span.start);
    assert!(clauses[0].guard.is_some());
    let PatternKind::List { prefix, .. } = &clauses[0].params[0].pattern.kind else {
        panic!()
    };
    assert!(
        matches!(&prefix[0].kind,PatternKind::NamedConstructor{name,..} if name.ends_with(".Item"))
    );
    assert_eq!(loaded.program.docs[0].target, clauses[0].name);
}

#[test]
fn parameter_patterns_and_guards_cannot_expose_private_names() {
    let project = Project::new();
    project.write("model.mr", "type Hidden:\n    value: Int\nfn secret() -> Bool: true\npub type Public:\n    value: Int\n");
    for source in [
        "import model\nfn f(model.Hidden(x): model.Public) -> Int: x\nfn main(): 0\n",
        "import model\nfn f(x: Int) if model.secret() -> Int: x\nfn f(_: Int) -> Int: 0\nfn main(): 0\n",
    ] {
        let path = project.write("main.mr", source);
        assert!(
            modules::load(&path)
                .unwrap_err()
                .message
                .contains("private")
        );
    }
}
