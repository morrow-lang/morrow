//! Build the repository's own documentation site with the staged compiler's `morrow doc --site`.
//!
//! The same generator that documents Morrow libraries renders the project's guides, decision
//! record, design and examples; `cargo doc` output for the Rust workspace is embedded beside it.
use crate::execute;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Guides rendered in sidebar order; the root README becomes the landing page.
pub const EXTRAS: &[&str] = &[
    "README.md",
    "docs",
    "docs/language",
    "DESIGN.md",
    "ROADMAP.md",
    "DECISIONS.md",
    "BUILD.md",
    "MORROW_STYLE.md",
];
const REPOSITORY: &str = "https://github.com/morrow-lang/morrow";
const MAX_COPIED_FILES: usize = 50_000;
const MAX_COPIED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_COPY_DEPTH: usize = 32;

/// Generate the site into `output`, then optionally add rustdoc under `output/rust`.
pub fn run(root: &Path, bin: &Path, output: &Path, rust: bool) -> Result<(), String> {
    let mut command = Command::new(bin.join("morrow"));
    command
        .current_dir(root)
        .args(["doc", "examples", "--inferred", "--site"])
        .arg(output)
        .args(["--title", "Morrow", "--version", env!("CARGO_PKG_VERSION")]);
    for extra in EXTRAS {
        command.arg("--extras").arg(extra);
    }
    command.arg("--link").arg(format!("GitHub={REPOSITORY}"));
    if rust {
        command
            .arg("--link")
            .arg("Rust API (rustdoc)=rust/morrow_compiler/index.html");
    }
    execute(&mut command)?;
    copy_tree(
        &root.join("docs/assets"),
        &root.join(output).join("docs/assets"),
    )?;
    if rust {
        let mut cargo = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        cargo
            .current_dir(root)
            .args(["doc", "--workspace", "--no-deps", "--locked"]);
        execute(&mut cargo)?;
        let target = env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        let generated = target.join("doc");
        let destination = root.join(output).join("rust");
        copy_tree(&generated, &destination)?;
    }
    println!(
        "Documentation site: {}",
        root.join(output).join("index.html").display()
    );
    Ok(())
}

/// Copy regular files and directories without following links, within explicit bounds.
pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let mut budget = (0usize, 0u64);
    copy_directory(source, destination, 0, &mut budget)
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    depth: usize,
    budget: &mut (usize, u64),
) -> Result<(), String> {
    if depth > MAX_COPY_DEPTH {
        return Err(format!(
            "{}: copy depth exceeds {MAX_COPY_DEPTH}",
            source.display()
        ));
    }
    fs::create_dir_all(destination)
        .map_err(|error| format!("{}: {error}", destination.display()))?;
    let mut entries: Vec<_> = fs::read_dir(source)
        .map_err(|error| format!("{}: {error}", source.display()))?
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            copy_directory(&from, &to, depth + 1, budget)?;
        } else if kind.is_file() {
            budget.0 += 1;
            if budget.0 > MAX_COPIED_FILES {
                return Err(format!("rustdoc output exceeds {MAX_COPIED_FILES} files"));
            }
            let copied =
                fs::copy(&from, &to).map_err(|error| format!("{}: {error}", from.display()))?;
            budget.1 = budget.1.saturating_add(copied);
            if budget.1 > MAX_COPIED_BYTES {
                return Err("rustdoc output exceeds the copy byte budget".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_tree_preserves_files_and_directories_but_skips_links() {
        let stage = crate::Temporary::new(&env::temp_dir()).unwrap();
        let source = stage.0.join("source");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("index.html"), "root").unwrap();
        fs::write(source.join("nested/page.html"), "nested").unwrap();
        std::os::unix::fs::symlink(source.join("index.html"), source.join("alias.html")).unwrap();
        let destination = stage.0.join("destination");
        copy_tree(&source, &destination).unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("index.html")).unwrap(),
            "root"
        );
        assert_eq!(
            fs::read_to_string(destination.join("nested/page.html")).unwrap(),
            "nested"
        );
        assert!(!destination.join("alias.html").exists());
    }
}
