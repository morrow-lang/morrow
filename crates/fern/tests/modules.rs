use fern_compiler::modules;
use std::fmt::Write as _;
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
fn modules_resolve_aliases_and_keep_private_helpers_local() {
    let project = Project::new();
    project.write("math.fn", "module math\nfn helper(x: Int) -> Int: x + 1\npub fn increment(x: Int) -> Int: helper(x)\n");
    let main = project.write(
        "main.fn",
        "module main\nimport math as m\nfn main(): println(m.increment(4))\n",
    );
    let loaded = modules::load(&main).unwrap();
    let checked = fern_compiler::check::check(&loaded.program).unwrap();
    fern_compiler::lowering::emit(&checked).unwrap();
    let main = project.write(
        "main.fn",
        "module main\nimport math\nfn main(): println(math.helper(4))\n",
    );
    let error = modules::load(&main).unwrap_err();
    assert!(
        error.message.contains("private") || error.message.contains("export"),
        "{error:?}"
    );
}

#[test]
fn imported_diagnostics_retain_their_source_path() {
    let project = Project::new();
    let library = project.write(
        "broken.fn",
        "module broken\npub fn value() -> Int: missing\n",
    );
    let main = project.write(
        "main.fn",
        "import broken\nfn main(): println(broken.value())\n",
    );
    let loaded = modules::load(&main).unwrap();
    let error = fern_compiler::check::check(&loaded.program).unwrap_err();
    let rendered = loaded.render(error);
    assert!(
        rendered.contains(&format!("{}:2:", library.display())),
        "{rendered}"
    );
}

#[test]
fn import_cycles_and_missing_files_are_diagnosed() {
    let project = Project::new();
    let main = project.write("main.fn", "import cycle\nfn main(): 0\n");
    project.write(
        "cycle.fn",
        "module cycle\nimport main\npub fn x() -> Int: 0\n",
    );
    assert!(modules::load(&main).unwrap_err().message.contains("cycle"));
    project.write(
        "cycle.fn",
        "module cycle\nimport missing\npub fn x() -> Int: 0\n",
    );
    assert!(
        modules::load(&main)
            .unwrap_err()
            .message
            .contains("missing")
    );
}

#[test]
fn selective_imports_reexports_and_record_names_are_preserved() {
    let project = Project::new();
    project.write("model.fn", "module model\npub type Box(a):\n    value: a\npub fn wrap(value: a) -> Box(a): Box(value)\n");
    project.write("api/mod.fn", "module api\npub import model.{Box, wrap}\n");
    let main = project.write("main.fn", "import api.{Box, wrap}\nfn value(box: Box(Int)) -> Int: box.value\nfn main(): println(value(wrap(7)))\n");
    let loaded = modules::load(&main).unwrap();
    fern_compiler::lowering::emit(&fern_compiler::check::check(&loaded.program).unwrap()).unwrap();
}

#[test]
fn local_bindings_shadow_imported_functions() {
    let project = Project::new();
    project.write("math.fn", "pub fn value() -> Int: 1\n");
    let main = project.write(
        "main.fn",
        "import math.{value}\nfn main():\n    let value = 2\n    println(value)\n",
    );
    let loaded = modules::load(&main).unwrap();
    fern_compiler::check::check(&loaded.program).unwrap();
}

#[test]
fn private_nominal_annotations_and_constructor_patterns_cannot_bypass_exports() {
    let project = Project::new();
    project.write(
        "library.fn",
        "module library\ntype Hidden:\n    Secret(Int)\npub fn hidden() -> Hidden: Secret(1)\n",
    );
    let main = project.write(
        "main.fn",
        "import library\nfn leak(value: library.Hidden) -> Int: 0\nfn main(): 0\n",
    );
    let error = modules::load(&main).unwrap_err();
    assert!(
        error.message.contains("private") || error.message.contains("export"),
        "{error:?}"
    );
    project.write("main.fn", "import library\nfn main():\n    match library.hidden():\n        library.Secret(n) -> println(n)\n");
    let error = modules::load(&main).unwrap_err();
    assert!(
        error.message.contains("private") || error.message.contains("export"),
        "{error:?}"
    );
}

#[test]
fn transitive_and_unaliased_qualified_names_cannot_reach_private_functions() {
    let project = Project::new();
    project.write(
        "library.fn",
        "module library\nfn secret() -> Int: 7\npub fn value() -> Int: secret()\n",
    );
    project.write(
        "api.fn",
        "module api\nimport library\npub fn value() -> Int: library.value()\n",
    );
    let main = project.write(
        "main.fn",
        "import api\nfn main(): println(library.secret())\n",
    );
    assert!(modules::load(&main).is_err());
    project.write(
        "main.fn",
        "import library as l\nfn main(): println(library.secret())\n",
    );
    assert!(modules::load(&main).is_err());
}

#[test]
fn builtin_constructor_names_cannot_be_hidden_by_module_qualification() {
    let project = Project::new();
    for builtin in ["Some", "None", "Ok", "Err", "Int", "String"] {
        let main = project.write(
            "main.fn",
            &format!("type Fake:\n    {builtin}(Int)\nfn main(): 0\n"),
        );
        let error = modules::load(&main).unwrap_err();
        assert!(error.message.contains("reserved"), "{builtin}: {error:?}");
    }
}

#[test]
fn graph_limit_counts_unique_files_not_repeat_dependency_visits() {
    let project = Project::new();
    for index in 0..127 {
        project.write(
            &format!("m{index}.fn"),
            &format!("module m{index}\npub fn value() -> Int: {index}\n"),
        );
    }
    let mut source = String::new();
    for index in 0..127 {
        writeln!(source, "import m{index}").unwrap();
    }
    source.push_str("import m0 as shared\nfn main(): println(shared.value())\n");
    let main = project.write("main.fn", &source);
    let loaded = modules::load(&main).unwrap();
    assert_eq!(loaded.program.functions.len(), 128);
    fern_compiler::check::check(&loaded.program).unwrap();
    project.write("extra.fn", "pub fn value() -> Int: 0\n");
    project.write("main.fn", &format!("import extra\n{source}"));
    assert!(
        modules::load(&main)
            .unwrap_err()
            .message
            .contains("128-file")
    );
}

#[test]
fn all_module_failures_carry_the_cli_error_marker() {
    let project = Project::new();
    let main = project.write("main.fn", "import missing\nfn main(): 0\n");
    assert!(modules::load(&main).unwrap_err().message.contains("error:"));
    project.write(
        "main.fn",
        "fn duplicate() -> Int: 0\nfn main(): 0\nfn duplicate() -> Int: 1\n",
    );
    assert!(modules::load(&main).unwrap_err().message.contains("error:"));
    assert!(
        modules::load(&project.0.join("absent.fn"))
            .unwrap_err()
            .message
            .contains("error:")
    );
}

#[test]
fn self_qualified_helpers_and_library_main_keep_distinct_entry_identities() {
    let project = Project::new();
    project.write(
        "library.fn",
        "module library\npub fn main() -> Int: library.helper()\nfn helper() -> Int: 4\n",
    );
    let main = project.write(
        "entry.fn",
        "module entry\nimport library\nfn main(): println(library.main())\n",
    );
    let loaded = modules::load(&main).unwrap();
    let checked = fern_compiler::check::check(&loaded.program).unwrap();
    assert_eq!(
        checked
            .functions
            .iter()
            .filter(|f| f.name == "main")
            .count(),
        1
    );
    assert!(checked.functions.iter().any(|f| f.name == "library.main"));
}

#[test]
fn diamond_dependencies_are_loaded_once_and_deep_rewrites_fail_cleanly() {
    let project = Project::new();
    project.write("common.fn", "pub fn value() -> Int: 1\n");
    for name in ["left", "right"] {
        project.write(
            &format!("{name}.fn"),
            "import common\npub fn value() -> Int: common.value()\n",
        );
    }
    let main = project.write(
        "main.fn",
        "import left\nimport right\nfn main(): println(left.value() + right.value())\n",
    );
    let loaded = modules::load(&main).unwrap();
    assert_eq!(
        loaded
            .program
            .functions
            .iter()
            .filter(|f| f.name == "common.value")
            .count(),
        1
    );
    fern_compiler::check::check(&loaded.program).unwrap();
    let nested = format!("{}0{}", "Some(".repeat(300), ")".repeat(300));
    project.write("left.fn", &format!("pub fn value() -> Int: {nested}\n"));
    let error = modules::load(&main).unwrap_err();
    assert!(
        error.message.contains("limit") || error.message.contains("nesting"),
        "{error:?}"
    );
}

#[test]
fn dependencies_cannot_call_the_entry_main_without_an_import() {
    let project = Project::new();
    project.write("library.fn", "pub fn enter() -> Unit: main()\n");
    let main = project.write("main.fn", "import library\nfn main(): library.enter()\n");
    let error = modules::load(&main).unwrap_err();
    assert!(
        error.message.contains("main") && error.message.contains("import"),
        "{error:?}"
    );
}

#[test]
fn source_overlays_replace_disk_and_admit_new_files_without_writing() {
    use std::collections::HashMap;
    let project = Project::new();
    let main = project.write("main.fn", "import math\nfn main(): println(math.value())\n");
    let math = project.write("math.fn", "module math\npub fn value() -> Int: missing\n");
    let snapshots = HashMap::from([(
        math.clone(),
        "module math\npub fn value() -> Int: 7\n".into(),
    )]);
    let loaded = modules::load_with_sources(&main, &snapshots).unwrap();
    fern_compiler::check::check(&loaded.program).unwrap();
    assert!(fs::read_to_string(&math).unwrap().contains("missing"));
    let new = project.0.join("new.fn");
    let snapshots = HashMap::from([(new.clone(), "fn main(): ()\n".into())]);
    modules::load_with_sources(&new, &snapshots).unwrap();
    assert!(!new.exists());
}

#[test]
fn source_overlay_errors_retain_structured_original_locations() {
    use std::collections::HashMap;
    let project = Project::new();
    let main = project.write("main.fn", "import math\nfn main(): println(math.value())\n");
    let math = project.0.join("math.fn");
    let source = "module math\npub fn value() -> Int: missing\n";
    let snapshots = HashMap::from([(math.clone(), source.into())]);
    let loaded = modules::load_with_sources(&main, &snapshots).unwrap();
    let diagnostic = fern_compiler::check::check(&loaded.program).unwrap_err();
    let located = loaded.locate(diagnostic).unwrap();
    assert_eq!(located.path, modules::source_identity(&math).unwrap());
    assert_eq!(
        &located.source[located.diagnostic.span.start..located.diagnostic.span.end],
        "missing"
    );
    let snapshots = HashMap::from([(math.clone(), "module math\npub fn value(:\n".into())]);
    let error = modules::load_with_sources(&main, &snapshots).unwrap_err();
    assert_eq!(
        error.location.unwrap().path,
        modules::source_identity(&math).unwrap()
    );
}

#[test]
fn source_overlays_preserve_cycles_containment_and_size_limits() {
    use std::collections::HashMap;
    let project = Project::new();
    let main = project.write("main.fn", "import math\nfn main(): ()\n");
    let math = project.0.join("math.fn");
    let snapshots = HashMap::from([(math.clone(), "module math\nimport main\n".into())]);
    assert!(
        modules::load_with_sources(&main, &snapshots)
            .unwrap_err()
            .message
            .contains("cycle")
    );
    let snapshots = HashMap::from([(math, " ".repeat(1024 * 1024 + 1))]);
    assert!(
        modules::load_with_sources(&main, &snapshots)
            .unwrap_err()
            .message
            .contains("1 MiB")
    );
}

#[cfg(unix)]
#[test]
fn source_overlays_cannot_bypass_symlink_containment() {
    use std::collections::HashMap;
    let project = Project::new();
    let outside = Project::new();
    let main = project.write("main.fn", "import math\nfn main(): ()\n");
    let target = outside.write("math.fn", "module math\n");
    let alias = project.0.join("math.fn");
    std::os::unix::fs::symlink(target, &alias).unwrap();
    let snapshots = HashMap::from([(alias, "module math\n".into())]);
    assert!(
        modules::load_with_sources(&main, &snapshots)
            .unwrap_err()
            .message
            .contains("outside project root")
    );
}

#[test]
fn imported_module_name_mismatch_points_to_the_offending_source() {
    let project = Project::new();
    let main = project.write("main.fn", "import math\nfn main(): ()\n");
    let math = project.write("math.fn", "module different\n");
    let error = modules::load(&main).unwrap_err();
    assert_eq!(error.location.unwrap().path, math.canonicalize().unwrap());
}
