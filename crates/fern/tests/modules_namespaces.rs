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

fn checked(project: &Project, source: &str) {
    let main = project.write("main.fn", source);
    let loaded = modules::load(&main).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    fern_compiler::check::check(&loaded.program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
}
fn rejected(project: &Project, source: &str) {
    let main = project.write("main.fn", source);
    if let Ok(loaded) = modules::load(&main) {
        assert!(
            fern_compiler::check::check(&loaded.program).is_err(),
            "{source}"
        );
    }
}
#[test]
fn public_type_does_not_publish_same_spelled_private_function() {
    for declaration in [
        "pub newtype Id=Wrap(Int)",
        "pub type Id:\n    Wrap(Int)",
        "pub type Id=Int",
    ] {
        let project = Project::new();
        project.write(
            "ids.fn",
            &format!("module ids\n{declaration}\nfn Id(value:Int)->Int:value\n"),
        );
        checked(
            &project,
            "import ids.{Id}\nfn keep(value:Id)->Id:value\nfn main():()\n",
        );
        rejected(&project, "import ids\nfn main():println(ids.Id(1))\n");
    }
}
#[test]
fn public_function_does_not_publish_same_spelled_private_type() {
    for declaration in [
        "newtype Id=Wrap(Int)",
        "type Id:\n    Wrap(Int)",
        "type Id=Int",
    ] {
        let project = Project::new();
        project.write(
            "ids.fn",
            &format!("module ids\n{declaration}\npub fn Id(value:Int)->Int:value\n"),
        );
        checked(&project, "import ids.{Id}\nfn main():println(Id(1))\n");
        rejected(
            &project,
            "import ids\nfn keep(value:ids.Id)->ids.Id:value\nfn main():()\n",
        );
        rejected(&project, "import ids\nfn main():println(ids.Wrap(1))\n");
    }
}
#[test]
fn both_namespaces_survive_reexports_and_lexical_shadowing() {
    for import in ["import api as m", "import api.{Id}", "import api.*"] {
        let project = Project::new();
        project.write(
            "ids.fn",
            "module ids\npub type Id(a)=a\npub fn Id(value:a)->a:value\n",
        );
        project.write("api.fn", "module api\npub import ids.{Id}\n");
        let name = if import.contains(" as ") {
            "m.Id"
        } else {
            "Id"
        };
        checked(
            &project,
            &format!(
                "{import}\nfn main():\n    let ids=3\n    let value:{name}(Int)={name}(1)\n    let callback={name}\n    println(value |> {name})\n    println(callback(ids))\n"
            ),
        );
    }
}
#[test]
fn imports_collide_only_within_their_namespace() {
    let project = Project::new();
    project.write("types.fn", "module types\npub type Item=Int\n");
    project.write("values.fn", "module values\npub fn Item(x:Int)->Int:x\n");
    checked(
        &project,
        "import types.{Item}\nimport values.{Item}\nfn main():\n    let value:Item=Item(1)\n    println(value)\n",
    );
    project.write("other.fn", "module other\npub type Item=String\n");
    rejected(
        &project,
        "import types.{Item}\nimport other.{Item}\nfn main():()\n",
    );
}
#[test]
fn alias_and_unrelated_constructor_share_spelling_without_minting_alias_values() {
    let project = Project::new();
    project.write(
        "ids.fn",
        "module ids\npub type Tag=Int\npub type Value:\n    Tag(Int)\n",
    );
    checked(
        &project,
        "import ids.{Tag, Value}\nfn main():\n    let number:Tag=1\n    let value:Value=Tag(number)\n    match value:\n        Tag(x)->println(x)\n",
    );
}
#[test]
fn formatting_and_docs_preserve_independent_visibility() {
    for declaration in [
        "newtype Id=Wrap(Int)",
        "type Id:\n    Wrap(Int)",
        "type Id=Int",
    ] {
        let source = format!("{declaration}\npub fn Id(x:Int)->Int:x\n");
        let formatted = fern_compiler::format::format(&source).unwrap();
        assert!(!formatted.starts_with("pub "), "{formatted}");
        assert!(formatted.contains("pub fn Id"), "{formatted}");
    }
}

#[test]
fn duplicate_selectors_and_same_value_imports_remain_errors() {
    let project = Project::new();
    project.write("ids.fn", "pub type Id=Int\npub fn make()->Int:1\n");
    rejected(&project, "import ids.{Id, Id}\nfn main():()\n");
    project.write("other.fn", "pub fn make()->Int:2\n");
    rejected(
        &project,
        "import ids.{make}\nimport other.{make}\nfn main():()\n",
    );
}

#[test]
fn privacy_survives_selected_and_wildcard_public_reexports() {
    for import in ["pub import ids.{Id}", "pub import ids.*"] {
        let project = Project::new();
        project.write("ids.fn", "pub type Id=Int\nfn Id(x:Int)->Int:x\n");
        project.write("api.fn", &format!("{import}\n"));
        checked(
            &project,
            "import api\nfn keep(x:api.Id)->api.Id:x\nfn main():()\n",
        );
        rejected(&project, "import api\nfn main():println(api.Id(1))\n");
        project.write("ids.fn", "type Id=Int\npub fn Id(x:Int)->Int:x\n");
        checked(&project, "import api\nfn main():println(api.Id(1))\n");
        rejected(
            &project,
            "import api\nfn keep(x:api.Id)->api.Id:x\nfn main():()\n",
        );
        rejected(
            &project,
            "import api\nfn keep(x:ids.Id)->ids.Id:x\nfn main():()\n",
        );
    }
}

#[test]
fn real_local_receivers_shadow_values_but_not_type_annotations() {
    let project = Project::new();
    project.write("ids.fn", "pub type Id=Int\npub fn Id(x:Int)->Int:x\n");
    checked(
        &project,
        "import ids\ntype Receiver:\n    Id:(Int)->Int\nfn main():\n    let ids=Receiver((x)->x+1)\n    let value:ids.Id=ids.Id(1)\n    println(value)\n",
    );
    rejected(
        &project,
        "import ids\nfn main():\n    let ids=3\n    println(ids.Id(1))\n",
    );
}

#[test]
fn declaration_provenance_and_docs_do_not_use_spelling_only_exports() {
    use fern_compiler::{documentation, parse};
    let source = "type Alias=Int\npub fn Alias(x:Int)->Int:x\npub newtype Wrap=Packed(Int)\nfn Wrap(x:Int)->Int:x\npub type Sum:\n    Present(Int)\nfn Sum(x:Int)->Int:x\n";
    let program = parse::parse(source).unwrap();
    assert!(!program.aliases[0].public);
    assert!(program.newtypes[0].public && program.types[0].public);
    assert!(program.functions[0].public);
    assert!(!program.functions[1].public && !program.functions[2].public);
    let docs = documentation::render(source, "Library", documentation::Output::Markdown).unwrap();
    assert!(!docs.contains("pub type Alias"), "{docs}");
    assert!(docs.contains("pub newtype Wrap"), "{docs}");
    assert!(!docs.contains("pub fn Wrap"), "{docs}");
    let formatted = fern_compiler::format::format(source).unwrap();
    assert_eq!(
        formatted,
        fern_compiler::format::format(&formatted).unwrap()
    );
}

#[test]
fn editor_budget_charges_type_and_value_visibility_together_before_copying() {
    let project = Project::new();
    let name = "X".repeat(65_536);
    project.write(
        "base.fn",
        &format!("pub type {name}=Int\npub fn {name}()->Int:1\n"),
    );
    let mut source = String::new();
    for i in 0..32 {
        source.push_str(&format!("import base as m{i}\n"));
    }
    source.push_str("fn main():()\n");
    let main = project.write("main.fn", &source);
    let error = modules::load_editor_sources(&main, &std::collections::HashMap::new()).unwrap_err();
    assert!(
        error.message.contains("editor symbol byte limit"),
        "{error:?}"
    );
    assert!(modules::load(&main).is_ok());
}

#[test]
fn documentation_qualification_uses_declaration_role_even_for_entry_main_identity() {
    let project = Project::new();
    let main=project.write("entry.fn","module entry\n@doc \"\"\"Type.\"\"\"\ntype main:\n    Wrapped(Int)\n@doc \"\"\"Function.\"\"\"\nfn main():()\n");
    let loaded = modules::load(&main).unwrap();
    assert_eq!(loaded.program.docs[0].target, "entry.main");
    assert_eq!(loaded.program.docs[1].target, "main");
}
