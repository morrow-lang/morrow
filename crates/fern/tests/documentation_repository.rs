//! Typecheck documentation literally; IO/network examples are never executed here.
use fern_compiler::{check, doctest, modules};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn snippet_path(root: &Path, source: &str) -> Result<PathBuf, String> {
    let first = source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or("");
    let module = first
        .strip_prefix("module ")
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .unwrap_or("doc_tests");
    let mut path = root.to_path_buf();
    for component in module.split('.') {
        if component.is_empty()
            || component
                .chars()
                .any(|ch| !ch.is_alphanumeric() && ch != '_')
        {
            return Err("invalid documentation module path".into());
        }
        path.push(component);
    }
    path.set_extension("fn");
    Ok(path)
}

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn verify(code: &str) -> Result<(), String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = Directory(std::env::temp_dir().join(format!(
        "fern-doc-check-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&directory.0).map_err(|error| error.to_string())?;
    let typecheck = |source: &str| {
        let path = snippet_path(&directory.0, source)?;
        fs::create_dir_all(path.parent().ok_or("missing source parent")?)
            .map_err(|error| error.to_string())?;
        fs::write(&path, source).map_err(|error| error.to_string())?;
        let loaded = modules::load(&path).map_err(|error| error.message)?;
        check::check(&loaded.program)
            .map(|_| ())
            .map_err(|error| loaded.render(error))
    };
    if typecheck(code).is_ok() {
        return Ok(());
    }
    if code
        .lines()
        .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
    {
        return Ok(());
    }
    let mut wrapper = String::from("fn main() -> Int:\n");
    for line in code.lines() {
        wrapper.push_str("    ");
        wrapper.push_str(line);
        wrapper.push('\n');
    }
    wrapper.push_str("    0\n");
    typecheck(&wrapper)
}

#[test]
fn literal_documentation_preserves_nesting_strings_and_result_obligations() {
    for source in [
        "println(\"hello\")",
        "println(\"literal # => text\")",
        "let value = if true:\n    1\nelse:\n    2\nprintln(value)",
        "match fs.read(\"missing.txt\"):\n    Ok(text) -> println(text)\n    Err(_) -> println(\"missing\")",
        "module samples.example\nfn main(): println(\"hello\")",
        "# ordinary comment\n",
    ] {
        verify(source).unwrap();
    }
    for source in [
        "fs.read(\"missing.txt\")",
        "let value: Int = \"wrong\"\nprintln(value)",
        "fn main(): unknown_name()",
    ] {
        assert!(verify(source).is_err(), "{source}");
    }
    assert!(
        verify("fs.read(\"missing.txt\")")
            .unwrap_err()
            .contains("Result")
    );
}

#[test]
fn documentation_module_names_cannot_escape_the_private_workspace() {
    for module in ["../escape", "a/b", "a\\b", ".hidden", "a..b"] {
        assert!(
            snippet_path(
                Path::new("/owned"),
                &format!("module {module}\nfn main(): ()")
            )
            .is_err()
        );
    }
    assert_eq!(
        snippet_path(Path::new("/owned"), "module samples.example\nfn main(): ()").unwrap(),
        Path::new("/owned/samples/example.fn")
    );
}

#[test]
fn parser_owned_doc_extraction_retains_nested_source() {
    let source = "@doc \"\"\"\nExample:\n```fern\n    if true:\n        println(\"ok\")\n```\n\"\"\"\nfn main(): ()\n";
    let snippets = doctest::extract(source).unwrap();
    assert_eq!(snippets.len(), 1);
    assert_eq!(snippets[0].code, "if true:\n    println(\"ok\")\n");
    verify(&snippets[0].code).unwrap();
}

#[test]
fn all_public_example_and_stdlib_documentation_snippets_typecheck() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut count = 0;
    for directory in ["examples", "docs/doctests"] {
        for entry in fs::read_dir(root.join(directory)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "fn") {
                continue;
            }
            let source = fs::read_to_string(&path).unwrap();
            for snippet in doctest::extract(&source).unwrap() {
                // Standalone snippets typecheck alone; module examples typecheck as the same
                // overlay `fern test --doc` executes, so they may call their own declarations.
                verify(&snippet.code)
                    .or_else(|_| {
                        let prepared =
                            doctest::prepare(&source, &snippet).map_err(|error| error.message)?;
                        let parsed = fern_compiler::parse::parse(&prepared.source)
                            .map_err(|error| error.message)?;
                        check::check_library(&parsed)
                            .map(|_| ())
                            .map_err(|error| error.message)
                    })
                    .unwrap_or_else(|error| {
                        panic!("{} example {}: {error}", path.display(), snippet.ordinal)
                    });
                count += 1;
            }
        }
    }
    assert!(count > 0, "documentation coverage disappeared");
}
