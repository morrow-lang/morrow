use super::*;
use std::{
    fs,
    io::Cursor,
    os::unix::fs::{PermissionsExt, symlink},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    stage: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "morrow-dist-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let stage = root.join("stage ' λ $literal");
        fs::create_dir(&stage).unwrap();
        for name in REQUIRED {
            fs::write(
                stage.join(name),
                if *name == "morrow-package.json" {
                    MARKER.as_bytes()
                } else {
                    b"fixture\n"
                },
            )
            .unwrap();
            fs::set_permissions(
                stage.join(name),
                fs::Permissions::from_mode(
                    if ["morrow", "morrow-test-supervisor"].contains(name) {
                        0o755
                    } else {
                        0o644
                    },
                ),
            )
            .unwrap();
        }
        Self { root, stage }
    }
    fn package(&self) -> Result<PathBuf, String> {
        package(&self.root, &self.stage, &self.root.join("dist"), "0.1.0")
    }
    fn prefix(&self) -> PathBuf {
        self.root.join("installed ' λ $literal")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn checksum(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sha256", path.display()))
}
fn forged(f: &Fixture, changes: &[(&str, tar::EntryType, u32, &[u8])]) -> PathBuf {
    let path = f.root.join("forged.tar.gz");
    let file = fs::File::create(&path).unwrap();
    let gzip = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut tar = tar::Builder::new(gzip);
    for name in REQUIRED {
        let change = changes.iter().find(|(n, _, _, _)| n == name);
        let content = fs::read(f.stage.join(name)).unwrap();
        let (kind, mode, data) = change
            .map(|(_, kind, mode, data)| (*kind, *mode, *data))
            .unwrap_or((
                tar::EntryType::Regular,
                if ["morrow", "morrow-test-supervisor"].contains(name) {
                    0o755
                } else {
                    0o644
                },
                &content,
            ));
        let mut header = tar::Header::new_ustar();
        header
            .set_path(format!("morrow-0.1.0-linux-arm64/{name}"))
            .unwrap();
        header.set_entry_type(kind);
        header.set_mode(mode);
        header.set_size(data.len() as u64);
        if kind.is_symlink() {
            header.set_link_name("/outside/helper").unwrap();
        }
        header.set_cksum();
        tar.append(&header, Cursor::new(data)).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
    use sha2::Digest;
    let digest = sha2::Sha256::digest(fs::read(&path).unwrap());
    fs::write(
        checksum(&path),
        format!(
            "{}  forged.tar.gz\n",
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
    )
    .unwrap();
    path
}
#[test]
fn package_contains_exact_rust_components_and_valid_checksum() {
    let f = Fixture::new();
    let archive = f.package().unwrap();
    verify(&archive, &checksum(&archive)).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(
        fs::File::open(archive).unwrap(),
    ));
    let names: std::collections::BTreeSet<_> = archive
        .entries()
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(names, REQUIRED.iter().map(|s| s.to_string()).collect());
}
#[test]
fn missing_required_inputs_reject_before_output_publication() {
    for name in REQUIRED {
        let f = Fixture::new();
        fs::remove_file(f.stage.join(name)).unwrap();
        assert!(f.package().is_err(), "{name}");
        assert!(!f.root.join("dist").exists());
    }
}
#[test]
fn helpers_require_owner_execute_and_regular_input_files() {
    for name in ["morrow", "morrow-test-supervisor"] {
        for mode in [0o644, 0o641] {
            let f = Fixture::new();
            fs::set_permissions(f.stage.join(name), fs::Permissions::from_mode(mode)).unwrap();
            assert!(verify_layout(&f.stage).is_err());
        }
        let f = Fixture::new();
        fs::remove_file(f.stage.join(name)).unwrap();
        symlink(f.stage.join("LICENSE"), f.stage.join(name)).unwrap();
        assert!(verify_layout(&f.stage).is_err());
    }
}
#[test]
fn marker_requires_exact_typed_rust_identity() {
    for value in [
        "",
        "{",
        "{}",
        r#"{"format":2.0,"compiler":"rust","backend":"cranelift","runtime":"rust"}"#,
        r#"{"format":true,"compiler":"rust","backend":"cranelift","runtime":"rust"}"#,
        r#"{"format":2,"compiler":"rust","backend":"qbe","runtime":"rust"}"#,
    ] {
        let f = Fixture::new();
        fs::write(f.stage.join("morrow-package.json"), value).unwrap();
        assert!(verify_layout(&f.stage).is_err(), "{value}");
    }
}
#[test]
fn optional_readme_obeys_regular_bounded_file_contract() {
    let f = Fixture::new();
    symlink(f.stage.join("LICENSE"), f.stage.join("README.md")).unwrap();
    assert!(verify_layout(&f.stage).is_err());
    fs::remove_file(f.stage.join("README.md")).unwrap();
    fs::File::create(f.stage.join("README.md"))
        .unwrap()
        .set_len(128 * 1024 * 1024 + 1)
        .unwrap();
    assert!(verify_layout(&f.stage).is_err());
}
#[test]
fn invalid_repack_keeps_previous_archive_and_checksum() {
    let f = Fixture::new();
    let path = f.package().unwrap();
    let before = (fs::read(&path).unwrap(), fs::read(checksum(&path)).unwrap());
    fs::write(f.stage.join("morrow-package.json"), "{}").unwrap();
    assert!(f.package().is_err());
    assert_eq!(
        (fs::read(&path).unwrap(), fs::read(checksum(&path)).unwrap()),
        before
    );
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
}
#[test]
fn valid_checksum_cannot_hide_non_executable_or_link_helpers() {
    for (kind, mode) in [
        (tar::EntryType::Regular, 0o641),
        (tar::EntryType::Symlink, 0o755),
    ] {
        let f = Fixture::new();
        let path = forged(&f, &[("morrow-test-supervisor", kind, mode, b"fixture")]);
        assert!(verify(&path, &checksum(&path)).is_err());
    }
}
#[test]
fn archive_checksum_and_marker_corruption_are_rejected() {
    let f = Fixture::new();
    let path = forged(
        &f,
        &[("morrow-package.json", tar::EntryType::Regular, 0o644, b"{}")],
    );
    assert!(verify(&path, &checksum(&path)).is_err());
    let path = f.package().unwrap();
    fs::write(checksum(&path), "0".repeat(64)).unwrap();
    assert!(verify(&path, &checksum(&path)).is_err());
}
#[test]
fn install_uses_literal_prefix_and_complete_component_layout() {
    let f = Fixture::new();
    install(&f.stage, &f.prefix()).unwrap();
    for name in REQUIRED {
        let directory = if ["LICENSE", "THIRD_PARTY_NOTICES.md"].contains(name) {
            "share/morrow"
        } else {
            "bin"
        };
        assert_eq!(
            fs::read(f.prefix().join(directory).join(name)).unwrap(),
            fs::read(f.stage.join(name)).unwrap()
        );
    }
    assert!(!f.prefix().join("bin/fern-c").exists());
    assert!(!f.prefix().join("bin/fern-qbe").exists());
}
#[test]
fn install_sets_executable_and_document_permissions() {
    let f = Fixture::new();
    install(&f.stage, &f.prefix()).unwrap();
    for (name, mode) in [
        ("bin/morrow", 0o755),
        ("bin/morrow-test-supervisor", 0o755),
        ("bin/libmorrow_runtime.a", 0o644),
        ("share/morrow/LICENSE", 0o644),
    ] {
        assert_eq!(
            fs::metadata(f.prefix().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            mode
        );
    }
}
#[test]
fn install_replaces_existing_files_as_complete_components() {
    let f = Fixture::new();
    install(&f.stage, &f.prefix()).unwrap();
    fs::write(f.stage.join("morrow"), b"replacement compiler").unwrap();
    install(&f.stage, &f.prefix()).unwrap();
    assert_eq!(
        fs::read(f.prefix().join("bin/morrow")).unwrap(),
        b"replacement compiler"
    );
}
#[test]
fn install_preflights_all_directory_destinations_before_replacing_any_file() {
    for name in REQUIRED {
        let f = Fixture::new();
        let location = if ["LICENSE", "THIRD_PARTY_NOTICES.md"].contains(name) {
            "share/morrow"
        } else {
            "bin"
        };
        let dest = f.prefix().join(location).join(name);
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("keep"), b"sentinel").unwrap();
        let previous = f.prefix().join("bin/morrow");
        if *name != "morrow" {
            fs::create_dir_all(previous.parent().unwrap()).unwrap();
            fs::write(&previous, b"previous").unwrap();
        }
        assert!(install(&f.stage, &f.prefix()).is_err(), "{name}");
        assert_eq!(fs::read(dest.join("keep")).unwrap(), b"sentinel");
        if *name != "morrow" {
            assert_eq!(fs::read(previous).unwrap(), b"previous");
        }
    }
}
#[test]
fn install_rejects_symlink_destinations_without_touching_targets() {
    for directory in [false, true] {
        let f = Fixture::new();
        fs::create_dir_all(f.prefix().join("bin")).unwrap();
        let outside = f.root.join("outside");
        if directory {
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("keep"), b"sentinel").unwrap();
        } else {
            fs::write(&outside, b"sentinel").unwrap();
        }
        symlink(&outside, f.prefix().join("bin/morrow")).unwrap();
        assert!(install(&f.stage, &f.prefix()).is_err());
        assert_eq!(
            fs::read(if directory {
                outside.join("keep")
            } else {
                outside
            })
            .unwrap(),
            b"sentinel"
        );
        assert!(f.prefix().join("bin/morrow").is_symlink());
    }
}
#[test]
fn install_rejects_symlink_parent_without_creating_external_components() {
    let f = Fixture::new();
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::create_dir(f.prefix()).unwrap();
    symlink(&outside, f.prefix().join("bin")).unwrap();
    assert!(install(&f.stage, &f.prefix()).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
#[test]
fn incomplete_installation_input_leaves_prior_destination_unchanged() {
    let f = Fixture::new();
    install(&f.stage, &f.prefix()).unwrap();
    fs::write(f.stage.join("morrow"), b"new compiler").unwrap();
    fs::remove_file(f.stage.join("libmorrow_runtime.a")).unwrap();
    assert!(install(&f.stage, &f.prefix()).is_err());
    assert_eq!(
        fs::read(f.prefix().join("bin/morrow")).unwrap(),
        b"fixture\n"
    );
}
#[test]
fn same_source_destination_is_rejected_without_mutating_the_staged_bundle() {
    let f = Fixture::new();
    fs::create_dir(f.stage.join("bin")).unwrap();
    symlink(f.stage.join("morrow"), f.stage.join("bin/morrow")).unwrap();
    assert!(install(&f.stage, &f.stage).is_err());
    assert_eq!(fs::read(f.stage.join("morrow")).unwrap(), b"fixture\n");
}
#[test]
fn staging_parent_links_are_not_followed() {
    let f = Fixture::new();
    let link = f.root.join("linked-stage");
    symlink(&f.stage, &link).unwrap();
    assert!(install(&link, &f.prefix()).is_err());
    assert!(!f.prefix().exists());
}

#[test]
fn uninstall_removes_only_published_components_and_is_idempotent() {
    let f = Fixture::new();
    install(&f.stage, &f.prefix()).unwrap();
    fs::write(f.prefix().join("bin/user-tool"), b"keep").unwrap();
    fs::write(f.prefix().join("share/morrow/user-note"), b"keep").unwrap();
    uninstall(&f.prefix()).unwrap();
    uninstall(&f.prefix()).unwrap();
    for name in REQUIRED {
        assert!(
            !f.prefix()
                .join(if ["LICENSE", "THIRD_PARTY_NOTICES.md"].contains(name) {
                    "share/morrow"
                } else {
                    "bin"
                })
                .join(name)
                .exists()
        );
    }
    assert_eq!(fs::read(f.prefix().join("bin/user-tool")).unwrap(), b"keep");
    assert_eq!(
        fs::read(f.prefix().join("share/morrow/user-note")).unwrap(),
        b"keep"
    );
}
#[test]
fn uninstall_preflights_all_names_before_removing_any_component() {
    let f = Fixture::new();
    install(&f.stage, &f.prefix()).unwrap();
    let notice = f.prefix().join("share/morrow/THIRD_PARTY_NOTICES.md");
    fs::remove_file(&notice).unwrap();
    symlink(f.stage.join("THIRD_PARTY_NOTICES.md"), &notice).unwrap();
    assert!(uninstall(&f.prefix()).is_err());
    assert_eq!(
        fs::read(f.prefix().join("bin/morrow")).unwrap(),
        b"fixture\n"
    );
    assert!(notice.is_symlink());
}
#[test]
fn uninstall_of_missing_prefix_never_creates_it() {
    let f = Fixture::new();
    uninstall(&f.prefix()).unwrap();
    assert!(!f.prefix().exists());
}
fn rewrite_gzip(path: &Path, bytes: &[u8]) {
    use std::io::Write;
    let mut gzip =
        flate2::write::GzEncoder::new(fs::File::create(path).unwrap(), flate2::Compression::fast());
    gzip.write_all(bytes).unwrap();
    gzip.finish().unwrap();
    use sha2::Digest;
    let hash = sha2::Sha256::digest(fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(
        checksum(path),
        format!("{hash}  {}\n", path.file_name().unwrap().to_str().unwrap()),
    )
    .unwrap();
}
#[test]
fn archive_without_tar_end_blocks_rejects_even_with_valid_checksum() {
    use std::io::Read;
    let f = Fixture::new();
    let path = f.package().unwrap();
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(fs::File::open(&path).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.ends_with(&[0; 1024]));
    bytes.truncate(bytes.len() - 1024);
    rewrite_gzip(&path, &bytes);
    assert!(verify(&path, &checksum(&path)).is_err());
}

#[test]
fn forged_privileged_permissions_are_not_part_of_the_release_contract() {
    let f = Fixture::new();
    let path = forged(
        &f,
        &[("morrow", tar::EntryType::Regular, 0o4755, b"fixture")],
    );
    assert!(verify(&path, &checksum(&path)).is_err());
}
#[test]
fn archive_rejects_duplicate_traversal_and_hidden_trailing_members() {
    use std::io::Read;
    for attack in [
        "duplicate",
        "traversal",
        "absolute",
        "hidden",
        "extra-gzip",
        "crc",
    ] {
        let f = Fixture::new();
        let path = f.package().unwrap();
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(fs::File::open(&path).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        match attack {
            "duplicate" => {
                let entry = bytes[..1024].to_vec();
                bytes.truncate(bytes.len() - 1024);
                bytes.extend(entry);
                bytes.extend([0; 1024]);
            }
            "traversal" | "absolute" => {
                let mut header = tar::Header::new_ustar();
                header.as_mut_bytes().copy_from_slice(&bytes[..512]);
                header.as_mut_bytes()[..100].fill(0);
                let name = if attack == "absolute" {
                    b"/outside/morrow".as_slice()
                } else {
                    b"morrow-root/../morrow"
                };
                header.as_mut_bytes()[..name.len()].copy_from_slice(name);
                header.set_cksum();
                bytes[..512].copy_from_slice(header.as_bytes());
            }
            "hidden" => bytes.extend_from_slice(b"unexpected payload"),
            _ => {}
        }
        rewrite_gzip(&path, &bytes);
        if attack == "extra-gzip" || attack == "crc" {
            let mut compressed = fs::read(&path).unwrap();
            if attack == "extra-gzip" {
                compressed.extend_from_within(..);
            } else {
                let index = compressed.len() - 8;
                compressed[index] ^= 1;
            }
            fs::write(&path, &compressed).unwrap();
            use sha2::Digest;
            let hash = sha2::Sha256::digest(&compressed)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            fs::write(
                checksum(&path),
                format!("{hash}  {}\n", path.file_name().unwrap().to_str().unwrap()),
            )
            .unwrap();
        }
        assert!(verify(&path, &checksum(&path)).is_err(), "{attack}");
    }
}
#[test]
fn aggregate_staging_limit_is_checked_before_reading_component_payloads() {
    let f = Fixture::new();
    for name in [
        "morrow",
        "morrow-test-supervisor",
        "libmorrow_runtime.a",
        "LICENSE",
        "THIRD_PARTY_NOTICES.md",
    ] {
        fs::OpenOptions::new()
            .write(true)
            .open(f.stage.join(name))
            .unwrap()
            .set_len(128 * 1024 * 1024)
            .unwrap();
    }
    assert!(verify_layout(&f.stage).is_err());
    assert!(!f.root.join("dist").exists());
}
#[test]
fn invalid_versions_and_linked_output_directories_never_publish() {
    let f = Fixture::new();
    for value in [
        "../1.0.0",
        "1.0",
        "1.0.0/escape",
        "1.0.0+",
        "1.0.0-",
        "01.0.0",
    ] {
        assert!(package(&f.root, &f.stage, &f.root.join("dist"), value).is_err());
    }
    assert!(!f.root.join("dist").exists());
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, f.root.join("dist")).unwrap();
    assert!(f.package().is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
#[test]
fn held_destination_directory_cannot_be_redirected_by_a_parent_link_swap() {
    use std::io::Write;
    let f = Fixture::new();
    let parent = super::fs::Dir::open(&f.root, false).unwrap();
    let bin = parent.child(std::ffi::OsStr::new("bin"), true).unwrap();
    let mut temporary = super::fs::Temporary::new(&parent).unwrap();
    temporary
        .create("compiler")
        .unwrap()
        .write_all(b"complete compiler")
        .unwrap();
    fs::rename(f.root.join("bin"), f.root.join("displaced")).unwrap();
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, f.root.join("bin")).unwrap();
    temporary.dir.rename("compiler", &bin, "morrow").unwrap();
    assert_eq!(
        fs::read(f.root.join("displaced/morrow")).unwrap(),
        b"complete compiler"
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
