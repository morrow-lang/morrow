//! Native object linking and QBE process adaptation share private artifact ownership.
mod linker_flags;
mod package_path;
use linker_flags::parse as parse_linker_flags;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::{
    env,
    ffi::OsString,
    fs, io,
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
    let development = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../bin")
        .join(filename);
    component_at(directory, &development, filename, variable)
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
        "missing {filename}; run `mise run rust-build` or set {variable}"
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

/// Read pkg-config output as linker arguments without invoking a shell.
fn package_flags(packages: &[&str]) -> Result<Vec<OsString>, String> {
    let output = Command::new("pkg-config")
        .arg("--libs")
        .args(packages)
        .output();
    if let Ok(output) = output {
        if output.status.success() {
            let flags = std::str::from_utf8(&output.stdout)
                .map_err(|_| "pkg-config returned non-UTF-8 linker flags")?;
            return parse_linker_flags(flags);
        }
    }
    Ok(packages
        .iter()
        .flat_map(|package| match *package {
            "bdw-gc" => vec!["-lgc"],
            "sqlite3" => vec!["-lsqlite3"],
            "openssl" => vec!["-lssl", "-lcrypto"],
            _ => Vec::new(),
        })
        .map(OsString::from)
        .collect())
}

/// Match the reference compiler's static GC linkage when the archive is available.
fn gc_flags() -> Result<Vec<OsString>, String> {
    if let Ok(output) = Command::new("pkg-config")
        .args(["--variable=libdir", "bdw-gc"])
        .output()
    {
        if output.status.success() {
            let directory = package_path::parse(&output.stdout)?;
            let archive = directory.join("libgc.a");
            if archive.is_file() {
                return Ok(vec![archive.into_os_string()]);
            }
        }
    }
    package_flags(&["bdw-gc"])
}

/// Compile checked QBE in a private workspace and retain the resulting executable there.
pub fn compile(il: &str, workspace: &Workspace) -> Result<PathBuf, String> {
    let backend = component("FERN_QBE", "fern-qbe")?;
    let runtime = runtime_archive()?;
    let source = workspace.file("program.ssa");
    let assembly = workspace.file("program.s");
    let object = workspace.file("program.o");
    fs::write(&source, il).map_err(|e| e.to_string())?;
    execute(Command::new(backend).arg(&source).arg(&assembly), "QBE")?;
    let compiler = env::var_os("CC").unwrap_or_else(|| "cc".into());
    execute(
        Command::new(compiler)
            .arg("-c")
            .arg(&assembly)
            .arg("-o")
            .arg(&object),
        "assembly",
    )?;
    link_object(&object, &runtime, workspace)
}

/// Accept backend-produced object bytes without resolving QBE or invoking an assembler.
#[cfg(feature = "cranelift")]
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
            .args(gc_flags()?)
            .args(package_flags(&["sqlite3", "openssl"])?)
            .arg("-pthread")
            .arg("-lm")
            .arg("-o")
            .arg(&executable),
        "link",
    )?;
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::parse_linker_flags;
    use std::ffi::OsString;

    #[test]
    fn linker_paths_preserve_non_ascii_whitespace() {
        assert_eq!(
            parse_linker_flags("-L/opt/a\u{a0}b -lssl").unwrap(),
            vec![OsString::from("-L/opt/a\u{a0}b"), OsString::from("-lssl")]
        );
    }

    #[test]
    fn linker_boundaries_reject_nul_and_aggregate_excess_before_publication() {
        for flags in [
            "-Lgood \0bad".to_string(),
            "x".repeat(65537),
            "x".repeat(16385),
            "'' ".repeat(4097),
        ] {
            assert!(
                parse_linker_flags(&flags).is_err(),
                "accepted malformed/oversized argv"
            );
        }
    }

    #[test]
    fn linker_exact_limits_are_valid() {
        assert_eq!(parse_linker_flags(&"x".repeat(16384)).unwrap().len(), 1);
        assert_eq!(parse_linker_flags(&"'' ".repeat(4096)).unwrap().len(), 4096);
        let flags = format!("{} ", "x".repeat(16383)).repeat(4);
        assert_eq!(flags.len(), 65536);
        assert_eq!(parse_linker_flags(&flags).unwrap().len(), 4);
    }

    #[test]
    fn linker_word_limits_count_utf8_bytes_and_discard_late_failure() {
        let exact = format!("{}λ", "x".repeat(16382));
        assert_eq!(
            parse_linker_flags(&exact).unwrap(),
            vec![OsString::from(&exact)]
        );
        assert!(parse_linker_flags(&format!("ok {}λ", "x".repeat(16383))).is_err());
        assert!(parse_linker_flags("ok 'unfinished").is_err());
    }

    #[test]
    fn linker_single_quoted_literal_roundtrips() {
        let alphabet = [
            'a', ' ', '\t', '\n', '\r', '\u{a0}', 'λ', '\\', '\'', '"', '$', '`', '*', ';',
        ];
        for offset in 0..alphabet.len() {
            let word: String = alphabet.iter().cycle().skip(offset).take(32).collect();
            let quoted = format!("'{}'", word.replace('\'', "'\"'\"'"));
            assert_eq!(
                parse_linker_flags(&quoted).unwrap(),
                vec![OsString::from(word)]
            );
        }
    }

    #[test]
    fn linker_words_preserve_escaped_quoted_and_literal_shell_characters() {
        let flags = parse_linker_flags(
            r#"-L/a\ b '-L/single quoted' "-L/double quoted" -lssl -Wl,$ORIGIN \$HOME \`literal\`"#,
        )
        .unwrap();
        let expected: Vec<OsString> = [
            "-L/a b",
            "-L/single quoted",
            "-L/double quoted",
            "-lssl",
            "-Wl,$ORIGIN",
            "$HOME",
            "`literal`",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        assert_eq!(flags, expected);
        assert_eq!(
            parse_linker_flags("  -framework\tSecurity\n").unwrap(),
            vec![OsString::from("-framework"), OsString::from("Security")]
        );
    }

    #[test]
    fn linker_words_handle_quote_adjacency_and_shell_double_quote_backslashes() {
        assert_eq!(
            parse_linker_flags(r#"-L"a b"/c '' "a\qb" "a\\b" "a\"b""#).unwrap(),
            vec![
                OsString::from("-La b/c"),
                OsString::from(""),
                OsString::from(r"a\qb"),
                OsString::from(r"a\b"),
                OsString::from("a\"b")
            ]
        );
    }

    #[test]
    fn linker_words_reject_truncated_quoting_and_escapes() {
        for malformed in ["-L'bad", "-L\"bad", "-Lbad\\", "\"bad\\"] {
            let error = parse_linker_flags(malformed).unwrap_err();
            assert!(error.contains("pkg-config"), "{error}");
        }
    }
}

pub mod capture;

#[cfg(test)]
mod preview_tests {
    use super::{component_at, Workspace};
    use std::fs;

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
                ("fern-qbe", "FERN_QBE"),
                ("fern-test-supervisor", "FERN_TEST_SUPERVISOR"),
                ("libfern_runtime.a", "FERN_RUNTIME_LIB"),
            ] {
                let error = component_at(&package, &checkout, name, variable).unwrap_err();
                assert!(error.contains("installed package"), "{error}");
                assert!(error.contains(name) && error.contains(variable), "{error}");
            }
        }
        let sibling = package.join("fern-qbe");
        fs::write(&sibling, b"installed helper").unwrap();
        assert_eq!(
            component_at(&package, &checkout, "fern-qbe", "FERN_QBE").unwrap(),
            sibling
        );
    }

    #[test]
    fn preview_marker_blocks_existing_checkout_fallback() {
        let workspace = Workspace::new(&std::env::temp_dir()).unwrap();
        let package = workspace.file("package");
        let checkout = workspace.file("development-qbe");
        fs::create_dir(&package).unwrap();
        fs::write(&checkout, b"valid development fixture").unwrap();
        assert_eq!(
            component_at(&package, &checkout, "fern-qbe", "FERN_QBE").unwrap(),
            checkout
        );
        let marker = package.join("fern-rust-preview.json");
        for bytes in [b"".as_slice(), b"invalid json", b"{}"] {
            fs::write(&marker, bytes).unwrap();
            let error = component_at(&package, &checkout, "fern-qbe", "FERN_QBE").unwrap_err();
            assert!(error.contains("preview package"), "{error}");
        }
        fs::remove_file(&marker).unwrap();
        fs::create_dir(&marker).unwrap();
        assert!(component_at(&package, &checkout, "fern-qbe", "FERN_QBE").is_err());
        fs::remove_dir(&marker).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("absent", &marker).unwrap();
            assert!(component_at(&package, &checkout, "fern-qbe", "FERN_QBE").is_err());
        }
        let sibling = package.join("fern-qbe");
        fs::write(&sibling, b"package helper").unwrap();
        assert_eq!(
            component_at(&package, &checkout, "fern-qbe", "FERN_QBE").unwrap(),
            sibling
        );
    }
}
