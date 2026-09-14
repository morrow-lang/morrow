//! Independent native behavior fixtures retained across implementation migrations.
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
    process::Command,
    time::Duration,
};
#[derive(Debug)]
pub struct Case {
    pub file: String,
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
}
/// Validate fixture shape, unique relative paths and complete expected process results.
pub fn cases(source: &str) -> Result<Vec<Case>, String> {
    let value: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
    let entries = value
        .as_array()
        .ok_or("native fixture inventory must be an array")?;
    let mut names = BTreeSet::new();
    entries
        .iter()
        .map(|entry| {
            let text = |key| {
                entry[key]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("missing fixture {key}"))
            };
            let file = text("file")?;
            if Path::new(&file)
                .extension()
                .is_none_or(|extension| extension != "fn")
                || Path::new(&file)
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
                || !names.insert(file.clone())
            {
                return Err(format!("invalid or duplicate fixture path: {file}"));
            }
            let exit = entry["exit"]
                .as_i64()
                .filter(|code| (0..=255).contains(code))
                .ok_or("invalid fixture exit code")? as i32;
            Ok(Case {
                file,
                exit,
                stdout: text("stdout")?,
                stderr: text("stderr")?,
            })
        })
        .collect()
}

/// Run through the shared bounded supervisor with a literal working directory.
pub fn capture(
    command: Command,
    bin: &Path,
    cwd: &Path,
) -> Result<morrow_test_supervisor::Captured, String> {
    let mut supervisor = Command::new(bin.join("morrow-test-supervisor"));
    supervisor.current_dir(cwd);
    morrow_test_supervisor::capture(command, supervisor, Duration::from_secs(60))
}

/// Compile and execute independent expected-output cases with no old implementation dependency.
pub fn native(root: &Path, bin: &Path, filter: Option<&str>) -> Result<(), String> {
    let inventory = cases(include_str!("../../crates/morrow/tests/native-cases.json"))?;
    let workspace = crate::Temporary::new(&std::env::temp_dir())?;
    let mut failures = Vec::new();
    let mut count = 0;
    for case in inventory
        .iter()
        .filter(|case| filter.is_none_or(|filter| case.file.contains(filter)))
    {
        count += 1;
        let outcome = (|| {
            let executable = workspace.0.join("program");
            let mut compiler = Command::new(bin.join("morrow"));
            compiler
                .arg("build")
                .arg(root.join("crates/morrow/tests").join(&case.file))
                .arg("-o")
                .arg(&executable);
            let built = capture(compiler, bin, root)?;
            if !built.status.success() {
                return Err(format!(
                    "compile failed: {}",
                    String::from_utf8_lossy(&built.stderr)
                ));
            }
            let result = capture(Command::new(executable), bin, root)?;
            if result.status.code() != Some(case.exit)
                || result.stdout != case.stdout.as_bytes()
                || result.stderr != case.stderr.as_bytes()
            {
                return Err(format!(
                    "expected exit={} stdout={:?} stderr={:?}; got {} stdout={:?} stderr={:?}",
                    case.exit,
                    case.stdout,
                    case.stderr,
                    result.status,
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                ));
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            eprintln!("FAIL {}: {error}", case.file);
            failures.push(case.file.clone());
        }
        if count % 25 == 0 {
            eprintln!(
                "Native fixtures: {count} checked, {} failed",
                failures.len()
            );
        }
    }
    if count == 0 {
        return Err("no native fixtures matched".into());
    }
    if failures.is_empty() {
        println!("{count} native-output fixtures passed");
        Ok(())
    } else {
        Err(format!(
            "{} of {count} native fixtures failed: {}",
            failures.len(),
            failures.join(", ")
        ))
    }
}

/// Typecheck every public example through the installed compiler path.
pub fn examples(root: &Path, bin: &Path) -> Result<(), String> {
    let mut paths = fs::read_dir(root.join("examples"))
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    paths.sort();
    let mut count = 0;
    for path in paths
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "fn"))
    {
        let mut command = Command::new(bin.join("morrow"));
        command.arg("check").arg(path);
        let result = capture(command, bin, root)?;
        if !result.status.success() {
            return Err(format!(
                "example {}: {}",
                path.display(),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        count += 1;
    }
    if count == 0 {
        return Err("no examples found".into());
    }
    println!("{count} examples passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_all_native_oracles_and_rejects_escaping_paths() {
        let actual = cases(include_str!("../../crates/morrow/tests/native-cases.json")).unwrap();
        assert_eq!(actual.len(), 316);
        let fault = actual
            .iter()
            .find(|case| case.file == "actors/bad_timeout.fn")
            .unwrap();
        assert_eq!(fault.exit, 1);
        assert!(fault.stdout.is_empty());
        assert!(fault.stderr.contains("600000"));
        for path in [
            "../outside.fn",
            "/outside.fn",
            "x/../../outside.fn",
            "same.fn",
        ] {
            let input = serde_json::json!([
                {"file":path,"exit":0,"stdout":"","stderr":""},
                {"file":path,"exit":0,"stdout":"","stderr":""}
            ]);
            assert!(cases(&input.to_string()).is_err(), "{path}");
        }
    }
}
