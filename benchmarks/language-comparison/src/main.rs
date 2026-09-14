//! Dependency-free harness for bounded, trusted comparison fixtures.
mod oracle;

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Instant,
};

struct Tools {
    fern: String,
    rustc: String,
    bun: String,
    tsc: String,
}

impl Tools {
    fn load() -> Self {
        Self {
            fern: env::var("FERN").unwrap_or_else(|_| "target/release/fern".into()),
            rustc: env::var("RUSTC").unwrap_or_else(|_| "rustc".into()),
            bun: env::var("BUN").unwrap_or_else(|_| "bun".into()),
            tsc: env::var("TSC").expect("TSC must name a pinned TypeScript bin/tsc file"),
        }
    }

    fn check(&self, language: &str, source: &Path, out: &Path, strict: bool) -> Command {
        match language {
            "fern" => {
                let mut c = Command::new(&self.fern);
                c.arg("check").arg(source);
                c
            }
            "rust" => {
                let mut c = Command::new(&self.rustc);
                c.args([
                    "--edition=2024",
                    "--crate-name",
                    "comparison",
                    "--emit=metadata",
                ])
                .arg(source)
                .arg("-o")
                .arg(out.join("check.rmeta"));
                if strict {
                    c.arg("-Dunused_must_use");
                }
                c
            }
            "typescript" => {
                let mut c = Command::new(&self.bun);
                c.arg(&self.tsc)
                    .args(["--strict", "--noEmit", "--target", "ES2022"]);
                if strict {
                    c.arg("--noUncheckedIndexedAccess");
                }
                c.arg(source);
                c
            }
            _ => unreachable!(),
        }
    }

    fn build(&self, language: &str, source: &Path, target: &Path) -> Command {
        match language {
            "fern" => {
                let mut c = Command::new(&self.fern);
                c.arg("build").arg(source).arg("-o").arg(target);
                c
            }
            "rust" => {
                let mut c = Command::new(&self.rustc);
                c.args([
                    "--edition=2024",
                    "--crate-name",
                    "comparison",
                    "-O",
                    "-C",
                    "strip=symbols",
                    "-C",
                    "panic=abort",
                ])
                .arg(source)
                .arg("-o")
                .arg(target);
                c
            }
            _ => unreachable!(),
        }
    }
}

fn execute(command: &mut Command) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{command:?}: {error}"))
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "status {}\nstdout {}\nstderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn program(tools: &Tools, out: &Path, language: &str, name: &str) -> Command {
    if language == "bun" {
        let mut command = Command::new(&tools.bun);
        command.arg(format!("benchmarks/language-comparison/programs/{name}.ts"));
        command
    } else {
        Command::new(out.join(format!("{name}-{language}")))
    }
}

fn prepare(tools: &Tools, out: &Path) {
    fs::create_dir(out).expect("output directory must be new");
    let mut metadata = String::new();
    for (label, command) in [
        ("fern", Command::new(&tools.fern).arg("--version")),
        ("rust", Command::new(&tools.rustc).arg("-vV")),
        ("bun", Command::new(&tools.bun).arg("--version")),
        (
            "typescript",
            Command::new(&tools.bun).arg(&tools.tsc).arg("--version"),
        ),
        ("source", Command::new("git").args(["rev-parse", "HEAD"])),
        ("platform", Command::new("uname").arg("-a")),
    ] {
        let result = execute(command);
        success(&result);
        metadata.push_str(&format!(
            "{label}: {}\n",
            String::from_utf8_lossy(&result.stdout).trim()
        ));
    }
    metadata.push_str(&format!(
        "FERN_RUNTIME_LIB: {:?}\n",
        env::var("FERN_RUNTIME_LIB")
    ));
    fs::write(out.join("environment.txt"), metadata).unwrap();
    let mut sizes = String::from("program,language,bytes\n");
    for name in ["startup", "workloads"] {
        for (language, extension) in [("fern", "fn"), ("rust", "rs")] {
            let source = PathBuf::from(format!(
                "benchmarks/language-comparison/programs/{name}.{extension}"
            ));
            success(&execute(&mut tools.check(language, &source, out, true)));
            let target = out.join(format!("{name}-{language}"));
            success(&execute(&mut tools.build(language, &source, &target)));
            sizes.push_str(&format!(
                "{name},{language},{}\n",
                fs::metadata(target).unwrap().len()
            ));
        }
        let source = PathBuf::from(format!("benchmarks/language-comparison/programs/{name}.ts"));
        success(&execute(&mut tools.check("typescript", &source, out, true)));
        sizes.push_str(&format!(
            "{name},typescript-source,{}\n",
            fs::metadata(source).unwrap().len()
        ));
    }
    sizes.push_str(&format!(
        "runtime,bun,{}\n",
        fs::metadata(&tools.bun).unwrap().len()
    ));
    fs::write(out.join("sizes.csv"), sizes).unwrap();
}

fn expected(mode: &str, steps: u64, seed: u64) -> String {
    match mode {
        "scalar" | "scalar-bigint" => format!("{}\n", oracle::scalar(steps, seed)),
        "precision" => format!("{}\n", 9_007_199_254_740_993_u64 + seed),
        _ => format!("{}\n0\n", oracle::model(steps, seed)),
    }
}

fn verify(tools: &Tools, out: &Path) {
    let mut count = 0;
    for (steps, seed) in [
        (0, 1),
        (1, 1),
        (2, 1),
        (17, 7),
        (256, 255),
        (513, 9),
        (1_000, 7),
    ] {
        for (language, modes) in [
            ("fern", vec!["scalar", "model", "precision"]),
            ("rust", vec!["scalar", "model", "mutable", "precision"]),
            (
                "bun",
                vec!["scalar", "scalar-bigint", "model", "mutable", "precision"],
            ),
        ] {
            for mode in modes {
                let result = execute(program(tools, out, language, "workloads").args([
                    mode,
                    &steps.to_string(),
                    &seed.to_string(),
                ]));
                success(&result);
                assert_eq!(
                    String::from_utf8_lossy(&result.stdout),
                    expected(mode, steps, seed),
                    "{language} {mode} {steps} {seed}"
                );
                count += 1;
            }
        }
    }
    let precision =
        execute(program(tools, out, "bun", "workloads").args(["precision-number", "0", "0"]));
    success(&precision);
    assert_eq!(precision.stdout, b"9007199254740992\n");
    for (case, expected_stdout) in [("exhaustive", "pending\n"), ("bounds", "undefined\n")] {
        let result = execute(Command::new(&tools.bun).arg(format!(
            "benchmarks/language-comparison/mutations/{case}.ts"
        )));
        success(&result);
        assert_eq!(String::from_utf8_lossy(&result.stdout), expected_stdout);
    }
    let mut observations = String::from("case,language,strict_extra,rejected\n");
    for (case, fern_reject, rust_reject, ts_reject) in [
        ("exhaustive", true, true, true),
        ("result", true, false, false),
        ("labels", true, false, false),
        ("bounds", false, false, false),
        ("newtypes", true, true, true),
    ] {
        for (language, extension, reject) in [
            ("fern", "fn", fern_reject),
            ("rust", "rs", rust_reject),
            ("typescript", "ts", ts_reject),
        ] {
            let source = PathBuf::from(format!(
                "benchmarks/language-comparison/mutations/{case}.{extension}"
            ));
            for strict in [false, true] {
                let result = execute(&mut tools.check(language, &source, out, strict));
                let expected_reject = reject
                    || (strict
                        && ((case == "result" && language == "rust")
                            || (case == "bounds" && language == "typescript")));
                assert_eq!(
                    !result.status.success(),
                    expected_reject,
                    "{case} {language} strict={strict}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                let log = [result.stdout, result.stderr].concat();
                if expected_reject {
                    let phrase = match (case, language) {
                        ("exhaustive", "fern") => "match must be exhaustive",
                        ("exhaustive", "rust") => "E0004",
                        ("exhaustive" | "bounds", "typescript") => "TS2322",
                        ("result", "fern") => "Result value must be handled",
                        ("result", "rust") => "unused `Result`",
                        ("labels", "fern") => "argument requires label",
                        ("newtypes", "fern") => "UserId",
                        ("newtypes", "rust") => "E0308",
                        ("newtypes", "typescript") => "TS2345",
                        _ => unreachable!(),
                    };
                    assert!(
                        String::from_utf8_lossy(&log).contains(phrase),
                        "wrong rejection: {}",
                        String::from_utf8_lossy(&log)
                    );
                }
                fs::write(
                    out.join(format!("mutation-{case}-{language}-{strict}.txt")),
                    log,
                )
                .unwrap();
                observations.push_str(&format!("{case},{language},{strict},{expected_reject}\n"));
            }
            let original = fs::read_to_string(&source).unwrap();
            let fixed = match (case, language) {
                ("exhaustive", "fern") => original.replace("    Paused\n", ""),
                ("exhaustive", "rust") => original.replace(", Paused", "").replace("    Paused,\n", ""),
                ("exhaustive", "typescript") => original.replace(" | \"paused\"", ""),
                ("result", "fern") => original.replace("validate(-1)", "match validate(-1):\n        Ok(value) -> println(value)\n        Err(message) -> println(message)"),
                ("result", "rust") => original.replace("validate(-1);", "let _ = validate(-1);"),
                ("labels", "fern") => original.replace("transfer(100, 20)", "transfer(from: 100, to: 20)"),
                ("newtypes", "fern" | "rust") => original.replace("ProductId(1)", "UserId(1)"),
                ("newtypes", "typescript") => original.replace("load(product)", "load(1 as UserId)"),
                ("bounds", "typescript") => original.replace("values[3]", "values[3] ?? 0"),
                _ => original,
            };
            let positive = out.join(format!("positive_{case}.{extension}"));
            fs::write(&positive, fixed).unwrap();
            success(&execute(&mut tools.check(language, &positive, out, true)));
        }
    }
    for (language, extension) in [("fern", "fn"), ("rust", "rs")] {
        let source = PathBuf::from(format!(
            "benchmarks/language-comparison/mutations/bounds.{extension}"
        ));
        let target = out.join(format!("bounds-{language}"));
        success(&execute(&mut tools.build(language, &source, &target)));
        let result = execute(&mut Command::new(target));
        assert!(!result.status.success());
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("out of bounds"), "{diagnostic}");
        fs::write(
            out.join(format!("runtime-bounds-{language}.txt")),
            result.stderr,
        )
        .unwrap();
    }
    fs::write(out.join("mutations.csv"), observations).unwrap();
    fs::write(out.join("verification.txt"), format!("{count} independent-output comparisons; exact Number precision-loss oracle; 30 checker observations with diagnostic matching; 15 positive checker controls; 2 runtime bounds faults; 2 Bun execution-without-typecheck controls.\n")).unwrap();
    println!("Verified {count} workload outputs and all mutation contracts.");
}

fn timed(command: &mut Command) -> (u128, u64, Output) {
    let mut timer = Command::new("/usr/bin/time");
    timer
        .arg("-l")
        .arg(command.get_program())
        .args(command.get_args());
    let start = Instant::now();
    let output = execute(&mut timer);
    let elapsed = start.elapsed().as_nanos();
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    let rss = diagnostics
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_suffix("maximum resident set size")
                .map(str::trim)
        })
        .expect("macOS /usr/bin/time -l RSS report")
        .parse()
        .unwrap();
    (elapsed, rss, output)
}

fn measure(tools: &Tools, out: &Path) {
    assert_eq!(
        env::consts::OS,
        "macos",
        "RSS units/parser are explicit for macOS"
    );
    let cases = [
        ("fern", "scalar", 20_000_000),
        ("rust", "scalar", 20_000_000),
        ("bun", "scalar", 20_000_000),
        ("bun", "scalar-bigint", 20_000_000),
        ("fern", "model", 10_000),
        ("rust", "model", 10_000),
        ("bun", "model", 10_000),
        ("rust", "mutable", 10_000),
        ("bun", "mutable", 10_000),
    ];
    let mut csv = String::from("case,language,iteration,steps,seed,wall_ns,peak_rss_bytes\n");
    for &(language, mode, steps) in &cases {
        let result = execute(program(tools, out, language, "workloads").args([
            mode,
            &steps.to_string(),
            "7",
        ]));
        success(&result);
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            expected(mode, steps, 7)
        );
    }
    for iteration in 0..9 {
        for offset in 0..cases.len() {
            let (language, mode, steps) = cases[(iteration + offset) % cases.len()];
            let (wall, rss, result) = timed(program(tools, out, language, "workloads").args([
                mode,
                &steps.to_string(),
                "7",
            ]));
            success(&result);
            assert_eq!(
                String::from_utf8_lossy(&result.stdout),
                expected(mode, steps, 7)
            );
            csv.push_str(&format!(
                "{mode},{language},{iteration},{steps},7,{wall},{rss}\n"
            ));
        }
        println!("Measurement round {} of 9 complete", iteration + 1);
    }
    for iteration in 0..21 {
        for language in ["fern", "rust", "bun"] {
            let (wall, rss, result) = timed(&mut program(tools, out, language, "startup"));
            success(&result);
            assert_eq!(result.stdout, b"0\n");
            csv.push_str(&format!(
                "startup,{language},{iteration},0,0,{wall},{rss}\n"
            ));
        }
    }
    for iteration in 0..5 {
        for (language, extension) in [("fern", "fn"), ("rust", "rs"), ("typescript", "ts")] {
            let source = PathBuf::from(format!(
                "benchmarks/language-comparison/programs/workloads.{extension}"
            ));
            let (wall, rss, result) = timed(&mut tools.check(language, &source, out, true));
            success(&result);
            csv.push_str(&format!("check,{language},{iteration},0,0,{wall},{rss}\n"));
            if language != "typescript" {
                let target = out.join(format!("rebuild-{language}"));
                let (wall, rss, result) = timed(&mut tools.build(language, &source, &target));
                success(&result);
                csv.push_str(&format!("build,{language},{iteration},0,0,{wall},{rss}\n"));
            }
        }
    }
    fs::write(out.join("measurements.csv"), csv).unwrap();
}

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "usage: runner prepare|verify|measure|measure-check-policy|summary OUTPUT_DIRECTORY"
    );
    let tools = Tools::load();
    let out = Path::new(&args[2]);
    match args[1].as_str() {
        "prepare" => prepare(&tools, out),
        "verify" => verify(&tools, out),
        "measure" => measure(&tools, out),
        "measure-check-policy" => measure_check_policy(&tools, out),
        "summary" => summary(out),
        _ => panic!("unknown operation"),
    }
}

fn measure_check_policy(tools: &Tools, out: &Path) {
    let source = Path::new("benchmarks/language-comparison/programs/workloads.ts");
    let mut warmup = tools.check("typescript", source, out, true);
    warmup.arg("--skipLibCheck");
    success(&execute(&mut warmup));
    let mut csv = String::from("case,language,iteration,steps,seed,wall_ns,peak_rss_bytes\n");
    for iteration in 0..5 {
        let mut command = tools.check("typescript", source, out, true);
        command.arg("--skipLibCheck");
        let (wall, rss, result) = timed(&mut command);
        success(&result);
        csv.push_str(&format!(
            "check-skip-lib,typescript,{iteration},0,0,{wall},{rss}\n"
        ));
    }
    fs::write(out.join("check-policy.csv"), csv).unwrap();
}

fn summary(out: &Path) {
    let mut groups = BTreeMap::<(String, String), Vec<(u64, u64)>>::new();
    for name in ["measurements.csv", "check-policy.csv"] {
        if !out.join(name).exists() {
            continue;
        }
        for line in fs::read_to_string(out.join(name)).unwrap().lines().skip(1) {
            let columns: Vec<_> = line.split(',').collect();
            assert_eq!(columns.len(), 7);
            groups
                .entry((columns[0].into(), columns[1].into()))
                .or_default()
                .push((columns[5].parse().unwrap(), columns[6].parse().unwrap()));
        }
    }
    println!("| Case | Language | Samples | Median ms | Min–max ms | Median peak RSS MiB |");
    println!("| --- | --- | ---: | ---: | ---: | ---: |");
    for ((case, language), values) in groups {
        let mut times: Vec<_> = values.iter().map(|v| v.0).collect();
        let mut memory: Vec<_> = values.iter().map(|v| v.1).collect();
        times.sort_unstable();
        memory.sort_unstable();
        println!(
            "| {case} | {language} | {} | {:.3} | {:.3}–{:.3} | {:.2} |",
            values.len(),
            times[times.len() / 2] as f64 / 1_000_000.0,
            times[0] as f64 / 1_000_000.0,
            times[times.len() - 1] as f64 / 1_000_000.0,
            memory[memory.len() / 2] as f64 / 1_048_576.0
        );
    }
}
