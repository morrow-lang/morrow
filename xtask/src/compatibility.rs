//! Native boundary and rejection oracles independent of compiler implementation.
mod inventory;
use crate::acceptance::capture;
use morrow_test_supervisor::Captured;
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, PermissionsExt},
    },
    path::Path,
    process::Command,
};

/// Run dynamic native programs and atomic rejections in private working directories.
pub fn run(root: &Path, bin: &Path) -> Result<(), String> {
    let cases = inventory::native()?;
    for case in &cases {
        native(bin, case).map_err(|e| format!("{}: {e}", case["name"]))?;
    }
    let invalid = inventory::invalid()?;
    for case in &invalid {
        let work = crate::Temporary::new(&std::env::temp_dir())?;
        let source = prepare(&work.0, case)?;
        rejected(bin, &work.0, &source, case).map_err(|e| format!("{}: {e}", case["name"]))?;
    }
    let mut directory_cases = 0;
    for (group, count) in [
        ("aliases/invalid", 12),
        ("json_values/invalid", 12),
        ("labels_native", 8),
        ("namespaces_native", 12),
    ] {
        let mut paths = fs::read_dir(root.join("crates/morrow/tests").join(group))
            .map_err(|e| e.to_string())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        paths.retain(|path| {
            path.extension().is_some_and(|ext| ext == "mr")
                && (group.ends_with("invalid")
                    || path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("invalid_")))
        });
        paths.sort();
        if paths.len() != count {
            return Err(format!(
                "{group}: expected {count} negative fixtures, found {}",
                paths.len()
            ));
        }
        for source in paths {
            let work = crate::Temporary::new(&std::env::temp_dir())?;
            rejected(bin, &work.0, &source, &serde_json::json!({}))
                .map_err(|e| format!("{}: {e}", source.display()))?;
            directory_cases += 1;
        }
    }
    union_tests(bin)?;
    println!(
        "Compatibility passed: {} dynamic native programs, {} atomic rejections, union test continuation",
        cases.len(),
        invalid.len() + directory_cases
    );
    Ok(())
}

/// Materialize a trusted fixture with literal metacharacters in its source path.
fn prepare(directory: &Path, case: &Value) -> Result<std::path::PathBuf, String> {
    if let Some(files) = case["files"].as_object() {
        for (name, text) in files {
            if Path::new(name).components().count() != 1
                || !matches!(
                    Path::new(name).components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                return Err("invalid fixture sibling path".into());
            }
            fs::write(
                directory.join(name),
                text.as_str().ok_or("invalid fixture file")?,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    let source = directory.join("hello 'source' $literal.mr");
    fs::write(&source, case["source"].as_str().ok_or("missing source")?)
        .map_err(|e| e.to_string())?;
    Ok(source)
}
fn command(bin: &Path, directory: &Path, action: &str, source: &Path) -> Result<Captured, String> {
    let mut command = Command::new(bin.join("morrow"));
    command.arg(action).arg(source);
    capture(command, bin, directory)
}

/// Separate compilation allows raw non-UTF8 argv to reach generated code unchanged.
fn native(bin: &Path, case: &Value) -> Result<(), String> {
    let work = crate::Temporary::new(&std::env::temp_dir())?;
    let source = prepare(&work.0, case)?;
    fs::write(work.0.join("invalid-utf8.txt"), [0xc0, 0xaf]).map_err(|e| e.to_string())?;
    let executable = work.0.join("program 'output' $literal");
    let mut build = Command::new(bin.join("morrow"));
    build.arg("build").arg(&source).arg("-o").arg(&executable);
    let built = capture(build, bin, &work.0)?;
    if !built.status.success() {
        return Err(format!(
            "build failed: {}",
            String::from_utf8_lossy(&built.stderr)
        ));
    }
    let mut run = Command::new(executable);
    match case["name"].as_str() {
        Some("test_rust_boundaries/split_invalid_bytes") => {
            run.arg(OsString::from_vec(vec![0xc0, 0xaf]));
        }
        Some("test_rust_stdlib/arguments") => {
            run.args(["literal ; $ value", "🌿"]);
        }
        _ => {}
    }
    let result = capture(run, bin, &work.0)?;
    if result.status.code().map(i64::from) != case["exit"].as_i64()
        || result.stdout != case["stdout"].as_str().ok_or("missing stdout")?.as_bytes()
        || result.stderr != case["stderr"].as_str().ok_or("missing stderr")?.as_bytes()
    {
        return Err(format!(
            "unexpected {} stdout={:?} stderr={:?}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(())
}
const SENTINEL: &[u8] = b"existing executable\0must survive failed checking\xff";

/// Rejection preserves prior executable bytes, inode, mode and modification time.
fn rejected(bin: &Path, directory: &Path, source: &Path, case: &Value) -> Result<(), String> {
    if let Some(action) = case["preflight"].as_str() {
        let parsed = command(bin, directory, action, source)?;
        if !parsed.status.success() {
            return Err(format!(
                "negative fixture must {action}: {}",
                String::from_utf8_lossy(&parsed.stderr)
            ));
        }
    }
    let output = directory.join("preserved executable");
    let absent = case["no_artifact"].as_bool() == Some(true);
    let before = if absent {
        None
    } else {
        fs::write(&output, SENTINEL).map_err(|e| e.to_string())?;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o751))
            .map_err(|e| e.to_string())?;
        Some(fs::metadata(&output).map_err(|e| e.to_string())?)
    };
    let mut build = Command::new(bin.join("morrow"));
    build.arg("build").arg(source).arg("-o").arg(&output);
    let actual = capture(build, bin, directory)?;
    let diagnostic = String::from_utf8_lossy(&actual.stderr);
    let expected = case["diagnostic"].as_str().unwrap_or("error:");
    let forbidden = case["absent_diagnostic"].as_str().unwrap_or("");
    if actual.status.code() != Some(1)
        || !actual.stdout.is_empty()
        || diagnostic.contains("panicked")
        || !diagnostic.to_lowercase().contains(&expected.to_lowercase())
        || (!forbidden.is_empty() && diagnostic.contains(forbidden))
    {
        return Err(format!(
            "invalid program must diagnose {expected:?}: {} {diagnostic}",
            actual.status
        ));
    }
    unchanged(&output, before.as_ref())
}
fn unchanged(output: &Path, before: Option<&fs::Metadata>) -> Result<(), String> {
    match before {
        None if !output.try_exists().map_err(|e| e.to_string())? => Ok(()),
        Some(before) => {
            let after = fs::metadata(output).map_err(|e| e.to_string())?;
            if fs::read(output).map_err(|e| e.to_string())? == SENTINEL
                && (
                    before.ino(),
                    before.mode(),
                    before.mtime(),
                    before.mtime_nsec(),
                ) == (after.ino(), after.mode(), after.mtime(), after.mtime_nsec())
            {
                Ok(())
            } else {
                Err("failed build modified existing executable".into())
            }
        }
        None => Err("failed build published an executable".into()),
    }
}
fn union_tests(bin: &Path) -> Result<(), String> {
    let work = crate::Temporary::new(&std::env::temp_dir())?;
    let path = work.0.join("unit_library.mr");
    fs::write(
        &path,
        include_str!("../../tests/fixtures/union-test-continuation.mr"),
    )
    .map_err(|e| e.to_string())?;
    let actual = command(bin, &work.0, "test", &path)?;
    let diagnostic = String::from_utf8_lossy(&actual.stderr);
    if actual.status.code() != Some(1)
        || !String::from_utf8_lossy(&actual.stdout).contains("2/4 passed")
        || ![
            "System.exit cannot terminate a test",
            "test_failed",
            "test_exit",
        ]
        .iter()
        .all(|part| diagnostic.contains(part))
        || fs::read(work.0.join("events")).map_err(|e| e.to_string())? != b"after"
    {
        return Err(format!("union test continuation failed: {actual:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_checker_detects_bytes_modes_and_created_output() {
        let work = crate::Temporary::new(&std::env::temp_dir()).unwrap();
        let path = work.0.join("output");
        assert!(unchanged(&path, None).is_ok());
        fs::write(&path, SENTINEL).unwrap();
        let before = fs::metadata(&path).unwrap();
        assert!(unchanged(&path, Some(&before)).is_ok());
        assert!(unchanged(&path, None).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(unchanged(&path, Some(&before)).is_err());
        fs::set_permissions(&path, before.permissions()).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert!(unchanged(&path, Some(&before)).is_err());
    }
    #[test]
    fn fixture_siblings_cannot_escape_private_workspace() {
        let work = crate::Temporary::new(&std::env::temp_dir()).unwrap();
        let bad = serde_json::json!({"source":"fn main():()","files":{"../escaped.mr":""}});
        assert!(prepare(&work.0, &bad).is_err());
        let good =
            serde_json::json!({"source":"fn main():()","files":{"model.mr":"pub fn value():1"}});
        assert!(prepare(&work.0, &good).is_ok());
        assert_eq!(
            fs::read_to_string(work.0.join("model.mr")).unwrap(),
            "pub fn value():1"
        );
    }
}
