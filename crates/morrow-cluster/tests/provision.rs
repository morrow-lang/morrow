use morrow_cluster::{ClusterId, NodeId, NodeSettings, provision};
use std::{
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "morrow-provision-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        Self(root)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn nodes() -> Vec<(NodeId, std::net::SocketAddr)> {
    vec![
        (
            NodeId::new("a").unwrap(),
            "127.0.0.1:32001".parse().unwrap(),
        ),
        (
            NodeId::new("b").unwrap(),
            "127.0.0.1:32002".parse().unwrap(),
        ),
    ]
}
#[test]
fn fresh_node_bundles_share_manifest_and_contain_separate_private_keys() {
    let root = Directory::new();
    let made = provision(
        &root.0.join("cluster"),
        ClusterId::new("test").unwrap(),
        nodes(),
    )
    .unwrap();
    assert_eq!(made.nodes.len(), 2);
    let a = NodeSettings::load(&made.nodes[0].settings).unwrap();
    let b = NodeSettings::load(&made.nodes[1].settings).unwrap();
    assert_eq!(a.routing.manifest(), b.routing.manifest());
    assert_ne!(a.routing.local(), b.routing.local());
    assert_eq!(a.bind, "127.0.0.1:32001".parse().unwrap());
    let key_a = made.nodes[0].settings.parent().unwrap().join("key.der");
    let key_b = made.nodes[1].settings.parent().unwrap().join("key.der");
    assert_ne!(
        std::fs::read(&key_a).unwrap(),
        std::fs::read(&key_b).unwrap()
    );
    for key in [key_a, key_b] {
        let metadata = std::fs::metadata(key).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
    }
    assert!(!made.directory.join("ca.key.der").exists());
}
#[test]
fn existing_directory_is_never_modified_and_invalid_nodes_publish_nothing() {
    let root = Directory::new();
    std::fs::write(root.0.join("sentinel"), b"retain").unwrap();
    assert!(provision(&root.0, ClusterId::new("test").unwrap(), nodes()).is_err());
    assert_eq!(std::fs::read(root.0.join("sentinel")).unwrap(), b"retain");
    for members in [vec![], vec![nodes()[0].clone(), nodes()[0].clone()]] {
        assert!(
            provision(
                &root.0.join("invalid"),
                ClusterId::new("test").unwrap(),
                members
            )
            .is_err()
        );
        assert!(!root.0.join("invalid").exists());
    }
}
#[test]
fn settings_reject_traversal_symlink_key_and_oversized_configuration() {
    let root = Directory::new();
    let made = provision(
        &root.0.join("cluster"),
        ClusterId::new("test").unwrap(),
        nodes(),
    )
    .unwrap();
    let path = &made.nodes[0].settings;
    let original = std::fs::read(path).unwrap();
    let mut config: serde_json::Value = serde_json::from_slice(&original).unwrap();
    config["key"] = "../key.der".into();
    std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(NodeSettings::load(path).is_err());
    std::fs::write(path, &original).unwrap();
    let key = path.parent().unwrap().join("key.der");
    let outside = root.0.join("outside.der");
    std::fs::rename(&key, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &key).unwrap();
    assert!(NodeSettings::load(path).is_err());
    std::fs::write(path, vec![b' '; 65_537]).unwrap();
    assert!(NodeSettings::load(path).is_err());
}

#[test]
fn writable_parent_and_hardlinked_private_key_are_rejected() {
    let root = Directory::new();
    std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(
        provision(
            &root.0.join("cluster"),
            ClusterId::new("test").unwrap(),
            nodes()
        )
        .is_err()
    );
    std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    let made = provision(
        &root.0.join("cluster"),
        ClusterId::new("test").unwrap(),
        nodes(),
    )
    .unwrap();
    let path = &made.nodes[0].settings;
    let key = path.parent().unwrap().join("key.der");
    std::fs::hard_link(&key, root.0.join("alias.der")).unwrap();
    assert!(NodeSettings::load(path).is_err());
}
#[tokio::test]
async fn generated_bundles_connect_with_non_dns_node_ids() {
    use morrow_cluster::{BootId, Frame, Hello, IoLimits, LinkId, PROTOCOL_VERSION};
    let root = Directory::new();
    let a_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let b_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let made = provision(
        &root.0.join("cluster"),
        ClusterId::new("test").unwrap(),
        vec![
            (NodeId::new("..").unwrap(), a_listener.local_addr().unwrap()),
            (
                NodeId::new("_b._").unwrap(),
                b_listener.local_addr().unwrap(),
            ),
        ],
    )
    .unwrap();
    let a = NodeSettings::load(&made.nodes[0].settings).unwrap();
    let b = NodeSettings::load(&made.nodes[1].settings).unwrap();
    let hello = |node: &NodeSettings, value: u8| Hello {
        version: PROTOCOL_VERSION,
        cluster: node.routing.cluster().clone(),
        node: node.routing.local().clone(),
        manifest: node.routing.manifest(),
        boot: BootId::new([value; 16]).unwrap(),
        link: LinkId::new([value; 16]).unwrap(),
    };
    let server = async {
        let (socket, _) = b_listener.accept().await.unwrap();
        let mut peer = morrow_cluster::accept(
            socket,
            &b.routing,
            &b.security,
            hello(&b, 9),
            IoLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(peer.reader.read().await.unwrap(), Some(Frame::Close));
    };
    let client = async {
        let mut peer = morrow_cluster::connect(
            &a.routing,
            b.routing.local(),
            &a.security,
            hello(&a, 3),
            IoLimits::default(),
        )
        .await
        .unwrap();
        peer.writer.write(&Frame::Close).await.unwrap();
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(client, server)
    })
    .await
    .unwrap();
}
