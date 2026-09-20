//! Provisioning is available before any embedded browser assets or authentication setup.
use morrow_cluster::NodeSettings;
use std::{
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-cluster-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_morrow-web"));
    command
        .env_remove("MORROW_WEB_ACCESS_KEY")
        .env_remove("MORROW_WEB_CLUSTER");
    command
}
#[test]
fn initializes_loadable_node_bundles_without_assets_and_never_overwrites() {
    let root = Directory::new();
    let target = root.0.join("cluster");
    let run = || {
        command()
            .arg("--cluster-init")
            .arg(&target)
            .args(["demo", "a=127.0.0.1:32001", "_b=127.0.0.1:32002"])
            .output()
            .unwrap()
    };
    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("MORROW_WEB_CLUSTER="));
    assert!(output.contains("node.json"));
    for index in 0..2 {
        NodeSettings::load(&target.join(format!("node-{index}/node.json"))).unwrap();
    }
    let before = std::fs::read(target.join("node-0/key.der")).unwrap();
    assert!(!run().status.success());
    assert_eq!(
        std::fs::read(target.join("node-0/key.der")).unwrap(),
        before
    );
}
#[test]
fn invalid_arguments_publish_no_credentials() {
    let root = Directory::new();
    let target = root.0.join("cluster");
    for tail in [
        vec![],
        vec!["demo"],
        vec!["demo", "a=bad-address"],
        vec!["demo", "a=127.0.0.1:0"],
        vec!["demo", "a=127.0.0.1:32001", "a=127.0.0.1:32002"],
    ] {
        let result = command()
            .arg("--cluster-init")
            .arg(&target)
            .args(tail)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!target.exists());
    }
    let result = command()
        .arg("--cluster-init")
        .arg(&target)
        .arg("demo")
        .args((0..17).map(|n| format!("n{n}=127.0.0.1:{}", 32000 + n)))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!target.exists());
}
#[test]
fn help_describes_cluster_provisioning_and_runtime_settings() {
    let result = command().arg("--help").output().unwrap();
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("--cluster-init"));
    assert!(output.contains("MORROW_WEB_CLUSTER"));
    assert!(output.contains("GET /session issues an HttpOnly session cookie"));
}
