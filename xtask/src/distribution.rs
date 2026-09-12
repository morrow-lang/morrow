//! Closed, bounded Rust compiler distribution and installation.
mod archive;
mod fs;
use fs::{Dir, Temporary};
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
pub const MARKER: &str =
    "{\"format\":2,\"compiler\":\"rust\",\"backend\":\"cranelift\",\"runtime\":\"rust\"}\n";
pub const REQUIRED: &[&str] = &[
    "fern",
    "fern-test-supervisor",
    "libfern_runtime.a",
    "fern-package.json",
    "LICENSE",
    "THIRD_PARTY_NOTICES.md",
];
const OPTIONAL: &[&str] = &["README.md"];
const FILE_LIMIT: u64 = 128 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 512 * 1024 * 1024;
struct Input {
    name: &'static str,
    file: File,
    size: u64,
    mode: u32,
}
fn executable(name: &str) -> bool {
    ["fern", "fern-test-supervisor"].contains(&name)
}
fn validate_marker(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 65536 {
        return Err("package marker exceeds byte limit".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid package marker")?;
    let object = value
        .as_object()
        .ok_or("package marker must be an object")?;
    if object.len() != 4
        || object.get("format").and_then(serde_json::Value::as_u64) != Some(2)
        || object.get("compiler").and_then(serde_json::Value::as_str) != Some("rust")
        || object.get("backend").and_then(serde_json::Value::as_str) != Some("cranelift")
        || object.get("runtime").and_then(serde_json::Value::as_str) != Some("rust")
    {
        return Err(
            "package marker must identify Rust compiler/runtime and Cranelift backend".into(),
        );
    }
    Ok(())
}
fn inputs(staging: &Path) -> Result<Vec<Input>, String> {
    let directory = Dir::open(staging, false)
        .map_err(|e| format!("invalid staging directory {}: {e}", staging.display()))?;
    let mut inputs = Vec::new();
    let mut total = 0u64;
    for &name in REQUIRED.iter().chain(OPTIONAL) {
        let mut file = match directory.file(name) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound && OPTIONAL.contains(&name) => {
                continue;
            }
            Err(error) => return Err(format!("invalid staging component {name}: {error}")),
        };
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        let size = metadata.len();
        let mode = metadata.permissions().mode();
        if size == 0 || size > FILE_LIMIT {
            return Err(format!("{name} must be a bounded nonempty regular file"));
        }
        if executable(name) && mode & 0o100 == 0 {
            return Err(format!("{name} must be owner-executable"));
        }
        total = total
            .checked_add(size)
            .ok_or("release component size overflow")?;
        if total > TOTAL_LIMIT {
            return Err("release components exceed aggregate byte limit".into());
        }
        if name == "fern-package.json" {
            let mut marker = Vec::new();
            Read::by_ref(&mut file)
                .take(65537)
                .read_to_end(&mut marker)
                .map_err(|e| e.to_string())?;
            validate_marker(&marker)?;
            file.rewind().map_err(|e| e.to_string())?;
        }
        inputs.push(Input {
            name,
            file,
            size,
            mode: if executable(name) { 0o755 } else { 0o644 },
        });
    }
    Ok(inputs)
}
/// Validate the complete staged compiler without writing any destination.
pub fn verify_layout(staging: &Path) -> Result<(), String> {
    inputs(staging).map(|_| ())
}
/// Write the exact marker through a private file and one atomic relative rename.
pub fn write_marker(staging: &Path) -> Result<(), String> {
    let directory = Dir::open(staging, false).map_err(|e| e.to_string())?;
    directory
        .destination("fern-package.json")
        .map_err(|e| e.to_string())?;
    let mut temporary = Temporary::new(&directory).map_err(|e| e.to_string())?;
    let mut file = temporary.create("marker").map_err(|e| e.to_string())?;
    file.write_all(MARKER.as_bytes())
        .map_err(|e| e.to_string())?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644))
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    temporary
        .dir
        .rename("marker", &directory, "fern-package.json")
        .map_err(|e| e.to_string())
}
fn version(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let (base, metadata) = value
        .split_once('+')
        .map_or((value, None), |(base, meta)| (base, Some(meta)));
    let (core, pre) = base
        .split_once('-')
        .map_or((base, None), |(core, pre)| (core, Some(pre)));
    let core: Vec<_> = core.split('.').collect();
    let identifier = |v: &str| {
        !v.is_empty()
            && v.split('.').all(|piece| {
                !piece.is_empty()
                    && piece
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    };
    core.len() == 3
        && core.iter().all(|v| {
            !v.is_empty()
                && v.bytes().all(|b| b.is_ascii_digit())
                && (v.len() == 1 || !v.starts_with('0'))
        })
        && metadata.is_none_or(identifier)
        && pre.is_none_or(identifier)
}
fn stem(version_text: &str) -> Result<String, String> {
    if !version(version_text) {
        return Err("invalid semantic version".into());
    }
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        _ => return Err("unsupported release OS".into()),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => return Err("unsupported release architecture".into()),
    };
    Ok(format!("fern-{version_text}-{os}-{arch}"))
}
/// Create and verify one host bundle before atomically publishing each completed file.
pub fn package(
    root: &Path,
    staging: &Path,
    output: &Path,
    version: &str,
) -> Result<PathBuf, String> {
    let staging = if staging.is_absolute() {
        staging.to_path_buf()
    } else {
        root.join(staging)
    };
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        root.join(output)
    };
    let stem = stem(version)?;
    let mut files = inputs(&staging)?;
    let archive_name = format!("{stem}.tar.gz");
    let checksum_name = format!("{archive_name}.sha256");
    let directory =
        Dir::open(&output, true).map_err(|e| format!("invalid release destination: {e}"))?;
    for name in [&archive_name, &checksum_name] {
        directory.destination(name).map_err(|e| e.to_string())?;
    }
    let mut temporary = Temporary::new(&directory).map_err(|e| e.to_string())?;
    let mut archive_file = temporary.create(&archive_name).map_err(|e| e.to_string())?;
    archive::write(&mut archive_file, &stem, &mut files)?;
    archive_file.sync_all().map_err(|e| e.to_string())?;
    archive_file.rewind().map_err(|e| e.to_string())?;
    let digest = archive::hash(&mut archive_file)?;
    let text = format!("{digest}  {archive_name}\n");
    let mut checksum_file = temporary
        .create(&checksum_name)
        .map_err(|e| e.to_string())?;
    checksum_file
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    checksum_file.sync_all().map_err(|e| e.to_string())?;
    archive_file.rewind().map_err(|e| e.to_string())?;
    archive::verify_contents(archive_file)?;
    for name in [&archive_name, &checksum_name] {
        directory.destination(name).map_err(|e| e.to_string())?;
    }
    temporary
        .dir
        .rename(&archive_name, &directory, &archive_name)
        .map_err(|e| e.to_string())?;
    temporary
        .dir
        .rename(&checksum_name, &directory, &checksum_name)
        .map_err(|e| e.to_string())?;
    Ok(output.join(archive_name))
}
/// Check a bounded checksum and every archive member without extracting anything.
pub fn verify(archive: &Path, checksum: &Path) -> Result<(), String> {
    archive::verify(archive, checksum)
}
/// Install a complete validated stage. All destination types and all private copies
/// are checked before the first publication; each component replacement is atomic.
pub fn install(staging: &Path, prefix: &Path) -> Result<(), String> {
    let files = inputs(staging)?;
    let directory = Dir::open(prefix, true)
        .map_err(|e| format!("invalid install prefix {}: {e}", prefix.display()))?;
    let binary = directory
        .child(std::ffi::OsStr::new("bin"), true)
        .map_err(|e| format!("invalid install bin directory: {e}"))?;
    let share = directory
        .child(std::ffi::OsStr::new("share"), true)
        .and_then(|d| d.child(std::ffi::OsStr::new("fern"), true))
        .map_err(|e| format!("invalid install share/fern directory: {e}"))?;
    let location = |name: &str| {
        if ["LICENSE", "THIRD_PARTY_NOTICES.md", "README.md"].contains(&name) {
            &share
        } else {
            &binary
        }
    };
    for input in &files {
        location(input.name)
            .destination(input.name)
            .map_err(|e| e.to_string())?;
    }
    let mut temporary = Temporary::new(&directory).map_err(|e| e.to_string())?;
    for mut input in files {
        let mut file = temporary.create(input.name).map_err(|e| e.to_string())?;
        let copied = io::copy(
            &mut Read::by_ref(&mut input.file).take(input.size + 1),
            &mut file,
        )
        .map_err(|e| e.to_string())?;
        if copied != input.size {
            return Err(format!(
                "staging component changed while copying: {}",
                input.name
            ));
        }
        file.set_permissions(std::fs::Permissions::from_mode(input.mode))
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    for &name in REQUIRED.iter().chain(OPTIONAL) {
        if temporary
            .dir
            .metadata(name)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            location(name)
                .destination(name)
                .map_err(|e| e.to_string())?;
        }
    }
    for &name in REQUIRED.iter().chain(OPTIONAL) {
        if temporary
            .dir
            .metadata(name)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            temporary
                .dir
                .rename(name, location(name), name)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
/// Remove only known installed regular files, after preflighting every name.
/// Missing installations are a no-op; user files and containing directories stay.
pub fn uninstall(prefix: &Path) -> Result<(), String> {
    let existing = |result: io::Result<Dir>| match result {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    };
    let Some(directory) = existing(Dir::open(prefix, false))? else {
        return Ok(());
    };
    let binary = existing(directory.child(std::ffi::OsStr::new("bin"), false))?;
    let share = existing(
        directory
            .child(std::ffi::OsStr::new("share"), false)
            .and_then(|directory| directory.child(std::ffi::OsStr::new("fern"), false)),
    )?;
    let location = |name: &str| {
        if ["LICENSE", "THIRD_PARTY_NOTICES.md", "README.md"].contains(&name) {
            share.as_ref()
        } else {
            binary.as_ref()
        }
    };
    for &name in REQUIRED.iter().chain(OPTIONAL) {
        if let Some(directory) = location(name) {
            directory.destination(name).map_err(|e| e.to_string())?;
        }
    }
    for &name in REQUIRED.iter().chain(OPTIONAL) {
        if let Some(directory) = location(name) {
            match directory.unlink(name, false) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
