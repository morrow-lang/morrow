//! `morrow doc --site` publishes a complete multi-page site atomically and protects its inputs.
#![cfg(unix)]
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow-doc-site-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("lib")).unwrap();
        fs::create_dir_all(path.join("docs")).unwrap();
        fs::write(
            path.join("lib/math.mr"),
            "module lib.math\n\n@moduledoc \"\"\"Arithmetic helpers.\"\"\"\n\n@doc \"\"\"Add two numbers.\"\"\"\npub fn add(a: Int, b: Int) -> Int: a + b\n\nfn twice(value) -> Int: value + value\n",
        )
        .unwrap();
        fs::write(
            path.join("lib/text.mr"),
            "@doc \"\"\"Shout.\"\"\"\npub fn shout(text: String) -> String: text\n",
        )
        .unwrap();
        fs::write(
            path.join("README.md"),
            "# Sample\n\nRead the [guide](docs/GUIDE.md) and call `lib.math.add`.\n",
        )
        .unwrap();
        fs::write(
            path.join("docs/GUIDE.md"),
            "# Getting Started\n\n## Install\n\nBack to the [readme](../README.md).\n",
        )
        .unwrap();
        fs::write(path.join("docs/notes.txt"), "ignored").unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_morrow"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.0.join(relative)).unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn site_writes_pages_guides_assets_and_replaces_previous_output_atomically() {
    let project = Project::new();
    let result = project.run(&[
        "doc",
        "lib",
        "--site",
        "site",
        "--title",
        "Sample",
        "--version",
        "0.9.0",
        "--extras",
        "README.md",
        "--extras",
        "docs",
        "--link",
        "GitHub=https://example.com/sample",
    ]);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    let index = project.read("site/index.html");
    assert!(index.contains("<a href=\"guide.html\">guide</a>"));
    assert!(index.contains("<a href=\"lib.math.html#add\"><code>lib.math.add</code></a>"));
    assert!(index.contains("v0.9.0"));
    assert!(index.contains("https://example.com/sample"));
    let math = project.read("site/lib.math.html");
    assert!(math.contains("Arithmetic helpers."));
    assert!(math.contains("id=\"add\""));
    assert!(project.0.join("site/text.html").is_file());
    assert!(
        project
            .read("site/guide.html")
            .contains("<a href=\"index.html\">readme</a>")
    );
    assert!(project.0.join("site/morrow-docs.css").is_file());
    assert!(project.0.join("site/morrow-docs.js").is_file());
    assert!(project.0.join("site/morrow-search.js").is_file());
    assert!(!project.0.join("site/notes.html").exists());
    // A second build replaces the directory completely, removing stale pages.
    fs::write(project.0.join("site/stale.html"), "old").unwrap();
    fs::remove_file(project.0.join("lib/text.mr")).unwrap();
    let result = project.run(&["doc", "lib", "--site", "site", "--title", "Sample"]);
    assert!(result.status.success(), "{result:?}");
    assert!(!project.0.join("site/stale.html").exists());
    assert!(!project.0.join("site/text.html").exists());
    assert!(project.0.join("site/lib.math.html").is_file());
    // No staging directories remain beside the published site.
    let leftovers: Vec<_> = fs::read_dir(&project.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".morrow"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn site_creates_missing_parent_directories() {
    let project = Project::new();
    let result = project.run(&["doc", "lib", "--site", "dist/docs"]);
    assert!(result.status.success(), "{result:?}");
    assert!(project.0.join("dist/docs/index.html").is_file());
    assert!(project.0.join("dist/docs/morrow-docs.css").is_file());
}

#[test]
fn site_directory_extras_omit_readme_files_so_collections_can_share_the_name() {
    let project = Project::new();
    fs::write(
        project.0.join("docs/README.md"),
        "# Catalog\n\nGuides live here.\n",
    )
    .unwrap();
    fs::create_dir(project.0.join("more")).unwrap();
    fs::write(project.0.join("more/README.md"), "# Collection notes\n").unwrap();
    fs::write(
        project.0.join("more/extra.md"),
        "# Extra\n\nAnother guide.\n",
    )
    .unwrap();
    let result = project.run(&[
        "doc",
        "lib",
        "--site",
        "site",
        "--extras",
        "README.md",
        "--extras",
        "docs",
        "--extras",
        "more",
    ]);
    assert!(result.status.success(), "{result:?}");
    assert!(project.0.join("site/index.html").is_file());
    assert!(project.0.join("site/guide.html").is_file());
    assert!(project.0.join("site/extra.html").is_file());
    assert!(!project.0.join("site/readme.html").exists());
    let explicit = project.run(&[
        "doc",
        "lib",
        "--site",
        "site",
        "--extras",
        "README.md",
        "--extras",
        "docs/README.md",
        "--extras",
        "docs",
        "--extras",
        "more",
    ]);
    assert!(explicit.status.success(), "{explicit:?}");
    assert!(
        project
            .read("site/readme.html")
            .contains("Guides live here."),
        "{}",
        project.read("site/readme.html")
    );
}

#[test]
fn site_fails_before_publishing_when_any_input_is_invalid() {
    let project = Project::new();
    assert!(
        project
            .run(&["doc", "lib", "--site", "site"])
            .status
            .success()
    );
    fs::write(project.0.join("lib/broken.mr"), "fn bad():\n    (\n").unwrap();
    let result = project.run(&["doc", "lib", "--site", "site"]);
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("broken.mr:"), "{stderr}");
    assert!(project.0.join("site/lib.math.html").is_file());
    assert!(!project.0.join("site/broken.html").exists());
}

#[test]
fn site_refuses_directories_it_does_not_own() {
    let project = Project::new();
    // The source directory, its ancestors and arbitrary existing directories are protected.
    for target in [".", "lib", "docs"] {
        let result = project.run(&["doc", "lib", "--site", target]);
        assert_eq!(result.status.code(), Some(1), "{target}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("refusing"), "{target}: {stderr}");
    }
    assert!(project.0.join("lib/math.mr").is_file());
    assert!(project.0.join("docs/GUIDE.md").is_file());
    fs::write(project.0.join("file"), "x").unwrap();
    let result = project.run(&["doc", "lib", "--site", "file"]);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(project.read("file"), "x");
    fs::create_dir(project.0.join("other")).unwrap();
    fs::write(project.0.join("other/keep.txt"), "keep").unwrap();
    let result = project.run(&["doc", "lib", "--site", "other"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("refusing"));
    assert_eq!(project.read("other/keep.txt"), "keep");
    std::os::unix::fs::symlink(project.0.join("other"), project.0.join("alias")).unwrap();
    let result = project.run(&["doc", "lib", "--site", "alias"]);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(project.read("other/keep.txt"), "keep");
}

#[test]
fn site_options_are_validated_and_guides_alone_are_allowed() {
    let project = Project::new();
    let cases: [(&[&str], &str); 5] = [
        (&["doc", "lib", "--site", "site", "-o", "x.html"], "-o"),
        (&["doc", "lib", "--site", "site", "--html"], "--html"),
        (&["doc", "lib", "--extras", "docs"], "--site"),
        (
            &["doc", "lib", "--site", "site", "--link", "nolabel"],
            "label=url",
        ),
        (
            &[
                "doc",
                "lib",
                "--site",
                "site",
                "--link",
                "x=javascript:alert(1)",
            ],
            "safe",
        ),
    ];
    for (args, expected) in cases {
        let result = project.run(args);
        assert_eq!(result.status.code(), Some(1), "{args:?}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains(expected), "{args:?}: {stderr}");
        assert!(!project.0.join("site").exists());
    }
    let result = project.run(&["doc", "--site", "site", "--extras", "docs"]);
    assert!(result.status.success(), "{result:?}");
    assert!(project.0.join("site/guide.html").is_file());
    assert!(project.0.join("site/index.html").is_file());
    assert!(!project.0.join("site/lib.math.html").exists());
}

#[test]
fn site_inferred_adds_checked_signatures() {
    let project = Project::new();
    let result = project.run(&["doc", "lib", "--site", "site", "--inferred"]);
    assert!(result.status.success(), "{result:?}");
    let math = project.read("site/lib.math.html");
    assert!(math.contains("Checked signature"), "{math}");
    assert!(
        math.contains("<span class=\"fn\">twice</span>(value: <span class=\"ty\">Int</span>)"),
        "{math}"
    );
}
