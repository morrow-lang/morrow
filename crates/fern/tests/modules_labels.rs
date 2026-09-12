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
            "fern-label-modules-{}-{}",
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
fn labels_survive_alias_reexports_without_canonical_root_capture() {
    let project = Project::new();
    let model = "module model\npub fn subtract(left:Int,right:Int)->Int:left-right\n";
    project.write("model.fn", model);
    project.write("api.fn", "module api\npub import model.{subtract}\n");
    let entry=project.write("main.fn","import api as m\nfn main():\n    let model=99\n    println(m.subtract(right:2,left:9))\n    println(9 |> m.subtract(right:2,left:_))\n");
    let loaded = modules::load(&entry).unwrap();
    let checked = fern_compiler::check::check(&loaded.program).unwrap();
    assert!(
        fern_compiler::lowering::emit(&checked)
            .unwrap()
            .contains("function")
    );
}
#[test]
fn external_label_spans_relocate_independently_from_local_pattern_names() {
    let project = Project::new();
    let model = "module model\npub fn choose(ενεργό true:Bool)->Int:1\npub fn choose(ενεργό false:Bool)->Int:0\n";
    let path = project.write("model.fn", model);
    let entry = project.write(
        "main.fn",
        "import model.{choose}\nfn main():println(choose(ενεργό:true))\n",
    );
    let loaded = modules::load(&entry).unwrap();
    fern_compiler::check::check(&loaded.program).unwrap();
    let source = loaded
        .sources()
        .find(|source| source.path == path.canonicalize().unwrap())
        .unwrap();
    let function = loaded
        .program
        .functions
        .iter()
        .find(|f| f.name == "model.choose")
        .unwrap();
    let label = function.params[0].label.as_ref().unwrap();
    assert_eq!(
        &source.text[label.span.start - source.start..label.span.end - source.start],
        "ενεργό"
    );
}
