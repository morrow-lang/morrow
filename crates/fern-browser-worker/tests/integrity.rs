use fern_browser_worker::{ASSETS, asset_integrities};

#[test]
fn malformed_or_incomplete_integrities_never_disable_browser_verification() {
    let hash = "sha256-ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=";
    let manifest = ASSETS
        .iter()
        .map(|path| format!("{path} {hash}\n"))
        .collect::<String>();
    assert_eq!(asset_integrities(&manifest).unwrap().len(), ASSETS.len());
    for invalid in [
        String::new(),
        manifest.replace("/style.css", "/index.html"),
        manifest.replace("sha256-", "sha1-"),
        manifest.replace("ungWv", "****!"),
        format!("{manifest}/extra {hash}\n"),
    ] {
        assert!(asset_integrities(&invalid).is_err());
    }
}
