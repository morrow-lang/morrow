//! Correctness-checked macOS BEAM comparison. No third-party harness dependencies.
//! beam FERN_BATCH RUST_BATCH ELIXIR COMPILED_BEAM_DIRECTORY NEW_OUTPUT_DIRECTORY
mod oracle;

use std::{env, fs, io::Write, path::Path, process::Command, time::Instant};

fn expected(mode: &str, steps: u64, seed: u64, repeats: usize) -> String {
    let single = match mode {
        "scalar" => format!("{}\n", oracle::scalar(steps, seed)),
        "model" | "model-struct" => format!("{}\n0\n", oracle::model(steps, seed)),
        "precision" => format!("{}\n", 9_007_199_254_740_993 + seed),
        _ => panic!("unknown controlled workload"),
    };
    single.repeat(repeats)
}

fn rss(stderr: &str) -> u64 {
    stderr
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_suffix(" maximum resident set size")?
                .trim()
                .parse()
                .ok()
        })
        .expect("macOS time must report peak RSS")
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn invocation(
    inputs: &[String],
    implementation: usize,
    mode: &str,
    steps: u64,
    seed: u64,
    repeats: usize,
) -> Vec<String> {
    let mut args = if implementation < 2 {
        vec![inputs[implementation].clone()]
    } else {
        vec![
            inputs[2].clone(),
            "-pa".into(),
            inputs[3].clone(),
            "-e".into(),
            "FernComparison.main()".into(),
            "--".into(),
            "batch".into(),
        ]
    };
    args.extend([
        mode.into(),
        steps.to_string(),
        seed.to_string(),
        repeats.to_string(),
    ]);
    args
}

fn checked_output(args: &[String]) -> std::process::Output {
    let output = Command::new(&args[0])
        .args(&args[1..])
        .output()
        .expect("execute bounded local comparator");
    assert!(output.status.success(), "{args:?}: {output:?}");
    output
}

fn main() {
    assert!(
        cfg!(target_os = "macos"),
        "measurement requires macOS time -l"
    );
    let args: Vec<String> = env::args().collect();
    assert_eq!(
        args.len(),
        6,
        "beam FERN_BATCH RUST_BATCH ELIXIR COMPILED_BEAM_DIRECTORY NEW_OUTPUT_DIRECTORY"
    );
    let inputs: Vec<String> = args[1..5]
        .iter()
        .map(|p| fs::canonicalize(p).unwrap().to_str().unwrap().to_owned())
        .collect();
    let out = Path::new(&args[5]);
    fs::create_dir(out).expect("new exclusive evidence directory");
    fs::create_dir(out.join("raw")).unwrap();
    let mut metadata = fs::File::create(out.join("inputs.txt")).unwrap();
    writeln!(metadata, "Arguments: {inputs:?}\nHost: {:?}\nBEAM flags: ERL_FLAGS={:?}, ERL_AFLAGS={:?}, ELIXIR_ERL_OPTIONS={:?}", checked_output(&["/usr/bin/uname".into(), "-a".into()]), env::var("ERL_FLAGS"), env::var("ERL_AFLAGS"), env::var("ELIXIR_ERL_OPTIONS")).unwrap();
    let version = checked_output(&[inputs[2].clone(), "--version".into()]);
    fs::write(out.join("elixir-version.txt"), &version.stdout).unwrap();
    let mut files = vec![inputs[0].clone(), inputs[1].clone(), inputs[2].clone()];
    files.extend(
        fs::read_dir(&inputs[3])
            .unwrap()
            .map(|entry| entry.unwrap().path().to_str().unwrap().to_owned())
            .filter(|path| path.ends_with(".beam")),
    );
    files.extend(
        [
            "programs/batch.fn",
            "programs/batch.rs",
            "programs/workloads.ex",
            "src/beam.rs",
            "src/oracle.rs",
        ]
        .map(|p| format!("benchmarks/language-comparison/{p}")),
    );
    for file in files {
        let hash = checked_output(&[
            "/usr/bin/shasum".into(),
            "-a".into(),
            "256".into(),
            file.clone(),
        ]);
        writeln!(
            metadata,
            "{} bytes; {}",
            fs::metadata(&file).unwrap().len(),
            String::from_utf8(hash.stdout).unwrap().trim()
        )
        .unwrap();
    }

    let mut checks = 0;
    // Independent native boundary cases, including repeated evaluation and old aliases.
    for implementation in 0..2 {
        for mode in ["scalar", "model", "precision"] {
            for steps in [0, 1, 2, 255, 256, 257, 1025] {
                for seed in [0, 7, 255, 12_345, 2_147_483_646] {
                    let cmd = invocation(&inputs, implementation, mode, steps, seed, 2);
                    let output = checked_output(&cmd);
                    assert!(output.stderr.is_empty(), "{output:?}");
                    assert_eq!(
                        output.stdout,
                        expected(mode, steps, seed, 2).as_bytes(),
                        "{cmd:?}"
                    );
                    checks += 2;
                }
            }
        }
    }
    // Batch BEAM boundary cases inside one VM; no startup-time multiplication.
    let output = checked_output(&[
        inputs[2].clone(),
        "-pa".into(),
        inputs[3].clone(),
        "-e".into(),
        "FernComparison.main()".into(),
        "--".into(),
        "verify".into(),
    ]);
    assert!(output.stderr.is_empty(), "{output:?}");
    fs::write(out.join("verification.stdout"), &output.stdout).unwrap();
    let lines = String::from_utf8(output.stdout).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    assert_eq!(
        lines.lines().next(),
        Some("case,mode,steps,seed,result,original")
    );
    for (index, line) in lines.lines().skip(1).enumerate() {
        let fields: Vec<_> = line.split(',').collect();
        assert_eq!(fields.len(), 6, "{line}");
        assert_eq!(fields[0].parse::<usize>().unwrap(), index);
        let mode = fields[1];
        let steps = fields[2].parse::<u64>().unwrap();
        let seed = fields[3].parse::<u64>().unwrap();
        let expected = expected(mode, steps, seed, 1);
        assert_eq!(fields[4], expected.lines().next().unwrap(), "{line}");
        assert_eq!(fields[5], "0", "{line}");
        assert!(
            seen.insert((mode, steps, seed)),
            "duplicate verification case"
        );
        checks += 1;
    }
    for mode in ["scalar", "model", "model-struct", "precision"] {
        for steps in [0, 1, 2, 255, 256, 257, 1000] {
            for seed in [0, 1, 7, 17, 255, 256, 2_147_483_646] {
                assert!(
                    seen.contains(&(mode, steps, seed)),
                    "missing {mode}/{steps}/{seed}"
                );
            }
        }
    }

    let labels = ["fern", "rust", "elixir-tuple", "elixir-struct"];
    let mut csv = fs::File::create(out.join("measurements.csv")).unwrap();
    writeln!(csv, "implementation,mode,steps,seed,repeats,round,phase,elapsed_ms,per_workload_ms,peak_rss_bytes").unwrap();
    let mut summary = String::from(
        "| Workload | Repeats/process | Fern ms/workload | Rust | Elixir tuples | Elixir structs |\n| --- | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for (mode, steps, repeats) in [
        ("scalar", 0, 1),
        ("model", 10_000, 1),
        ("model", 100_000, 1),
        ("scalar", 20_000_000, 1),
        ("model", 10_000, 10),
        ("model", 100_000, 10),
        ("scalar", 20_000_000, 10),
    ] {
        let mut times: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::new());
        for round in 0..8 {
            for offset in 0..4 {
                let i = (round + offset) % 4;
                let run_mode = if mode == "model" && i == 3 {
                    "model-struct"
                } else {
                    mode
                };
                let cmd = invocation(&inputs, i, run_mode, steps, 7, repeats);
                let mut command = Command::new("/usr/bin/time");
                command.arg("-l").args(&cmd);
                let start = Instant::now();
                let output = command.output().unwrap();
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let name = format!("{}-{mode}-{steps}-{repeats}-{round}", labels[i]);
                fs::write(
                    out.join("raw").join(format!("{name}.stdout")),
                    &output.stdout,
                )
                .unwrap();
                fs::write(
                    out.join("raw").join(format!("{name}.stderr")),
                    &output.stderr,
                )
                .unwrap();
                assert!(output.status.success(), "{command:?}: {output:?}");
                assert_eq!(
                    output.stdout,
                    expected(run_mode, steps, 7, repeats).as_bytes(),
                    "{command:?}"
                );
                checks += repeats;
                let peak = rss(std::str::from_utf8(&output.stderr).unwrap());
                let per_workload = elapsed / repeats as f64;
                let phase = if round == 0 { "warmup" } else { "measured" };
                writeln!(csv, "{},{mode},{steps},7,{repeats},{round},{phase},{elapsed:.6},{per_workload:.6},{peak}", labels[i]).unwrap();
                csv.flush().unwrap();
                if round > 0 {
                    times[i].push(per_workload);
                }
            }
        }
        let [fern, rust, tuple, structure] = times.map(median);
        let row = format!(
            "| {mode} {steps} | {repeats} | {fern:.3} | {rust:.3} | {tuple:.3} | {structure:.3} |\n"
        );
        print!("{row}");
        summary.push_str(&row);
    }
    // Supplementary in-VM timings exclude startup entirely. They are not used
    // as if native whole-process samples were equivalent CPU-only timings.
    let mut warm_summary = String::from("| BEAM workload | In-VM median ms |\n| --- | ---: |\n");
    for (mode, steps) in [
        ("model", 10_000),
        ("model-struct", 10_000),
        ("model", 100_000),
        ("model-struct", 100_000),
        ("scalar", 20_000_000),
    ] {
        let output = checked_output(&[
            inputs[2].clone(),
            "-pa".into(),
            inputs[3].clone(),
            "-e".into(),
            "FernComparison.main()".into(),
            "--".into(),
            "warm".into(),
            mode.into(),
            steps.to_string(),
            "7".into(),
            "7".into(),
        ]);
        assert!(output.stderr.is_empty(), "{output:?}");
        fs::write(
            out.join("raw").join(format!("warm-{mode}-{steps}.stdout")),
            &output.stdout,
        )
        .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(
            text.lines().next(),
            Some("round,elapsed_ns,checksum,original")
        );
        let mut samples = Vec::new();
        for (round, line) in text.lines().skip(1).enumerate() {
            let fields: Vec<_> = line.split(',').collect();
            assert_eq!(fields.len(), 4);
            assert_eq!(fields[0].parse::<usize>().unwrap(), round);
            assert_eq!(
                fields[2],
                expected(mode, steps, 7, 1).lines().next().unwrap()
            );
            assert_eq!(fields[3], "0");
            checks += 1;
            if round > 0 {
                samples.push(fields[1].parse::<u64>().unwrap() as f64 / 1_000_000.0);
            }
        }
        assert_eq!(samples.len(), 7);
        warm_summary.push_str(&format!("| {mode} {steps} | {:.3} |\n", median(samples)));
    }
    print!("{warm_summary}");
    fs::write(out.join("warm-summary.md"), warm_summary).unwrap();
    fs::write(out.join("summary.md"), summary).unwrap();
    fs::write(out.join("verification.txt"), format!("{checks} independent result checks; every measured output verified, original model aliases remain zero. Seven measured rotated rounds plus one retained warmup. All measurements include process startup, output, shutdown and time wrapper; batch rows divide total elapsed by ten, do not subtract an estimated startup.\n")).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_repetitions_preserve_alias_and_precision() {
        assert_eq!(expected("model", 256, 7, 2), "32896\n0\n32896\n0\n");
        assert_eq!(expected("model-struct", 1, 7, 1), "8\n0\n");
        assert_eq!(expected("precision", 0, 7, 1), "9007199254741000\n");
        assert_eq!(expected("scalar", 2, 1, 0), "");
    }

    #[test]
    fn process_measurements_keep_units_and_order() {
        assert_eq!(
            rss("0.1 real\n 1234 maximum resident set size\n42 page reclaims\n"),
            1234
        );
        assert_eq!(median(vec![3.0, 1.0, 2.0]), 2.0);
        let inputs = ["/fern", "/rust", "/elixir", "/beam dir"].map(str::to_owned);
        assert_eq!(
            invocation(&inputs, 0, "scalar", 2, 7, 10),
            ["/fern", "scalar", "2", "7", "10"]
        );
        assert_eq!(
            invocation(&inputs, 3, "model-struct", 2, 7, 10)
                .last()
                .unwrap(),
            "10"
        );
    }
}
