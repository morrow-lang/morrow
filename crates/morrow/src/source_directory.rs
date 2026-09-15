//! Shared bounded discovery for explicit source-directory commands.
// Decision107 pilot: strict boundary lints are enforceable here without hiding validated IR rules.
#![deny(clippy::pedantic, clippy::nursery)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![deny(clippy::as_conversions, clippy::unreachable, clippy::string_slice)]
#![deny(clippy::arithmetic_side_effects)]
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Traverse without following child links; bound depth, entries, path bytes and source count.
// Previous count is at most 8192, so the next is at most 8193; validated depth + 1 is at most 33.
#[allow(clippy::arithmetic_side_effects)]
// Keep the caller boundary explicit even if this private module is reorganized later.
#[allow(clippy::redundant_pub_crate)]
pub(super) fn discover(root: &Path, purpose: &str) -> Result<Vec<PathBuf>, String> {
    if root.as_os_str().len() > 4096 {
        return Err(format!("{purpose} path exceeds 4096 bytes"));
    }
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut files = Vec::new();
    let mut count = 0;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 32 {
            return Err(format!("{purpose} directory nesting exceeds 32"));
        }
        for entry in
            fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?
        {
            count += 1;
            if count > 8192 {
                return Err(format!("{purpose} directory entry limit exceeded"));
            }
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if path.as_os_str().len() > 4096 {
                return Err(format!("{purpose} path exceeds 4096 bytes"));
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    name.as_ref(),
                    "target" | "build" | "bin" | "deps" | "node_modules"
                ) {
                    pending.push((path, depth + 1));
                }
            } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "mr")
            {
                if files.len() == 256 {
                    return Err(format!("{purpose} file limit exceeds 256"));
                }
                files.push(path);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(format!(
            "{purpose} directory contains no Morrow source files"
        ));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> std::io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "morrow-source-discovery-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    #[test]
    fn discovers_mr_sources_while_ignoring_legacy_fn_sources()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = TestDirectory::new()?;
        fs::create_dir(root.0.join("nested"))?;
        fs::write(root.0.join("z.mr"), "")?;
        fs::write(root.0.join("nested/a.mr"), "")?;
        fs::write(root.0.join("legacy.fn"), "")?;

        let files = discover(&root.0, "format").map_err(std::io::Error::other)?;

        assert_eq!(files, vec![root.0.join("nested/a.mr"), root.0.join("z.mr")]);
        Ok(())
    }

    #[test]
    fn oversized_initial_path_is_rejected_before_copy_or_filesystem_lookup() {
        let text = "x".repeat(4097);
        assert_eq!(
            discover(Path::new(&text), "format"),
            Err("format path exceeds 4096 bytes".into())
        );
    }
}
