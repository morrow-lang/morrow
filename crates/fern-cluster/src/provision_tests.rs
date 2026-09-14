use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fern-publish-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn staging(&self) -> Staging {
        std::fs::create_dir(self.0.join("staging")).unwrap();
        let parent = files::open_directory(&self.0).unwrap();
        let directory = files::open_child(&parent, "staging").unwrap();
        Staging {
            parent,
            name: "staging".into(),
            directory,
            children: Vec::new(),
            published: false,
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn replaced_staging_is_not_published_or_deleted_during_cleanup() {
    let root = Directory::new();
    let mut staging = root.staging();
    std::fs::rename(root.0.join("staging"), root.0.join("retained-original")).unwrap();
    std::fs::create_dir(root.0.join("staging")).unwrap();
    std::fs::write(root.0.join("staging/sentinel"), b"other entry").unwrap();
    assert!(staging.publish("cluster").is_err());
    drop(staging);
    assert!(!root.0.join("cluster").exists());
    assert_eq!(
        std::fs::read(root.0.join("staging/sentinel")).unwrap(),
        b"other entry"
    );
}
#[test]
fn destination_created_after_preflight_is_not_replaced() {
    let root = Directory::new();
    let mut staging = root.staging();
    std::fs::create_dir(root.0.join("cluster")).unwrap();
    assert!(staging.publish("cluster").is_err());
    drop(staging);
    assert!(root.0.join("cluster").is_dir());
    assert!(!root.0.join("staging").exists());
}
