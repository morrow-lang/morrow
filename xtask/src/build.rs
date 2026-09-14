//! Cargo artifact discovery for the exact selected native components.
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, PartialEq, Eq)]
pub struct Artifacts {
    pub compiler: PathBuf,
    pub supervisor: PathBuf,
    pub runtime: PathBuf,
}

/// Read Cargo's machine-readable stream; artifact locations may use a custom target directory.
pub fn artifacts(messages: &str) -> Result<Artifacts, String> {
    let (mut compiler, mut supervisor, mut runtime) = (None, None, None);
    for line in messages.lines().filter(|line| !line.is_empty()) {
        let message: Value = serde_json::from_str(line)
            .map_err(|error| format!("invalid Cargo message: {error}"))?;
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        match message["target"]["name"].as_str() {
            Some("morrow")
                if message["target"]["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin")) =>
            {
                compiler = message["executable"].as_str().map(PathBuf::from);
            }
            Some("morrow-test-supervisor") => {
                supervisor = message["executable"].as_str().map(PathBuf::from)
            }
            Some("morrow_runtime_native") => {
                runtime = message["filenames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .find(|name| {
                        Path::new(name)
                            .file_name()
                            .is_some_and(|file| file == "libmorrow_runtime_native.a")
                    })
                    .map(PathBuf::from);
            }
            _ => {}
        }
    }
    Ok(Artifacts {
        compiler: compiler.ok_or("Cargo did not produce the Morrow compiler")?,
        supervisor: supervisor.ok_or("Cargo did not produce the Morrow test supervisor")?,
        runtime: runtime.ok_or("Cargo did not produce the Rust native entry archive")?,
    })
}

/// Cargo owns build concurrency and target selection. Only validated complete builds are staged.
pub fn build(root: &Path, release: bool) -> Result<PathBuf, String> {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root).args([
        "build",
        "--locked",
        "--message-format=json-render-diagnostics",
        "-p",
        "morrow",
        "-p",
        "morrow-runtime",
        "-p",
        "morrow-runtime-native",
        "-p",
        "morrow-test-supervisor",
    ]);
    if release {
        command.arg("--release");
    }
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("Cargo build failed: {}", output.status));
    }
    let selected =
        artifacts(std::str::from_utf8(&output.stdout).map_err(|error| error.to_string())?)?;
    let stage = crate::Temporary::new(root)?;
    for (source, name) in [
        (&selected.compiler, "morrow"),
        (&selected.supervisor, "morrow-test-supervisor"),
        (&selected.runtime, "libmorrow_runtime.a"),
    ] {
        fs::copy(source, stage.0.join(name))
            .map_err(|error| format!("cannot stage {name}: {error}"))?;
    }
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md", "README.md"] {
        fs::copy(root.join(name), stage.0.join(name)).map_err(|error| error.to_string())?;
    }
    crate::distribution::write_marker(&stage.0)?;
    crate::distribution::install(&stage.0, root)?;
    Ok(root.join("bin"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_exact_artifacts_and_never_selects_the_core_archive() {
        let messages = concat!(
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"morrow_runtime\",\"kind\":[\"rlib\",\"staticlib\"]},\"filenames\":[\"/custom/libmorrow_runtime.a\"]}\n",
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"morrow_runtime_native\",\"kind\":[\"staticlib\"]},\"filenames\":[\"/custom/libmorrow_runtime_native.a\"]}\n",
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"morrow\",\"kind\":[\"bin\"]},\"executable\":\"/custom/morrow\"}\n",
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"morrow-test-supervisor\",\"kind\":[\"bin\"]},\"executable\":\"/custom/morrow-test-supervisor\"}\n",
            "{\"reason\":\"build-finished\",\"success\":true}\n",
        );
        assert_eq!(
            artifacts(messages).unwrap(),
            Artifacts {
                compiler: "/custom/morrow".into(),
                supervisor: "/custom/morrow-test-supervisor".into(),
                runtime: "/custom/libmorrow_runtime_native.a".into(),
            }
        );
        assert!(artifacts(&messages.replace("morrow_runtime_native", "unrelated")).is_err());
        assert!(artifacts("invalid json").is_err());
    }
}
