//! Compare unchanged immutable workloads against independently counted visits.
//! Usage: immutable BEFORE AFTER RUST OUTPUT_DIRECTORY (macOS timing/RSS).
#[allow(dead_code)]
mod oracle;

use std::{env, fs, io::Write, path::Path, process::Command, time::Instant};

fn run(binary: &str, steps: u64, seed: u64, timed: bool) -> (f64, u64) {
    let mut command = if timed {
        let mut command = Command::new("/usr/bin/time");
        command.args(["-l", binary]);
        command
    } else {
        Command::new(binary)
    };
    command.args(["model", &steps.to_string(), &seed.to_string()]);
    let start = Instant::now();
    let output = command.output().expect("run trusted bounded workload");
    let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
    assert!(output.status.success(), "{command:?}: {output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n0\n", oracle::model(steps, seed)),
        "checksum and original alias: {command:?}"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    let rss = if timed {
        stderr
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_suffix(" maximum resident set size")
                    .map(|number| number.trim().parse::<u64>().unwrap())
            })
            .expect("macOS time must report peak RSS")
    } else {
        assert!(stderr.is_empty(), "{stderr}");
        0
    };
    (milliseconds, rss)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    assert!(cfg!(target_os = "macos"), "measurement uses macOS time -l");
    let args: Vec<_> = env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "immutable BEFORE AFTER RUST OUTPUT_DIRECTORY"
    );
    let out = Path::new(&args[4]);
    fs::create_dir(out).expect("retain measurements in a new directory");
    let binaries = ["before", "after", "rust"]
        .into_iter()
        .zip(&args[1..4])
        .collect::<Vec<_>>();
    let mut metadata = fs::File::create(out.join("binaries.txt")).unwrap();
    for (label, binary) in &binaries {
        let hash = Command::new("/usr/bin/shasum")
            .args(["-a", "256", binary])
            .output()
            .unwrap();
        assert!(hash.status.success());
        writeln!(
            metadata,
            "{label}: {} bytes; {}",
            fs::metadata(binary).unwrap().len(),
            String::from_utf8(hash.stdout).unwrap().trim()
        )
        .unwrap();
        for seed in [0, 1, 7, 255, 256, 257, 12345] {
            for steps in [0, 1, 2, 255, 256, 257, 1025] {
                run(binary, steps, seed, false);
            }
        }
    }
    fs::write(
        out.join("verification.txt"),
        "147 independent checksum and retained-original comparisons passed\n",
    )
    .unwrap();
    let mut csv = fs::File::create(out.join("measurements.csv")).unwrap();
    writeln!(
        csv,
        "implementation,steps,seed,round,phase,elapsed_ms,peak_rss_bytes"
    )
    .unwrap();
    let mut summary = String::from(
        "| Updates | Before | After | Rust | Speedup |\n| --- | ---: | ---: | ---: | ---: |\n",
    );
    for steps in [10_000, 100_000] {
        let mut timings = [Vec::new(), Vec::new(), Vec::new()];
        for round in 0..10 {
            // Rotate order, retaining the full-workload warmup separately.
            for offset in 0..3 {
                let index = (round + offset) % 3;
                let (label, binary) = binaries[index];
                let (elapsed, rss) = run(binary, steps, 7, true);
                let phase = if round == 0 { "warmup" } else { "measured" };
                writeln!(csv, "{label},{steps},7,{round},{phase},{elapsed:.6},{rss}").unwrap();
                csv.flush().unwrap();
                if round != 0 {
                    timings[index].push(elapsed);
                }
            }
        }
        let [before, after, rust] = timings.map(median);
        summary.push_str(&format!(
            "| {steps} | {before:.2} ms | {after:.2} ms | {rust:.2} ms | {:.2}× |\n",
            before / after
        ));
    }
    fs::write(out.join("summary.md"), &summary).unwrap();
    print!("{summary}");
}
