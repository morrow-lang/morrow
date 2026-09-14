#[test]
#[ignore = "requires MORROW_BROWSER and an embedded server via MORROW_WEB_CHECK_ORIGIN"]
fn rejects_mismatched_successful_assets_and_keeps_previous_offline_app() {
    xtask::web::acceptance::run_integrity(std::path::Path::new("dist/morrow-web")).unwrap();
}
