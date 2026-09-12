use fern_compiler::modules;
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
            "fern-modules-{}-{}",
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
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn imported_aliases_expand_in_their_definition_scope() {
    let project = Project::new();
    project.write("values.fn", "module values\ntype Hidden = Int\npub type Id = Hidden\npub type Pair(a) = (Id, a)\npub fn make(x: Id) -> Pair(String): (x, \"s\")\n");
    let main = project.write("main.fn", "module main\nimport values as v\ntype Hidden = Bool\nfn use(x: v.Pair(String)) -> Int: x.0\nfn main(): println(use(v.make(42)))\n");
    let loaded = modules::load(&main).unwrap();
    let checked = fern_compiler::check::check(&loaded.program).unwrap();
    fern_compiler::lowering::emit(&checked).unwrap();
}
#[test]
fn alias_exports_do_not_export_private_constructors_or_types() {
    let project = Project::new();
    project.write("values.fn", "module values\ntype Hidden:\n    value: Int\npub type Public = Hidden\npub fn make() -> Public: Hidden(42)\n");
    let main = project.write("main.fn", "module main\nimport values.{Public, make}\nfn use(x: Public) -> Int: x.value\nfn main(): println(use(make()))\n");
    let loaded = modules::load(&main).unwrap();
    fern_compiler::check::check(&loaded.program).unwrap();
    let main = project.write(
        "main.fn",
        "module main\nimport values.{Hidden}\nfn main(): ()\n",
    );
    assert!(modules::load(&main).is_err());
}
#[test]
fn aliases_participate_in_namespace_conflicts_and_source_locations() {
    let project = Project::new();
    project.write("values.fn", "module values\npub type Invalid = Missing\n");
    let main = project.write("main.fn", "module main\nimport values\nfn main(): ()\n");
    let loaded = modules::load(&main).unwrap();
    let error = fern_compiler::check::check(&loaded.program).unwrap_err();
    assert!(error.message.contains("Missing"));
    let location = loaded.locate(error).unwrap();
    assert!(location.path.ends_with("values.fn"));
    assert_eq!(
        &location.source[location.diagnostic.span.start..location.diagnostic.span.end],
        "type Invalid = Missing"
    );
    let main = project.write(
        "main.fn",
        "type Id = Int\nfn Id() -> Int: 1\nfn main(): ()\n",
    );
    fern_compiler::check::check(&modules::load(&main).unwrap().program).unwrap();
    let main = project.write("main.fn", "type Id=Int\ntype Id=String\nfn main():()\n");
    assert!(modules::load(&main).is_err());
}
