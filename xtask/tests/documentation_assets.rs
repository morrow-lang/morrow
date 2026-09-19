use std::{env, fs, path::Path};

#[test]
fn repository_site_includes_language_chapters_and_local_images() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let temporary = xtask::Temporary::new(&env::temp_dir()).unwrap();
    let root = &temporary.0;
    fs::create_dir_all(root.join("examples")).unwrap();
    fs::write(root.join("examples/main.mr"), "fn main():\n    ()\n").unwrap();
    fs::create_dir_all(root.join("docs/language")).unwrap();
    fs::create_dir_all(root.join("docs/assets")).unwrap();
    fs::write(
        root.join("README.md"),
        "# Morrow\n\n![Morrow logo](docs/assets/mark.png)\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/language/getting-started.md"),
        "# Getting started\n\nA complete first program.\n",
    )
    .unwrap();
    for name in ["DESIGN.md", "ROADMAP.md", "BUILD.md", "MORROW_STYLE.md"] {
        fs::write(root.join(name), format!("# {name}\n")).unwrap();
    }
    // Decision records are a vrdx collection rather than a single guide file.
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join("decisions/2026-01-01_000000000_a-recorded-decision.md"),
        "# A recorded decision\n\nThe reasoning is preserved.\n",
    )
    .unwrap();
    let image = b"\x89PNG\r\n\x1a\nfixture bytes copied without interpretation";
    fs::write(root.join("docs/assets/mark.png"), image).unwrap();
    let output = root.join("site");
    // Rebuilding must preserve image delivery as well as the first publication.
    for _ in 0..2 {
        xtask::docs::run(root, &project.join("bin"), &output, false).unwrap();
        assert!(
            fs::read_to_string(output.join("index.html"))
                .unwrap()
                .contains("<img src=\"docs/assets/mark.png\" alt=\"Morrow logo\">")
        );
        assert!(
            fs::read_to_string(output.join("getting-started.html"))
                .unwrap()
                .contains("A complete first program.")
        );
        assert_eq!(
            fs::read(output.join("docs/assets/mark.png")).unwrap(),
            image
        );
    }
}
