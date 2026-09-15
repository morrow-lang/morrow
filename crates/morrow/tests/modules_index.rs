use morrow_compiler::{Span, modules, parse};
use std::{collections::HashMap, fs, path::PathBuf};
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-source-index-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, text).unwrap();
        p
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn editor_metadata_retains_aliases_without_changing_ordinary_compiler_syntax() {
    let project = Project::new();
    let model = project.write(
        "model.mr",
        "pub type Item:\n    value: Int\npub fn value()->Int:1\nfn hidden()->Int:2\n",
    );
    let main = project.write(
        "main.mr",
        "import model as m\nfn main():println(m.value())\n",
    );
    let plain = modules::load(&main).unwrap();
    let indexed = modules::load_editor_sources(&main, &HashMap::new()).unwrap();
    assert!(plain.symbols.is_empty());
    assert_eq!(
        format!("{:?}", plain.program),
        format!("{:?}", indexed.program)
    );
    let main = main.canonicalize().unwrap();
    let visible = &indexed
        .symbols
        .iter()
        .find(|s| s.path == main)
        .unwrap()
        .values;
    assert_eq!(visible.get("m.value").unwrap(), "model.value");
    assert!(!visible.contains_key("m.hidden"));
    let function = indexed
        .program
        .functions
        .iter()
        .find(|f| f.name == "model.value")
        .unwrap();
    let (path, span) = indexed.locate_span(function.span).unwrap();
    assert_eq!(path, model.canonicalize().unwrap());
    let source = indexed.sources().find(|s| s.path == path).unwrap();
    assert!(source.text[span.start..span.end].starts_with("fn value"));
    assert!(indexed.locate_span(Span { start: 100, end: 0 }).is_none());
}
#[test]
fn identifier_ranges_are_exact_unicode_and_exclude_noncode_contents() {
    let source = "fn 🌿(x: Int): println(\"text {x}\") # hidden\n";
    let index = parse::identifier_index(source).unwrap();
    let names = index
        .identifiers
        .iter()
        .map(|s| &source[s.start..s.end])
        .collect::<Vec<_>>();
    assert!(names.contains(&"🌿"));
    assert!(names.contains(&"x"));
    assert!(!names.contains(&"text"));
    assert!(!names.contains(&"hidden"));
    for end in 0..=source.len() {
        if source.is_char_boundary(end) {
            let _ = parse::identifier_index(&source[..end]);
        }
    }
    assert!(parse::identifier_index(&" ".repeat(1024 * 1024 + 1)).is_err());
}

#[test]
fn editor_alias_retention_has_an_aggregate_byte_limit_before_cloning() {
    let project = Project::new();
    let name = "x".repeat(65_536);
    project.write("base.mr", &format!("pub fn {name}() -> Int: 1\n"));
    let mut source = String::new();
    for index in 0..64 {
        source.push_str(&format!("import base as a{index}\n"));
    }
    source.push_str("fn main(): ()\n");
    let main = project.write("main.mr", &source);
    let error = modules::load_editor_sources(&main, &HashMap::new()).unwrap_err();
    assert!(
        error.message.contains("editor symbol byte limit"),
        "{}",
        error.message
    );
    assert!(modules::load(&main).is_ok());
}
