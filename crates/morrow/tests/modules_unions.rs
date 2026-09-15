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
            "morrow-modules-{}-{}",
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
fn union_aliases_reexports_and_same_spelled_functions_keep_namespace_identity() {
    let p = Project::new();
    p.write(
        "model.mr",
        "pub type Choice=String | Int\npub fn Choice(x:Int)->Int:x+1\npub fn value()->Choice:41\n",
    );
    p.write("api.mr", "pub import model.{Choice,value}\n");
    let main=p.write("main.mr","import api as m\nfn size(x:m.Choice)->Int:\n    match x:\n        n:Int -> m.Choice(n)\n        s:String -> String.len(s)\nfn main():\n    let model=1\n    println(size(m.value())+model)\n");
    morrow_compiler::check::check(&modules::load(&main).unwrap().program).unwrap();
}
#[test]
fn typed_pattern_annotations_cannot_bypass_private_union_members() {
    let p = Project::new();
    p.write("model.mr","newtype Secret=Hidden(Int)\npub type Choice=Secret | Int\npub fn value()->Choice:Hidden(1)\n");
    let main=p.write("main.mr","import model as m\nfn size(x:m.Choice)->Int:\n    match x:\n        n:Int -> n\n        hidden:m.Secret -> hidden.0\nfn main():()\n");
    let error = modules::load(&main).unwrap_err();
    assert!(
        error.message.contains("private") || error.message.contains("not exported"),
        "{error:?}"
    );
    assert!(error.location.is_some());
}
#[test]
fn typed_payload_binders_shadow_globals_in_their_guard_and_body_only() {
    let p = Project::new();
    p.write(
        "model.mr",
        "pub fn value()->Int:7\npub type Choice=Int | String\n",
    );
    let main=p.write("main.mr","import model\nfn inspect(x:model.Choice)->Int:\n    match x:\n        model:Int if model>0 -> model\n        _:Int -> 0\n        _:String -> model.value()\nfn main():println(inspect(1))\n");
    morrow_compiler::check::check(&modules::load(&main).unwrap().program).unwrap();
}
