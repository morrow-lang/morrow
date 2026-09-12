//! Repository automation with bounded literal process and filesystem operations.
pub mod acceptance;
pub mod build;
pub mod compatibility;
pub mod distribution;
pub mod fuzz;
pub mod lint_policy;
pub mod performance;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// A newly created private directory containing only this task's artifacts.
pub struct Temporary(pub PathBuf);
impl Temporary {
    pub fn new(parent: &Path) -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let parent = parent.canonicalize().map_err(|error| error.to_string())?;
        for _ in 0..100 {
            let path = parent.join(format!(
                ".fern-task-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("cannot allocate private task directory".into())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run a trusted developer tool with inherited output and explicit failure propagation.
pub fn execute(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("{command:?}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed: {status}"))
    }
}
