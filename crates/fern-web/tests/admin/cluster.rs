use super::*;
use fern_cluster::{
    BootId, ClusterId, Hello, IoLimits, LinkId, NodeId, NodeSettings, PROTOCOL_VERSION,
};
use std::os::unix::fs::DirBuilderExt;
struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[tokio::test]
async fn cluster_status_distinguishes_configuration_from_authenticated_connections_without_secrets()
{
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = Directory(std::env::temp_dir().join(format!(
        "fern-admin-cluster-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root.0)
        .unwrap();
    let a_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let b_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let made = fern_cluster::provision(
        &root.0.join("cluster"),
        ClusterId::new("admin-test").unwrap(),
        vec![
            (NodeId::new("a").unwrap(), a_listener.local_addr().unwrap()),
            (NodeId::new("b").unwrap(), b_listener.local_addr().unwrap()),
        ],
    )
    .unwrap();
    let a = NodeSettings::load(&made.nodes[0].settings).unwrap();
    let b = NodeSettings::load(&made.nodes[1].settings).unwrap();
    drop(a_listener);
    let server = Server::configured(|config| config.cluster = Some(a)).await;
    let denied = server.http("GET", "/admin/status", "", "").await;
    assert!(denied.starts_with("HTTP/1.1 401"));
    assert!(!body(&denied).contains("configured_nodes"));
    let (cookie, csrf) = server.session().await;
    let headers = format!("Cookie: {cookie}\r\n");
    let initial = server.http("GET", "/admin/status", &headers, "").await;
    let initial: serde_json::Value = serde_json::from_str(body(&initial)).unwrap();
    assert_eq!(initial["cluster"]["node"], "a");
    assert_eq!(initial["cluster"]["configured_nodes"], 2);
    assert_eq!(initial["cluster"]["connected_nodes"], 0);
    assert_eq!(initial["cluster"]["inbound_streams"], 0);
    assert_eq!(initial["cluster"]["outbound_streams"], 0);
    let hello = Hello {
        version: PROTOCOL_VERSION,
        cluster: b.routing.cluster().clone(),
        node: b.routing.local().clone(),
        manifest: b.routing.manifest(),
        boot: BootId::new([3; 16]).unwrap(),
        link: LinkId::new([5; 16]).unwrap(),
    };
    let peer = fern_cluster::connect(
        &b.routing,
        &NodeId::new("a").unwrap(),
        &b.security,
        hello,
        IoLimits::default(),
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let response = server.http("GET", "/admin/status", &headers, "").await;
            let status: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
            if status["cluster"]["connected_nodes"] == 1 {
                assert_eq!(status["cluster"]["configured_nodes"], 2);
                assert_eq!(status["cluster"]["inbound_streams"], 1);
                assert_eq!(status["cluster"]["forwarded_commands"], 0);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    for path in ["/admin", "/admin/status"] {
        let response = server.http("GET", path, &headers, "").await;
        let content = body(&response);
        for secret in [
            "a-long-test-access-key",
            csrf.as_str(),
            cookie.strip_prefix("fern_session=").unwrap(),
            root.0.to_str().unwrap(),
            "certificate_sha256",
            "key.der",
        ] {
            assert!(!content.contains(secret), "leaked {secret}");
        }
        if path == "/admin" {
            assert!(content.contains("Connected peers"));
            assert!(content.contains("Configured nodes"));
            assert!(content.contains("Fixed room ownership"));
        }
    }
    drop(peer);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let response = server.http("GET", "/admin/status", &headers, "").await;
            let status: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
            if status["cluster"]["connected_nodes"] == 0
                && status["cluster"]["inbound_streams"] == 0
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
