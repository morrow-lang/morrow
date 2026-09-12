//! Native object linking preserves private artifact ownership.
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

/// A private directory owns all compilation artifacts and cleans up on every exit.
pub struct Workspace {
    path: PathBuf,
}

impl Workspace {
    /// Allocate an exclusive directory with restrictive Unix permissions.
    pub fn new(parent: &Path) -> io::Result<Self> {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        for attempt in 0..100 {
            let path = parent.join(format!(".fern-rs-{}-{epoch}-{attempt}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot allocate compilation workspace",
        ))
    }

    /// Locate a compiler-owned file inside this workspace.
    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Resolve a backend component beside the binary or in the development checkout.
fn component(variable: &str, filename: &str) -> Result<PathBuf, String> {
    if let Some(path) = env::var_os(variable) {
        return Ok(PathBuf::from(path));
    }
    let executable = env::current_exe().map_err(|e| e.to_string())?;
    let directory = executable
        .parent()
        .ok_or("compiler has no parent directory")?;
    // Cargo emits both the core archive (without startup) and the native entry
    // archive. A Cargo compiler must never accidentally select the core archive.
    let filename = runtime_component_name(directory, filename);
    let development = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bin")
        .join(filename);
    component_at(directory, &development, filename, variable)
}

/// Cargo profile directories use the dedicated startup archive; installed layouts
/// retain their published name and never fall back to an unrelated core archive.
fn runtime_component_name<'a>(directory: &Path, filename: &'a str) -> &'a str {
    let cargo_profile = directory
        .file_name()
        .is_some_and(|name| name == "debug" || name == "release");
    let packaged = ["fern-package.json", "fern-rust-preview.json"]
        .iter()
        .any(|name| fs::symlink_metadata(directory.join(name)).is_ok());
    if filename == "libfern_runtime.a" && cargo_profile && !packaged {
        "libfern_runtime_native.a"
    } else {
        filename
    }
}

/// Package markers make sibling selection closed even when their contents are damaged.
fn component_at(
    directory: &Path,
    development: &Path,
    filename: &str,
    variable: &str,
) -> Result<PathBuf, String> {
    let candidate = directory.join(filename);
    if candidate.is_file() {
        return Ok(candidate);
    }
    for (marker, label) in [
        ("fern-package.json", "installed package"),
        ("fern-rust-preview.json", "Rust preview package"),
    ] {
        match fs::symlink_metadata(directory.join(marker)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => {
                return Err(format!(
                    "missing {filename} in {label}; restore the sibling component or set {variable}"
                ));
            }
        }
    }
    if development.is_file() {
        return Ok(development.to_path_buf());
    }
    Err(format!(
        "missing {filename}; run `cargo xtask build` or set {variable}"
    ))
}

/// Run a native tool using literal argument vectors, preserving failure diagnostics.
fn execute(command: &mut Command, stage: &str) -> Result<Output, String> {
    let output = command
        .output()
        .map_err(|error| format!("{stage}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{stage} failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output)
}

/// Accept backend-produced object bytes without resolving QBE or invoking an assembler.
pub fn compile_object(bytes: &[u8], workspace: &Workspace) -> Result<PathBuf, String> {
    let runtime = runtime_archive()?;
    let object = workspace.file("program.o");
    fs::write(&object, bytes).map_err(|error| format!("cannot write object: {error}"))?;
    link_object(&object, &runtime, workspace)
}

/// Resolve and validate the common runtime before executing native tools.
fn runtime_archive() -> Result<PathBuf, String> {
    let runtime = component("FERN_RUNTIME_LIB", "libfern_runtime.a")?;
    if !runtime.is_file() {
        return Err(format!(
            "runtime archive does not exist: {}",
            runtime.display()
        ));
    }
    Ok(runtime)
}

/// Link one compiler-owned object into the same workspace for atomic caller publication.
fn link_object(object: &Path, runtime: &Path, workspace: &Workspace) -> Result<PathBuf, String> {
    let executable = workspace.file("program");
    let compiler = env::var_os("CC").unwrap_or_else(|| "cc".into());
    execute(
        Command::new(compiler)
            .arg(object)
            .arg(runtime)
            .args(system_libraries())
            .arg("-o")
            .arg(&executable),
        "link",
    )?;
    Ok(executable)
}

/// Libraries required by Rust's native staticlib on the supported Unix hosts.
fn system_libraries() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["-Wl,-dead_strip", "-liconv", "-lSystem"]
    }
    #[cfg(target_os = "linux")]
    {
        &[
            "-Wl,--gc-sections",
            "-ldl",
            "-lpthread",
            "-lm",
            "-lrt",
            "-lutil",
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        &[]
    }
}

pub mod capture;

#[cfg(test)]
mod preview_tests {
    use super::{Workspace, component_at};
    use std::fs;

    #[test]
    fn cargo_runtime_selection_never_uses_the_core_archive_as_startup() {
        let workspace = Workspace::new(&std::env::temp_dir()).unwrap();
        let profile = workspace.file("debug");
        fs::create_dir(&profile).unwrap();
        fs::write(profile.join("libfern_runtime.a"), b"core only").unwrap();
        assert_eq!(
            super::runtime_component_name(&profile, "libfern_runtime.a"),
            "libfern_runtime_native.a"
        );
        fs::write(profile.join("fern-package.json"), b"{}").unwrap();
        assert_eq!(
            super::runtime_component_name(&profile, "libfern_runtime.a"),
            "libfern_runtime.a"
        );
        assert_eq!(
            super::runtime_component_name(&profile, "fern-test-supervisor"),
            "fern-test-supervisor"
        );
    }

    #[test]
    fn installed_marker_blocks_development_fallback_for_every_helper() {
        let workspace = Workspace::new(&std::env::temp_dir()).unwrap();
        let package = workspace.file("installed");
        fs::create_dir(&package).unwrap();
        let checkout = workspace.file("development-component");
        fs::write(&checkout, b"valid development fixture").unwrap();
        for marker in [b"".as_slice(), b"invalid json", b"{}"] {
            fs::write(package.join("fern-package.json"), marker).unwrap();
            for (name, variable) in [
                ("fern-test-supervisor", "FERN_TEST_SUPERVISOR"),
                ("libfern_runtime.a", "FERN_RUNTIME_LIB"),
            ] {
                let error = component_at(&package, &checkout, name, variable).unwrap_err();
                assert!(error.contains("installed package"), "{error}");
                assert!(error.contains(name) && error.contains(variable), "{error}");
            }
        }
        let sibling = package.join("fern-test-supervisor");
        fs::write(&sibling, b"installed helper").unwrap();
        assert_eq!(
            component_at(
                &package,
                &checkout,
                "fern-test-supervisor",
                "FERN_TEST_SUPERVISOR"
            )
            .unwrap(),
            sibling
        );
    }

    #[test]
    fn preview_marker_blocks_existing_checkout_fallback() {
        let workspace = Workspace::new(&std::env::temp_dir()).unwrap();
        let package = workspace.file("package");
        let checkout = workspace.file("development-supervisor");
        fs::create_dir(&package).unwrap();
        fs::write(&checkout, b"valid development fixture").unwrap();
        assert_eq!(
            component_at(
                &package,
                &checkout,
                "fern-test-supervisor",
                "FERN_TEST_SUPERVISOR"
            )
            .unwrap(),
            checkout
        );
        let marker = package.join("fern-rust-preview.json");
        for bytes in [b"".as_slice(), b"invalid json", b"{}"] {
            fs::write(&marker, bytes).unwrap();
            let error = component_at(
                &package,
                &checkout,
                "fern-test-supervisor",
                "FERN_TEST_SUPERVISOR",
            )
            .unwrap_err();
            assert!(error.contains("preview package"), "{error}");
        }
        fs::remove_file(&marker).unwrap();
        fs::create_dir(&marker).unwrap();
        assert!(
            component_at(
                &package,
                &checkout,
                "fern-test-supervisor",
                "FERN_TEST_SUPERVISOR"
            )
            .is_err()
        );
        fs::remove_dir(&marker).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("absent", &marker).unwrap();
            assert!(
                component_at(
                    &package,
                    &checkout,
                    "fern-test-supervisor",
                    "FERN_TEST_SUPERVISOR"
                )
                .is_err()
            );
        }
        let sibling = package.join("fern-test-supervisor");
        fs::write(&sibling, b"package helper").unwrap();
        assert_eq!(
            component_at(
                &package,
                &checkout,
                "fern-test-supervisor",
                "FERN_TEST_SUPERVISOR"
            )
            .unwrap(),
            sibling
        );
    }
}
