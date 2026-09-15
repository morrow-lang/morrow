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
            "morrow-module-text-{}-{}",
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
fn imported_unicode_docs_and_multiline_interpolation_keep_names_and_locations() {
    let project = Project::new();
    let library = project.write("café.mr", "module café\n@doc \"\"\"Literal {example}.\"\"\"\npub fn valeur() -> Int: 0x2a\n@doc \"\"\"A Unicode record.\"\"\"\npub type Boîte:\n    valeur: Int\n");
    let main = project.write(
        "main.mr",
        "import café\nfn main(): println(\"\"\"answer\n{café.valeur()}\"\"\")\n",
    );
    let loaded = modules::load(&main).unwrap();
    assert_eq!(loaded.program.docs.len(), 2);
    assert_eq!(loaded.program.docs[0].target, "café.valeur");
    assert_eq!(loaded.program.docs[1].target, "café.Boîte");
    let doc = &loaded.program.docs[0];
    assert_eq!(doc.text, "Literal {example}.");
    let located = loaded
        .locate(morrow_compiler::Diagnostic::new(doc.span, "metadata"))
        .unwrap();
    assert_eq!(located.path, library.canonicalize().unwrap());
    assert!(
        located.source[located.diagnostic.span.start..located.diagnostic.span.end]
            .starts_with("@doc")
    );
    lowering::emit(&check::check(&loaded.program).unwrap()).unwrap();
}

#[test]
fn multiline_interpolation_cannot_expose_private_imports() {
    let project = Project::new();
    project.write(
        "model.mr",
        "fn secret() -> Int: 42\npub fn number() -> Int: 1\n",
    );
    let main = project.write(
        "main.mr",
        "import model\nfn main(): println(\"\"\"answer\n{model.secret()}\"\"\")\n",
    );
    assert!(
        modules::load(&main)
            .unwrap_err()
            .message
            .contains("private")
    );
}
