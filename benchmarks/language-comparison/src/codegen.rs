//! Native optimization followup; run from the repository root on macOS.
//! Usage: codegen BEFORE_COMPILER AFTER_COMPILER RUNTIME BEFORE_PROGRAM
//!        AFTER_PROGRAM RUST_PROGRAM NEW_OUTPUT_DIRECTORY
//! Uses unchanged workloads.fn; every build gets the same explicit runtime.
#[allow(dead_code)]
mod oracle;

use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Instant,
};

const SOURCE: &str = "benchmarks/language-comparison/programs/workloads.fn";

fn rss(stderr: &str) -> u64 {
    stderr
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_suffix(" maximum resident set size")
                .and_then(|number| number.trim().parse().ok())
        })
        .expect("macOS time must report peak RSS")
}

fn success(command: &Command, output: &Output) {
    assert!(output.status.success(), "{command:?}: {output:?}");
}

fn expected(mode: &str, steps: u64, seed: u64) -> String {
    match mode {
        "scalar" => format!("{}\n", oracle::scalar(steps, seed)),
        "model" => format!("{}\n0\n", oracle::model(steps, seed)),
        _ => unreachable!("only bounded scalar/model workloads"),
    }
}

fn workload(binary: &Path, mode: &str, steps: u64, seed: u64) -> Command {
    let mut command = Command::new(binary);
    command.args([mode, &steps.to_string(), &seed.to_string()]);
    command
}

fn verify(binary: &Path, mode: &str, steps: u64, seed: u64) {
    let mut command = workload(binary, mode, steps, seed);
    let output = command.output().expect("execute trusted bounded workload");
    success(&command, &output);
    assert_eq!(output.stdout, expected(mode, steps, seed).as_bytes());
    assert!(output.stderr.is_empty(), "{output:?}");
}

/// Record wrapper-inclusive wall time and unmodified subprocess streams.
fn timed(command: &mut Command, out: &Path, name: &str) -> (Output, f64, u64) {
    let start = Instant::now();
    let output = command.output().expect("execute trusted measurement");
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    fs::write(out.join(format!("{name}.stdout")), &output.stdout).unwrap();
    fs::write(out.join(format!("{name}.stderr")), &output.stderr).unwrap();
    success(command, &output);
    let peak = rss(std::str::from_utf8(&output.stderr).unwrap());
    (output, elapsed, peak)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn hash(metadata: &mut fs::File, label: &str, path: &Path) {
    let mut command = Command::new("/usr/bin/shasum");
    command.args(["-a", "256"]).arg(path);
    let output = command.output().unwrap();
    success(&command, &output);
    writeln!(
        metadata,
        "{label}: {} bytes; {}",
        fs::metadata(path).unwrap().len(),
        std::str::from_utf8(&output.stdout).unwrap().trim()
    )
    .unwrap();
}

fn main() {
    assert!(cfg!(target_os = "macos"), "measurement uses macOS time -l");
    let args: Vec<_> = env::args_os().collect();
    assert_eq!(
        args.len(),
        8,
        "codegen BEFORE_COMPILER AFTER_COMPILER RUNTIME BEFORE_PROGRAM AFTER_PROGRAM RUST_PROGRAM NEW_OUTPUT_DIRECTORY"
    );
    let inputs: Vec<PathBuf> = args[1..7]
        .iter()
        .map(|path| fs::canonicalize(path).expect("input must exist"))
        .collect();
    let source = fs::canonicalize(SOURCE).expect("run from repository root");
    let out = Path::new(&args[7]);
    fs::create_dir(out).expect("retain measurements in a new directory");
    let raw = out.join("raw");
    fs::create_dir(&raw).unwrap();
    let builds = out.join("builds");
    fs::create_dir(&builds).unwrap();
    let mut metadata = fs::File::create(out.join("inputs.txt")).unwrap();
    for (label, path) in [
        "before compiler",
        "after compiler",
        "fixed runtime archive",
        "before workload",
        "after workload",
        "Rust workload",
    ]
    .into_iter()
    .zip(&inputs)
    {
        hash(&mut metadata, label, path);
    }
    hash(&mut metadata, "Fern source", &source);
    for file in ["codegen.rs", "oracle.rs"] {
        hash(
            &mut metadata,
            file,
            &Path::new("benchmarks/language-comparison/src").join(file),
        );
    }
    writeln!(
        metadata,
        "working directory: {}",
        env::current_dir().unwrap().display()
    )
    .unwrap();
    writeln!(metadata, "Timing includes /usr/bin/time launch and reap; raw streams retained. Scalar: 20,000,000 steps, seed 7; 1 warmup + 9 measured rotated rounds. Build: 1 warmup + 5 measured rotated rounds, unique output paths, FERN_RUNTIME_LIB fixed, LIBRARY_PATH removed. Correctness checks run outside timed intervals; medians exclude tagged warmups. No CPU isolation is implied.").unwrap();

    let labels = ["before", "after", "rust"];
    let binaries = &inputs[3..6];
    let mut comparisons = 0;
    for binary in binaries {
        for seed in [0, 1, 7, 255, 12_345, 2_147_483_646] {
            for steps in [0, 1, 2, 63, 257, 1025] {
                verify(binary, "scalar", steps, seed);
                comparisons += 1;
            }
        }
    }
    let mut csv = fs::File::create(out.join("measurements.csv")).unwrap();
    writeln!(
        csv,
        "operation,implementation,steps,seed,round,phase,elapsed_ms,peak_rss_bytes"
    )
    .unwrap();
    let mut scalar_times = [Vec::new(), Vec::new(), Vec::new()];
    let mut scalar_rss = [0_u64; 3];
    for round in 0..10 {
        for offset in 0..3 {
            let index = (round + offset) % 3;
            let label = labels[index];
            let mut command = Command::new("/usr/bin/time");
            command
                .arg("-l")
                .arg(&binaries[index])
                .args(["scalar", "20000000", "7"]);
            let (output, elapsed, peak) =
                timed(&mut command, &raw, &format!("scalar-{label}-{round}"));
            assert_eq!(output.stdout, expected("scalar", 20_000_000, 7).as_bytes());
            comparisons += 1;
            let phase = if round == 0 { "warmup" } else { "measured" };
            writeln!(
                csv,
                "scalar,{label},20000000,7,{round},{phase},{elapsed:.6},{peak}"
            )
            .unwrap();
            csv.flush().unwrap();
            if round != 0 {
                scalar_times[index].push(elapsed);
                scalar_rss[index] = scalar_rss[index].max(peak);
            }
        }
    }

    let mut build_times = [Vec::new(), Vec::new()];
    let mut build_rss = [0_u64; 2];
    let mut built_metadata = fs::File::create(out.join("built-binaries.txt")).unwrap();
    for round in 0..6 {
        for offset in 0..2 {
            let index = (round + offset) % 2;
            let label = labels[index];
            let binary = builds.join(format!("{label}-{round}"));
            let mut command = Command::new("/usr/bin/time");
            command
                .arg("-l")
                .arg(&inputs[index])
                .arg("build")
                .arg(&source)
                .arg("-o")
                .arg(&binary)
                .env("FERN_RUNTIME_LIB", &inputs[2])
                .env_remove("LIBRARY_PATH");
            let (_, elapsed, peak) = timed(&mut command, &raw, &format!("build-{label}-{round}"));
            let phase = if round == 0 { "warmup" } else { "measured" };
            writeln!(csv, "build,{label},,,{round},{phase},{elapsed:.6},{peak}").unwrap();
            csv.flush().unwrap();
            if round != 0 {
                build_times[index].push(elapsed);
                build_rss[index] = build_rss[index].max(peak);
            }
            // Independent output validation and hashing never enter build timing.
            verify(&binary, "scalar", 10_000, 7);
            verify(&binary, "model", 1025, 7);
            comparisons += 2;
            hash(&mut built_metadata, &format!("{label}-{round}"), &binary);
        }
    }
    fs::write(out.join("verification.txt"), format!("{comparisons} independent output comparisons passed, including each measured scalar result and scalar/model checks for all 12 fresh builds. Model checks retain the original zero-valued alias.\n")).unwrap();
    let [before, after, rust] = scalar_times.map(median);
    let [build_before, build_after] = build_times.map(median);
    let summary = format!(
        "| Operation | Before | After | Rust | Before / after |\n| --- | ---: | ---: | ---: | ---: |\n| Scalar 20M | {before:.2} ms | {after:.2} ms | {rust:.2} ms | {:.2}× |\n| Source build | {build_before:.2} ms | {build_after:.2} ms | — | {:.2}× |\n\nMaximum measured peak RSS (bytes): scalar before/after/Rust {scalar_rss:?}; compiler before/after {build_rss:?}.\n\nWhole-process medians exclude tagged warmups. Build timing includes code generation and system linking against the same runtime archive. All raw streams, timing samples, input hashes and fresh build hashes are retained.\n",
        before / after,
        build_before / build_after
    );
    fs::write(out.join("summary.md"), &summary).unwrap();
    print!("{summary}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_peak_rss_without_confusing_other_time_counters() {
        assert_eq!(
            rss(
                "0.03 real 0.01 user 0.01 sys\n 123456 maximum resident set size\n 42 page reclaims\n"
            ),
            123456
        );
    }

    #[test]
    fn exact_outputs_include_scalar_boundaries_and_immutable_alias() {
        assert_eq!(expected("scalar", 0, 2_147_483_646), "2147483646\n");
        assert_eq!(expected("scalar", 2, 1), "182605794\n");
        assert_eq!(expected("model", 256, 7), "32896\n0\n");
        assert_eq!(median(vec![3.0, 1.0, 2.0]), 2.0);
    }
}
