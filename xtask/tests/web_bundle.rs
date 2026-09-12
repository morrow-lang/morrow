use std::fs;
use xtask::web::{ASSETS, asset_digest, worker_script};

#[test]
fn worker_integrities_pin_every_asset_to_its_exact_bytes() {
    let directory = xtask::Temporary::new(&std::env::temp_dir()).unwrap();
    assets(&directory.0);
    fs::write(directory.0.join("style.css"), b"abc").unwrap();
    let manifest = xtask::web::integrity_manifest(&directory.0).unwrap();
    assert_eq!(manifest.lines().count(), ASSETS.len());
    assert!(
        manifest
            .lines()
            .any(|line| line == "/style.css sha256-ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=")
    );
    fs::write(directory.0.join("style.css"), b"tampered").unwrap();
    assert_ne!(
        manifest,
        xtask::web::integrity_manifest(&directory.0).unwrap()
    );
}

fn assets(directory: &std::path::Path) {
    for name in ASSETS {
        let bytes = if name.ends_with(".wasm") {
            b"\0asm\x01\0\0\0".as_slice()
        } else {
            b"public fixture"
        };
        fs::write(directory.join(name), bytes).unwrap();
    }
}

#[test]
fn cache_identity_covers_every_asset_and_rejects_missing_or_foreign_inputs() {
    let directory = xtask::Temporary::new(&std::env::temp_dir()).unwrap();
    assets(&directory.0);
    let original = asset_digest(&directory.0).unwrap();
    assert_eq!(original.len(), 64);
    assert_eq!(asset_digest(&directory.0).unwrap(), original);
    fs::write(directory.0.join("style.css"), b"new public stylesheet").unwrap();
    assert_ne!(asset_digest(&directory.0).unwrap(), original);
    fs::remove_file(directory.0.join("style.css")).unwrap();
    assert!(asset_digest(&directory.0).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("index.html", directory.0.join("style.css")).unwrap();
        assert!(asset_digest(&directory.0).is_err());
    }
}

#[test]
fn worker_is_self_contained_and_invalid_or_oversized_artifacts_reject() {
    let module = b"\0asm\x01\0\0\0";
    let script = worker_script("var wasm_bindgen = {};", module).unwrap();
    assert!(script.contains("initSync"));
    assert!(script.contains("new Uint8Array([0,97,115,109,1,0,0,0])"));
    assert!(!script.contains("fetch("));
    assert!(worker_script("glue", b"not wasm").is_err());
    let oversized = vec![0; 2 * 1024 * 1024 + 1];
    assert!(worker_script("glue", &oversized).is_err());
}

#[test]
fn static_linux_gate_rejects_dynamic_loading_and_wrong_architecture() {
    let mut elf = vec![0; 120];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[18..20].copy_from_slice(&183u16.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());
    elf[64..68].copy_from_slice(&1u32.to_le_bytes());
    assert!(xtask::web::validate_static_linux(&elf, "aarch64-unknown-linux-musl").is_ok());
    assert!(xtask::web::validate_static_linux(&elf, "x86_64-unknown-linux-musl").is_err());
    for kind in [2u32, 3] {
        elf[64..68].copy_from_slice(&kind.to_le_bytes());
        assert!(xtask::web::validate_static_linux(&elf, "aarch64-unknown-linux-musl").is_err());
    }
    assert!(xtask::web::validate_static_linux(&elf[..63], "aarch64-unknown-linux-musl").is_err());
}

#[test]
#[cfg(unix)]
fn executable_source_and_aliases_are_never_output_artifacts() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = xtask::Temporary::new(&std::env::temp_dir()).unwrap();
    let source = directory.0.join("checklist.fn");
    fs::write(&source, b"pub fn answer() -> Int { 42 }").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
    let alias = directory.0.join("alias");
    fs::hard_link(&source, &alias).unwrap();
    let link = directory.0.join("link");
    symlink(&source, &link).unwrap();
    for path in [&source, &alias, &link] {
        assert!(xtask::web::publication::validate(path).is_err());
    }
    assert!(xtask::web::publication::validate(&directory.0.join("new-server")).is_ok());
    assert!(xtask::web::publication::validate(&std::env::current_exe().unwrap()).is_ok());
    assert_eq!(fs::read(&source).unwrap(), b"pub fn answer() -> Int { 42 }");
}
