//! Offline caching is restricted to public application assets, never session data.
#![forbid(unsafe_code)]

/// Public cacheable assets, installed atomically as one application revision.
pub const ASSETS: &[&str] = &[
    "/index.html",
    "/bootstrap.js",
    "/morrow_browser.js",
    "/morrow_browser_bg.wasm",
    "/morrow_app.wasm",
    "/style.css",
];

/// Require one SHA-256 integrity for every public path, in canonical bundle order.
pub fn asset_integrities(manifest: &str) -> Result<Vec<(&'static str, &str)>, &'static str> {
    let mut lines = manifest.lines();
    let mut result = Vec::with_capacity(ASSETS.len());
    for path in ASSETS {
        let (name, integrity) = lines
            .next()
            .and_then(|line| line.split_once(' '))
            .ok_or("missing asset integrity")?;
        let digest = integrity
            .strip_prefix("sha256-")
            .ok_or("unsupported asset integrity")?;
        if name != *path
            || digest.len() != 44
            || !digest.ends_with('=')
            || !digest.as_bytes()[..43]
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+/".contains(byte))
        {
            return Err("invalid asset integrity");
        }
        result.push((*path, integrity));
    }
    if lines.next().is_some() {
        return Err("unexpected asset integrity");
    }
    Ok(result)
}

/// Reject queries, cross-origin requests and all application/session endpoints.
pub fn cache_path(
    method: &str,
    same_origin: bool,
    path: &str,
    query: &str,
) -> Option<&'static str> {
    if method != "GET" || !same_origin || !query.is_empty() {
        return None;
    }
    let path = if path == "/" { "/index.html" } else { path };
    ASSETS.iter().copied().find(|asset| *asset == path)
}

#[cfg(target_arch = "wasm32")]
mod worker;
