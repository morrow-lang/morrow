//! Supported workspace/tooling contracts; old generated-editor builds are retired.
use std::{
    fs,
    path::{Path, PathBuf},
};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}
fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap()
}
#[test]
fn editor_integration_is_rust_lsp_without_a_second_parser_toolchain() {
    for path in [
        "editor",
        "scripts/editor",
        "scripts/generate_editor_support.py",
        "scripts/package_zed.py",
    ] {
        assert!(
            !root().join(path).exists(),
            "retired integration remains: {path}"
        );
    }
    assert!(root().join("crates/fern/src/lsp.rs").is_file());
    assert!(root().join("crates/fern/tests/lsp.rs").is_file());
}
#[test]
fn ci_and_release_use_workspace_acceptance_and_rust_distribution() {
    let ci = read(".github/workflows/ci.yml");
    let release = read(".github/workflows/release.yml");
    for workflow in [&ci, &release] {
        assert!(workflow.contains("ubuntu-latest"));
        assert!(workflow.contains("macos-latest"));
        assert!(workflow.contains("cargo xtask check"));
        for retired in [
            "libgc-dev",
            "bdw-gc",
            "scripts/",
            "compiler-rs",
            "clang-format",
            "fern-qbe",
        ] {
            assert!(!workflow.contains(retired), "{retired}");
        }
    }
    assert!(release.contains("cargo xtask package"));
    assert!(release.contains("if-no-files-found: error"));
}
#[test]
fn release_metadata_updates_workspace_version_and_every_local_lock_entry() {
    let config: serde_json::Value =
        serde_json::from_str(&read(".github/release-please-config.json")).unwrap();
    let package = &config["packages"]["."];
    let extra = package["extra-files"].as_array().unwrap();
    assert!(
        extra.iter().any(|value| value["path"] == "Cargo.toml"
            && value["jsonpath"] == "$.workspace.package.version")
    );
    for lock_path in [
        "Cargo.lock",
        "benchmarks/compiler-phases/Cargo.lock",
        "benchmarks/network-codecs/Cargo.lock",
        "benchmarks/message-path/Cargo.lock",
    ] {
        let lock = read(lock_path);
        for block in lock
            .split("[[package]]")
            .skip(1)
            .filter(|block| !block.lines().any(|line| line.starts_with("source = ")))
        {
            let name = block
                .lines()
                .find_map(|line| {
                    line.strip_prefix("name = \"")
                        .and_then(|rest| rest.strip_suffix('"'))
                })
                .unwrap();
            if matches!(
                name,
                "fern-phase-benchmarks" | "fern-network-codecs" | "fern-message-path"
            ) {
                continue;
            }
            assert!(
                extra.iter().any(|value| value["path"] == lock_path
                    && value["jsonpath"]
                        .as_str()
                        .is_some_and(|query| query.contains(&format!("'{}'", name)))),
                "missing release update for {name} in {lock_path}"
            );
        }
    }
    assert!(!read(".github/release-please-config.json").contains("include/version.h"));
    let manifest: serde_json::Value =
        serde_json::from_str(&read(".github/.release-please-manifest.json")).unwrap();
    let cargo = read("Cargo.toml");
    let section = cargo
        .split("[workspace.package]")
        .nth(1)
        .unwrap()
        .split('[')
        .next()
        .unwrap();
    let version = section
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .unwrap();
    assert_eq!(manifest["."], version);
    assert_eq!(read(".github/release-version.txt").trim(), version);
}

#[test]
fn standalone_measurements_are_excluded_from_the_production_workspace() {
    let manifest = read("Cargo.toml");
    let exclusions = manifest
        .split("exclude = [")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap();
    for path in [
        "benchmarks/compiler-phases",
        "benchmarks/network-codecs",
        "benchmarks/message-path",
    ] {
        assert!(
            exclusions.contains(&format!("\"{path}\"")),
            "missing workspace exclusion: {path}"
        );
    }
}
#[test]
fn primary_guides_describe_only_supported_cargo_commands_and_components() {
    for path in ["README.md", "BUILD.md", "CLAUDE.md", "FERN_STYLE.md"] {
        let source = read(path);
        assert!(source.contains("cargo xtask"), "{path}");
        for retired in [
            "mise run rust-check",
            "mise run debug",
            "fern-c",
            "fern-qbe",
            "editor/zed-fern",
            "Boehm",
            "clang-format",
        ] {
            assert!(!source.contains(retired), "{path}: {retired}");
        }
    }
}
