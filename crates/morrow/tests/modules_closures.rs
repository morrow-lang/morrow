use morrow_compiler::{
    Type,
    ast::{ExprKind, Stmt},
    modules,
};
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
            "morrow-module-closures-{}-{}",
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
fn function_annotations_and_lambda_parameter_scopes_resolve_across_modules() {
    let project = Project::new();
    project.write(
        "model.fn",
        "pub type Box(a):\n    value: a\npub fn value() -> Int: 1\n",
    );
    let main=project.write("main.fn","import model\nfn apply(action: (model.Box(Int)) -> Int, item: model.Box(Int)) -> Int: action(item)\nfn main():\n    let callback = (model: Int) -> model + 1\n    println(callback(41))\n    println(apply((box: model.Box(Int)) -> box.value, model.Box(42)))\n");
    let loaded = modules::load(&main).unwrap();
    morrow_compiler::lowering::emit(&morrow_compiler::check::check(&loaded.program).unwrap())
        .unwrap();
    let function = loaded
        .program
        .functions
        .iter()
        .find(|f| f.name.ends_with(".apply") || f.name == "apply")
        .unwrap();
    assert!(
        matches!(function.params[0].annotation.as_ref().unwrap(),Type::Function(params,result) if matches!(&params[0],Type::Named(name,_) if name.contains("Box")) && **result==Type::Int)
    );
    let main = loaded
        .program
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap();
    let ExprKind::Block(statements) = &main.body.kind else {
        panic!()
    };
    let Stmt::Let { value, .. } = &statements[0] else {
        panic!()
    };
    let ExprKind::Lambda { params, body } = &value.kind else {
        panic!()
    };
    assert_eq!(params[0].name, "model");
    assert!(
        matches!(&body.kind,ExprKind::Binary{left,..} if matches!(&left.kind,ExprKind::Name(name) if name=="model"))
    );
}

#[test]
fn lambda_annotations_cannot_bypass_private_type_exports() {
    let project = Project::new();
    project.write(
        "model.fn",
        "type Hidden:\n    value: Int\npub fn number() -> Int: 1\n",
    );
    let main = project.write(
        "main.fn",
        "import model\nfn main():\n    let callback = (box: model.Hidden) -> box.value\n    0\n",
    );
    assert!(
        modules::load(&main)
            .unwrap_err()
            .message
            .contains("private")
    );
}
