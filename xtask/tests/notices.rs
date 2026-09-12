use xtask::notices;

use std::{fs, path::Path};

fn package(root: &Path, name: &str, license: &str) -> serde_json::Value {
    serde_json::json!({"name": name, "version": "1.2.3", "license": license,
        "manifest_path": root.join("Cargo.toml"), "source": "registry+fixture", "license_file": null})
}

#[test]
fn notices_preserve_existing_text_deduplicate_and_include_conjunctive_notices() {
    let temp = xtask::Temporary::new(&std::env::temp_dir()).unwrap();
    fs::write(temp.0.join("LICENSE-MIT"), "Permission fixture\n").unwrap();
    fs::write(temp.0.join("LICENSE-UNICODE"), "Unicode fixture\n").unwrap();
    fs::write(temp.0.join("NOTICE"), "Attribution fixture\n").unwrap();
    let existing = "# Third-party notices\n\n## Dependency inventory\n\n| Package | Declared license | Selected license and notices |\n| --- | --- | --- |\n\n## License texts\n\n### License 1\n\nOriginal owner.\n\n```text\nPermission fixture\n```\n\n## SQLite\n\nKeep this text exactly.\n";
    let metadata = serde_json::json!({"packages": [package(&temp.0, "fixture", "(MIT OR Apache-2.0) AND Unicode-3.0")]});
    let text = notices::render(existing, &metadata).unwrap();
    assert_eq!(text.matches("Permission fixture").count(), 1);
    assert!(text.contains("[MIT: LICENSE-MIT](#license-1)"));
    assert!(text.contains("Unicode fixture"));
    assert!(text.contains("Attribution fixture"));
    assert!(text.ends_with("## SQLite\n\nKeep this text exactly.\n"));
    assert_eq!(notices::render(&text, &metadata).unwrap(), text);
}

#[test]
fn missing_license_and_unsupported_expression_require_review() {
    let temp = xtask::Temporary::new(&std::env::temp_dir()).unwrap();
    let existing = "## Dependency inventory\n\n## License texts\n\n## SQLite\n";
    let metadata = serde_json::json!({"packages": [package(&temp.0, "missing", "MIT")]});
    assert!(
        notices::render(existing, &metadata)
            .unwrap_err()
            .contains("license files")
    );
    fs::write(temp.0.join("LICENSE"), "Unknown terms").unwrap();
    let metadata = serde_json::json!({"packages": [package(&temp.0, "unknown", "Custom-License")]});
    assert!(
        notices::render(existing, &metadata)
            .unwrap_err()
            .contains("review")
    );
    fs::write(temp.0.join("LICENSE-MIT"), "Permission fixture").unwrap();
    fs::write(temp.0.join("LICENSE-CUSTOM"), "Custom mandatory terms").unwrap();
    let metadata = serde_json::json!({"packages": [package(&temp.0, "unknown-and", "MIT AND Custom-License")]});
    assert!(
        notices::render(existing, &metadata)
            .unwrap_err()
            .contains("review")
    );
}

#[test]
fn dependency_upgrade_removes_stale_inventory_row_but_preserves_full_text() {
    let temp = xtask::Temporary::new(&std::env::temp_dir()).unwrap();
    fs::write(temp.0.join("LICENSE-MIT"), "Permission fixture").unwrap();
    let existing = "## Dependency inventory\n\n## License texts\n\n## SQLite\n";
    let mut package = package(&temp.0, "fixture", "MIT");
    let first =
        notices::render(existing, &serde_json::json!({"packages":[package.clone()]})).unwrap();
    package["version"] = serde_json::json!("1.2.4");
    let updated = notices::render(&first, &serde_json::json!({"packages":[package]})).unwrap();
    assert!(!updated.contains("[`fixture` 1.2.3]"));
    assert!(updated.contains("[`fixture` 1.2.4]"));
    assert_eq!(updated.matches("Permission fixture").count(), 1);
}

#[test]
fn browser_inventory_excludes_unreachable_native_workspace_dependencies() {
    let metadata = serde_json::json!({
        "workspace_members": ["browser", "runtime"],
        "packages": [
            {"id":"browser", "name":"fern-browser", "source":null},
            {"id":"runtime", "name":"fern-runtime", "source":null},
            {"id":"dom", "name":"web-sys", "source":"registry"},
            {"id":"sqlite", "name":"sqlite-wasm-rs", "source":"registry"}
        ],
        "resolve":{"nodes":[
            {"id":"browser", "deps":[{"pkg":"dom"}]},
            {"id":"runtime", "deps":[{"pkg":"sqlite"}]},
            {"id":"dom", "deps":[]}, {"id":"sqlite", "deps":[]}
        ]}
    });
    let selected = notices::target_packages(&metadata, true).unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["web-sys"]
    );
    assert_eq!(notices::target_packages(&metadata, false).unwrap().len(), 2);
}
