#[test]
#[ignore = "requires FERN_BROWSER and an embedded server via FERN_WEB_CHECK_ORIGIN"]
fn rejects_mismatched_successful_assets_and_keeps_previous_offline_app() {
    xtask::web::acceptance::run_integrity(std::path::Path::new("dist/fern-web")).unwrap();
}
