use crate::Result;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub struct PrivateDir {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl PrivateDir {
    pub fn new() -> Result<Self> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        for _ in 0..32 {
            let path = std::env::temp_dir().join(format!(
                "morrow-message-path-{}-{stamp}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => {
                    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                    return Ok(Self {
                        path,
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("could not own a private checkpoint directory".into())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for PrivateDir {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.is_dir() && metadata.dev() == self.device && metadata.ino() == self.inode
        }) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
