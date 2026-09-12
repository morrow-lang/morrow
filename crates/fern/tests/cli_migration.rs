//! Public command compatibility required before the Rust default switch.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern-cli-migration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_fern"))
            .args(args)
            .current_dir(&self.0)
            .env("FERN_QBE", self.0.join("missing-backend"))
            .output()
            .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn literal_source_delimiter_preserves_dash_prefixed_paths() {
    let dir = Directory::new();
    fs::write(dir.0.join("--quiet"), "fn main(): ()\n").unwrap();
    for command in ["check", "emit", "lex", "parse", "fmt", "doc"] {
        let result = dir.run(&[command, "--", "--quiet"]);
        assert!(result.status.success(), "{command}: {result:?}");
        assert!(result.stderr.is_empty(), "{command}: {result:?}");
    }
    for command in [
        "check", "emit", "lex", "parse", "fmt", "doc", "test", "build", "run",
    ] {
        let result = dir.run(&[command, "--", "--quiet", "extra.fn"]);
        assert!(!result.status.success(), "{command}: {result:?}");
    }
    fs::write(dir.0.join("--bad"), "fn main(): missing\n").unwrap();
    for command in ["build", "run"] {
        let result = dir.run(&[command, "--", "--bad"]);
        assert!(!result.status.success());
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("--bad:1:"), "{diagnostic}");
        assert!(diagnostic.contains("missing"), "{diagnostic}");
    }
    // No native tests means no compiler/runtime dependency is needed.
    let result = dir.run(&["test", "--doc", "--", "--quiet"]);
    assert!(result.status.success(), "{result:?}");
}

#[test]
fn documentation_defaults_to_current_project_directory() {
    let dir = Directory::new();
    fs::write(
        dir.0.join("library.fn"),
        "@doc \"\"\"Documented helper.\"\"\"\npub fn helper() -> Int: 42\n",
    )
    .unwrap();
    let default = dir.run(&["doc"]);
    let explicit = dir.run(&["doc", "."]);
    assert!(default.status.success(), "{default:?}");
    assert_eq!(default.stdout, explicit.stdout);
    assert!(
        String::from_utf8(default.stdout)
            .unwrap()
            .contains("Documented helper.")
    );
}

#[test]
fn version_uses_the_compatible_public_language_identity() {
    let directory = Directory::new();
    for flag in ["--version", "-v"] {
        let output = directory.run(&[flag]);
        assert!(output.status.success());
        assert_eq!(
            output.stdout,
            format!("fern {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
        );
        assert!(output.stderr.is_empty());
    }
}
