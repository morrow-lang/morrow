//! Deterministic grammar and mutation acceptance with bounded subprocesses.
mod generator;
mod language;
use std::{fs, path::Path, process::Command};
const SEEDS: [&str; 7] = [
    "basic",
    "call_chain",
    "collections",
    "operators",
    "if_chain",
    "match_with",
    "typed_signature",
];

/// Run the original grammar corpus and fixed mutation acceptance through bounded helpers.
/// The first seven iterations use retained seed files, matching the original runner.
pub fn run(root: &Path, bin: &Path, iterations: u32, seed: u64) -> Result<(), String> {
    let temporary = crate::Temporary::new(&std::env::temp_dir())?;
    let path = temporary.0.join("case.fn");
    let mut invoke = |action: &str, path: &Path| {
        let mut command = Command::new(bin.join("morrow"));
        command.arg(action).arg(path);
        let output = crate::acceptance::capture(command, bin, root)?;
        if !matches!(output.status.code(), Some(0 | 1))
            || output.stderr.windows(8).any(|part| part == b"panicked")
        {
            return Err(format!(
                "{action} failed abnormally: {} {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output)
    };
    for index in 0..iterations {
        let source = if (index as usize) < SEEDS.len() {
            fs::read_to_string(root.join(format!("fuzz/corpus/{}.fn", SEEDS[index as usize])))
                .map_err(|error| error.to_string())?
        } else {
            generator::generate(seed, index)
        };
        fs::write(&path, &source).map_err(|error| error.to_string())?;
        grammar(&path, &mut invoke).map_err(|error| {
            format!("grammar seed={seed:#x} index={index}: {error}\nsource={source:?}")
        })?;
        if (index + 1).is_multiple_of(64) {
            eprintln!("Fuzz grammar: {}/{iterations}", index + 1);
        }
    }
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/fuzz-mutations.json"))
            .map_err(|error| error.to_string())?;
    let mutation_seed = fixture["seed"]
        .as_u64()
        .ok_or("mutation fixture has no seed")?;
    let sources = fixture["sources"]
        .as_array()
        .filter(|sources| sources.len() == 192)
        .ok_or("mutation fixture must contain 192 sources")?;
    for (index, source) in sources.iter().enumerate() {
        let source = source.as_str().ok_or("mutation source must be text")?;
        fs::write(&path, source).map_err(|error| error.to_string())?;
        mutation(source, &path, &mut invoke).map_err(|error| {
            format!("mutation seed={mutation_seed:#x} index={index}: {error}\nsource={source:?}")
        })?;
    }
    let feature_cases = language::run(&path, seed, &mut invoke)?;
    println!(
        "Fuzz passed: {iterations} grammar cases seed={seed:#x}, 192 mutations seed={mutation_seed:#x}, {feature_cases} language-feature mutations"
    );
    Ok(())
}
type Invoke<'a> = dyn FnMut(&str, &Path) -> Result<morrow_test_supervisor::Captured, String> + 'a;
/// A successful formatting pass must be stable and remain parseable.
fn grammar(path: &Path, invoke: &mut Invoke<'_>) -> Result<(), String> {
    for action in ["parse", "fmt"] {
        require_success(action, invoke(action, path)?)?;
    }
    let canonical = fs::read(path).map_err(|error| error.to_string())?;
    require_success("second fmt", invoke("fmt", path)?)?;
    if fs::read(path).map_err(|error| error.to_string())? != canonical {
        return Err("formatter is not idempotent".into());
    }
    require_success("second parse", invoke("parse", path)?)
}
fn require_success(action: &str, output: morrow_test_supervisor::Captured) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{action}: {} {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
/// Invalid sources may fail cleanly; accepted check/format operations preserve lowering.
fn mutation(source: &str, path: &Path, invoke: &mut Invoke<'_>) -> Result<(), String> {
    let checked = invoke("check", path)?;
    let emitted = invoke("emit", path)?;
    if checked.status.success() && !emitted.status.success() {
        return Err("checked program cannot lower".into());
    }
    let formatted = invoke("fmt", path)?;
    if !formatted.status.success() {
        if fs::read(path).map_err(|error| error.to_string())? != source.as_bytes() {
            return Err("failed formatter modified source".into());
        }
    } else {
        let canonical = fs::read(path).map_err(|error| error.to_string())?;
        require_success("second fmt", invoke("fmt", path)?)?;
        if fs::read(path).map_err(|error| error.to_string())? != canonical {
            return Err("formatter is not idempotent".into());
        }
        let again = invoke("emit", path)?;
        if again.status.code() != emitted.status.code()
            || (again.status.success() && again.stdout != emitted.stdout)
        {
            return Err("formatting changed lowering behavior".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::process::ExitStatusExt, process::ExitStatus};
    fn output(code: i32, bytes: &[u8]) -> morrow_test_supervisor::Captured {
        morrow_test_supervisor::Captured {
            status: ExitStatus::from_raw(code << 8),
            stdout: bytes.to_vec(),
            stderr: vec![],
        }
    }
    #[test]
    fn mutations_reject_failed_formatter_writes_and_checked_lowering_failure() {
        let temporary = crate::Temporary::new(&std::env::temp_dir()).unwrap();
        let path = temporary.0.join("case.fn");
        fs::write(&path, "source").unwrap();
        assert!(
            mutation("source", &path, &mut |action, _| Ok(output(
                if action == "emit" { 1 } else { 0 },
                b""
            )))
            .is_err()
        );
        assert!(
            mutation("source", &path, &mut |action, path| {
                if action == "fmt" {
                    fs::write(path, "changed").unwrap();
                }
                Ok(output(1, b""))
            })
            .is_err()
        );
    }
    #[test]
    fn mutations_reject_changed_lowering_and_non_idempotent_formatting() {
        let temporary = crate::Temporary::new(&std::env::temp_dir()).unwrap();
        let path = temporary.0.join("case.fn");
        fs::write(&path, "source").unwrap();
        let mut emissions = 0;
        assert!(
            mutation("source", &path, &mut |action, _| {
                if action == "emit" {
                    emissions += 1;
                }
                Ok(output(0, if emissions == 2 { b"new" } else { b"old" }))
            })
            .is_err()
        );
        let mut formats = 0;
        assert!(
            mutation("source", &path, &mut |action, path| {
                if action == "fmt" {
                    formats += 1;
                    fs::write(path, formats.to_string()).unwrap();
                }
                Ok(output(0, b""))
            })
            .is_err()
        );
    }
}
