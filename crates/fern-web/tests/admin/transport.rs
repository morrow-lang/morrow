use super::*;

fn body(response: &str) -> &str {
    response.split_once("\r\n\r\n").unwrap().1
}

#[tokio::test]
async fn dashboard_requires_a_live_session_and_never_caches_system_information() {
    let server = Server::start().await;
    for path in ["/admin", "/admin/", "/admin/status"] {
        let denied = server.http("GET", path, "", "").await;
        assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
        assert!(denied.contains("cache-control: no-store"));
        assert!(!denied.contains("process_id"));
    }
    let (cookie, csrf) = server.session().await;
    for path in ["/admin", "/admin/status"] {
        let foreign = server
            .http(
                "GET",
                path,
                &format!("Cookie: {cookie}\r\nOrigin: https://foreign.example\r\n"),
                "",
            )
            .await;
        assert!(foreign.starts_with("HTTP/1.1 403"), "{foreign}");
        assert!(foreign.contains("cache-control: no-store"));
        let valid = server
            .http("GET", path, &format!("Cookie: {cookie}\r\n"), "")
            .await;
        assert!(valid.starts_with("HTTP/1.1 200"), "{valid}");
        assert!(valid.contains("cache-control: no-store"));
        assert!(!body(&valid).contains("a-long-test-access-key"));
        assert!(!body(&valid).contains(&csrf));
        assert!(!body(&valid).contains(cookie.strip_prefix("fern_session=").unwrap()));
        if path == "/admin" {
            assert!(valid.contains("script-src 'none'"));
            assert!(valid.contains("frame-ancestors 'none'"));
            assert!(body(&valid).contains("Fern system"));
            assert!(body(&valid).contains("/admin/status"));
            assert!(body(&valid).contains("/admin/style.css"));
        }
    }
    let logout = server
        .http(
            "POST",
            "/logout",
            &format!(
                "Origin: http://{}\r\nCookie: {cookie}\r\nX-Fern-CSRF: {csrf}\r\n",
                server.address
            ),
            "",
        )
        .await;
    assert!(logout.starts_with("HTTP/1.1 204"));
    for path in ["/admin", "/admin/status"] {
        let denied = server
            .http("GET", path, &format!("Cookie: {cookie}\r\n"), "")
            .await;
        assert!(denied.starts_with("HTTP/1.1 401"));
    }
}

#[tokio::test]
async fn dashboard_reports_the_real_server_configuration_and_socket_occupancy() {
    let server = Server::configured(|config| {
        config.workers = 2;
        config.limits.max_connections = 7;
    })
    .await;
    let (cookie, csrf) = server.session().await;
    let headers = format!("Cookie: {cookie}\r\n");
    let initial = server.http("GET", "/admin/status", &headers, "").await;
    assert!(initial.starts_with("HTTP/1.1 200"), "{initial}");
    let initial: serde_json::Value = serde_json::from_str(body(&initial)).unwrap();
    assert_eq!(initial["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(initial["schema_version"], 1);
    assert_eq!(initial["operating_system"], std::env::consts::OS);
    assert_eq!(initial["architecture"], std::env::consts::ARCH);
    assert_eq!(initial["process_id"], std::process::id());
    assert_eq!(initial["durability"], "ephemeral");
    assert_eq!(initial["websocket_limit"], 7);
    assert_eq!(initial["websocket_connections"], 0);
    assert_eq!(initial["embedded_asset_bytes"], 0);
    assert_eq!(initial["runtime"]["workers"].as_array().unwrap().len(), 2);
    assert!(initial["available_parallelism"].as_u64().unwrap() >= 1);
    assert!(initial["executable_bytes"].as_u64().unwrap() > 0);
    let mut socket = server.socket(&cookie, &csrf).await;
    join(&mut socket, None).await;
    let connected = server.http("GET", "/admin/status", &headers, "").await;
    let connected: serde_json::Value = serde_json::from_str(body(&connected)).unwrap();
    assert_eq!(connected["websocket_connections"], 1);
    socket.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let response = server.http("GET", "/admin/status", &headers, "").await;
            let status: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
            if status["websocket_connections"] == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
