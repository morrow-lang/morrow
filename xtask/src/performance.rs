//! Explicit local release measurements; correctness gates do not depend on noisy timing.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command, time::Instant};

fn summary(samples: &[f64]) -> Result<Value, String> {
    if samples.is_empty()
        || samples
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err("timings must be nonempty, finite and nonnegative".into());
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    };
    Ok(
        json!({"samples":sorted.len(),"median_ms":median,"p95_ms":sorted[(sorted.len()*95).div_ceil(100)-1]}),
    )
}

/// Measure explicitly staged components and simple native programs with independent outputs.
pub fn run(root: &Path, bin: &Path, output: &Path) -> Result<(), String> {
    let mut components = serde_json::Map::new();
    for name in ["morrow", "morrow-test-supervisor", "libmorrow_runtime.a"] {
        let bytes = fs::read(bin.join(name)).map_err(|error| error.to_string())?;
        components.insert(
            name.into(),
            json!({"bytes":bytes.len(),"sha256":Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}),
        );
    }
    let compiler = bin.join("morrow");
    let mut startup = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        let result = Command::new(&compiler)
            .arg("--version")
            .output()
            .map_err(|error| error.to_string())?;
        startup.push(started.elapsed().as_secs_f64() * 1000.0);
        if !result.status.success()
            || result.stdout != format!("morrow {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
            || !result.stderr.is_empty()
        {
            return Err("compiler version probe failed".into());
        }
    }
    let temporary = crate::Temporary::new(&std::env::temp_dir())?;
    let mut programs = Vec::new();
    for (name, expected, code) in [
        ("fib", "9227465\n", 0),
        ("sum", "50005000\n", 0),
        ("startup", "", 42),
    ] {
        let executable = temporary.0.join(name);
        let started = Instant::now();
        let result = Command::new(&compiler)
            .arg("build")
            .arg(root.join(format!("benchmarks/{name}_morrow.mr")))
            .arg("-o")
            .arg(&executable)
            .output()
            .map_err(|error| error.to_string())?;
        let compile_ms = started.elapsed().as_secs_f64() * 1000.0;
        if !result.status.success() {
            return Err(format!(
                "benchmark build: {}",
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        let mut samples = Vec::new();
        for _ in 0..10 {
            let started = Instant::now();
            let result = Command::new(&executable)
                .output()
                .map_err(|error| error.to_string())?;
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            if result.status.code() != Some(code)
                || result.stdout != expected.as_bytes()
                || !result.stderr.is_empty()
            {
                return Err(format!(
                    "benchmark {name} produced incorrect output: {result:?}"
                ));
            }
        }
        programs.push(json!({"name":name,"compile_ms":compile_ms,"execution":summary(&samples)?,"executable_bytes":fs::metadata(&executable).map_err(|error|error.to_string())?.len()}));
    }
    let report = json!({"version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "measurement":"local wall clock, process startup included; host contention is not controlled", "components":components,"compiler_startup":summary(&startup)?,"programs":programs});
    fs::write(
        output,
        serde_json::to_string_pretty(&report).map_err(|error| error.to_string())? + "\n",
    )
    .map_err(|error| error.to_string())?;
    println!("Performance report: {}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_summary_uses_sorted_nearest_rank_and_rejects_empty_or_invalid_samples() {
        let samples: Vec<_> = (1..=20).rev().map(f64::from).collect();
        let value = summary(&samples).unwrap();
        assert_eq!(value["samples"], 20);
        assert_eq!(value["median_ms"], 10.5);
        assert_eq!(value["p95_ms"], 19.0);
        assert!(summary(&[]).is_err());
        assert!(summary(&[f64::NAN]).is_err());
        assert!(summary(&[-1.0]).is_err());
    }
}
