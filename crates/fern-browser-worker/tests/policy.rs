use fern_browser_worker::cache_path;

#[test]
fn only_exact_same_origin_public_gets_are_cached() {
    for path in [
        "/",
        "/index.html",
        "/bootstrap.js",
        "/fern_browser.js",
        "/fern_browser_bg.wasm",
        "/fern_app.wasm",
        "/style.css",
    ] {
        assert!(cache_path("GET", true, path, "").is_some(), "{path}");
        assert!(cache_path("POST", true, path, "").is_none());
        assert!(cache_path("GET", false, path, "").is_none());
        assert!(cache_path("GET", true, path, "?token=private").is_none());
    }
    for path in [
        "/ws",
        "/session",
        "/logout",
        "/worker.js",
        "/api/tasks",
        "/assets/../session",
        "/index.html/",
        "/INDEX.HTML",
        "/favicon.ico",
    ] {
        assert!(cache_path("GET", true, path, "").is_none(), "{path}");
    }
    assert_eq!(cache_path("GET", true, "/", ""), Some("/index.html"));
}
