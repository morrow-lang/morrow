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
fn exported_newtype_constructors_and_private_newtype_aliases_keep_module_identity() {
    let project = Project::new();
    project.write("ids.fn", "module ids\npub newtype UserId = UserId(Int)\nnewtype Secret = Secret(String)\npub type Token = Secret\npub fn token() -> Token: Secret(\"secret\")\n");
    let main=project.write("main.fn", "module main\nimport ids.{UserId, Token, token}\nfn raw(value: UserId) -> Int: value.0\nfn main():\n    println(raw(UserId(42)))\n    let token: Token = token()\n    println(token.0)\n");
    let loaded = modules::load(&main).unwrap();
    morrow_compiler::lowering::emit(&morrow_compiler::check::check(&loaded.program).unwrap())
        .unwrap();
    let main = project.write(
        "main.fn",
        "module main\nimport ids.{Secret}\nfn main(): ()\n",
    );
    assert!(modules::load(&main).is_err());
}

#[test]
fn distinct_constructors_survive_selected_imports_and_public_reexports() {
    let project = Project::new();
    project.write("ids.fn", "module ids\npub newtype Wrapper(a)=Packed(a)\n");
    project.write(
        "facade.fn",
        "module facade\npub import ids.{Wrapper, Packed}\n",
    );
    let main=project.write("main.fn","import facade as api\nfn main():\n    let id: api.Wrapper(Int)=api.Packed(42)\n    println(id.0)\n");
    let loaded = modules::load(&main).unwrap();
    let declaration = &loaded.program.newtypes[0];
    assert_eq!(declaration.name, "ids.Wrapper");
    assert_eq!(declaration.constructor, "ids.Packed");
    assert_eq!(
        loaded
            .locate_span(declaration.constructor_span)
            .unwrap()
            .1
            .start,
        "module ids\npub newtype Wrapper(a)=Packed(a)\n"
            .find("Packed")
            .unwrap()
    );
    morrow_compiler::check::check(&loaded.program).unwrap();
    let main = project.write(
        "main.fn",
        "import ids.{Wrapper}\nfn main(): println(Wrapper.Packed(1).0)\n",
    );
    morrow_compiler::check::check(&modules::load(&main).unwrap().program).unwrap();
}

#[test]
fn public_aliases_do_not_export_private_newtype_constructors() {
    let project = Project::new();
    project.write("ids.fn","module ids\nnewtype Secret=Hidden(Int)\npub type Token=Secret\npub fn token() -> Token: Hidden(1)\n");
    for expression in ["ids.Hidden(1)", "ids.Secret(1)", "ids.Token.Hidden(1)"] {
        let source = format!("import ids\nfn main(): println(({expression}).0)\n");
        let main = project.write("main.fn", &source);
        assert!(modules::load(&main).is_err(), "{source}");
    }
}
