use fern_compiler::{Type, ast, modules};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fern-codec-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
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
fn static_targets_resolve_type_aliases_even_when_values_have_the_same_name() {
    let project = Project::new();
    project.write(
        "model.fn",
        "type User derive(Json):\n    name:String\npub type Data=User\npub fn Data()->Int:42\n",
    );
    let main = project.write(
        "main.fn",
        "import model as m\nfn main():\n    let Data=1\n    json.decode(\"1\",m.Data)\n",
    );
    let loaded = modules::load(&main).unwrap();
    let main = loaded
        .program
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap();
    let ast::ExprKind::Block(statements) = &main.body.kind else {
        panic!("block")
    };
    let ast::Stmt::Expr(value) = &statements[1] else {
        panic!("expression")
    };
    let ast::ExprKind::Call { args, .. } = &value.kind else {
        panic!("call")
    };
    assert!(
        matches!(&args[1].value.kind,ast::ExprKind::TypeTarget(Type::Named(name,args)) if name=="model.Data" && args.is_empty())
    );
}
#[test]
fn source_local_json_root_cannot_trigger_static_argument_reinterpretation() {
    let project = Project::new();
    let main = project.write(
        "main.fn",
        "fn main():\n    let json=1\n    json.decode(\"1\",Int)\n",
    );
    let loaded = modules::load(&main).unwrap();
    assert!(!format!("{:?}", loaded.program).contains("TypeTarget"));
}
#[test]
fn static_target_privacy_and_arbitrary_call_errors_are_located_before_value_rewriting() {
    let project = Project::new();
    project.write("model.fn", "type Hidden derive(Json):\n    value:Int\n");
    let main = project.write(
        "main.fn",
        "import model as m\nfn main():json.decode(\"1\",m.Hidden)\n",
    );
    let failure = modules::load(&main).unwrap_err();
    assert!(failure.message.contains("private"));
    let location = failure.location.unwrap();
    assert_eq!(
        &location.source[location.diagnostic.span.start..location.diagnostic.span.end],
        "m.Hidden"
    );
    let main = project.write(
        "main.fn",
        "fn effect():1\nfn main():json.decode(\"1\",effect())\n",
    );
    let error = modules::load(&main).unwrap_err();
    assert!(
        error.message.contains("target must be a type"),
        "{}",
        error.message
    );
}

#[test]
fn imported_derived_newtypes_keep_type_identity_and_shift_trait_spans() {
    use fern_compiler::{check, lowering};
    let project = Project::new();
    let module = "pub newtype Id derive(Json) = Packed(Int)\npub fn Id()->Int:99\n";
    project.write("model.fn", module);
    let main=project.write("main.fn","import model as m\nfn main() -> Result(Unit,json.Error):\n    let value=json.decode(\"42\",m.Id)?\n    println(value.0)\n    Ok(())\n");
    let loaded = modules::load(&main).unwrap();
    let newtype = &loaded.program.newtypes[0];
    assert_eq!(newtype.name, "model.Id");
    assert_eq!(
        newtype.derives[0].span.start - newtype.span.start,
        module.find("Json").unwrap() - module.find("newtype").unwrap()
    );
    lowering::emit(&check::check(&loaded.program).unwrap()).unwrap();
    project.write("model.fn", "newtype Id derive(Json) = Packed(Int)\n");
    assert!(
        modules::load(&main)
            .unwrap_err()
            .message
            .contains("private")
    );
}

#[test]
fn generic_static_targets_keep_module_identity_and_source_type_namespace() {
    let project = Project::new();
    project.write("codec.fn","pub fn read(text:String)->Result(a,json.Error):json.decode(text,a)\npub fn write(value:a)->Result(String,json.Error):json.encode(value)\n");
    let main=project.write("main.fn","import codec as c\nfn main()->Result(Unit,json.Error):\n    let value:Int=c.read(\"42\")?\n    println(c.write(value)?)\n    Ok(())\n");
    let loaded = modules::load(&main).unwrap();
    let typed = fern_compiler::check::check(&loaded.program).unwrap();
    fern_compiler::lowering::emit(&typed).unwrap();
}
