//! Independent output oracles and bounded runner for the Morrow/Elixir actor comparison.

use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const ROOT: &str = "benchmarks/language-comparison";
const MAX_CAPTURE: usize = 1_048_576;

#[derive(Clone, Copy, Debug)]
enum Workload {
    RequestReply {
        clients: u64,
        requests: u64,
    },
    Contention {
        schedulers: u64,
        probes: u64,
        work: u64,
    },
    Lifecycle {
        workers: u64,
        faults: u64,
    },
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::RequestReply { .. } => "request-reply",
            Self::Contention { .. } => "contention",
            Self::Lifecycle { .. } => "lifecycle",
        }
    }

    fn arguments(self) -> Vec<String> {
        match self {
            Self::RequestReply { clients, requests } => {
                vec![
                    self.name().into(),
                    clients.to_string(),
                    requests.to_string(),
                ]
            }
            Self::Contention {
                schedulers,
                probes,
                work,
            } => vec![
                self.name().into(),
                schedulers.to_string(),
                probes.to_string(),
                work.to_string(),
            ],
            Self::Lifecycle { workers, faults } => {
                vec![self.name().into(), workers.to_string(), faults.to_string()]
            }
        }
    }

    fn operations(self) -> u64 {
        match self {
            Self::RequestReply { clients, requests } => clients * requests,
            Self::Contention { probes, .. } => probes,
            Self::Lifecycle { workers, faults } => workers + faults * 2,
        }
    }
}

#[derive(Clone, Copy)]
enum Variant {
    Morrow {
        label: &'static str,
        stealing: bool,
        reductions: u16,
    },
    Elixir,
}

impl Variant {
    fn label(self) -> &'static str {
        match self {
            Self::Morrow { label, .. } => label,
            Self::Elixir => "elixir-beam",
        }
    }
}

struct Inputs {
    morrow: PathBuf,
    elixir: PathBuf,
    beam: PathBuf,
}

struct Observation {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    elapsed: Duration,
    samples: Vec<Duration>,
    ready: Option<Duration>,
    hot_done: Option<Duration>,
    finished: Option<Duration>,
    streams_closed: Duration,
    output_overflow: bool,
    timed_out: bool,
}

fn scalar(steps: u64, seed: u64) -> u64 {
    let (mut power, mut exponent, mut factor) = (1_u64, steps, 48_271_u64);
    while exponent != 0 {
        if exponent & 1 != 0 {
            power = power * factor % 2_147_483_647;
        }
        factor = factor * factor % 2_147_483_647;
        exponent >>= 1;
    }
    seed * power % 2_147_483_647
}

fn expected(workload: Workload) -> String {
    match workload {
        Workload::RequestReply { clients, requests } => {
            let replies = clients * requests;
            let payloads = 100_000 * requests * clients * (clients - 1) / 2
                + clients * requests * (requests - 1) / 2;
            let checksum = 3 * payloads + replies;
            format!("ready\nrequest-reply,{clients},{requests},{replies},{checksum}\n")
        }
        Workload::Contention {
            schedulers,
            probes,
            work,
        } => {
            assert!((1..=64).contains(&schedulers));
            let mut output = String::from("ready\n");
            for index in 0..probes {
                output.push_str(&format!("sample,{index}\n"));
            }
            let checksum = probes * (probes - 1) + probes;
            output.push_str("hot-done\n");
            output.push_str(&format!(
                "contention,{probes},{work},{checksum},{}\n",
                scalar(work, 7)
            ));
            output
        }
        Workload::Lifecycle { workers, faults } => {
            let worker_checksum = 17 * workers * (workers - 1) / 2 + workers;
            let fault_checksum = faults * faults.saturating_sub(1);
            format!(
                "ready\nlifecycle,{workers},{faults},{workers},{},{worker_checksum},{fault_checksum}\n",
                faults * 2
            )
        }
    }
}

fn command(inputs: &Inputs, variant: Variant, schedulers: u64, workload: Workload) -> Command {
    let arguments = workload.arguments();
    match variant {
        Variant::Morrow {
            stealing,
            reductions,
            ..
        } => {
            let mut command = Command::new(&inputs.morrow);
            command.args(arguments);
            command.env("MORROW_SCHEDULERS", schedulers.to_string());
            command.env("MORROW_REDUCTIONS", reductions.to_string());
            if stealing {
                command.env("MORROW_WORK_STEALING", "1");
            } else {
                command.env_remove("MORROW_WORK_STEALING");
            }
            command
        }
        Variant::Elixir => {
            let mut command = Command::new(&inputs.elixir);
            command.args([
                "-pa".into(),
                inputs.beam.to_string_lossy().into_owned(),
                "-e".into(),
                "ActorComparison.main()".into(),
                "--".into(),
            ]);
            command.args(arguments);
            command.env("ERL_FLAGS", format!("+S {schedulers}:{schedulers}"));
            command.env_remove("ERL_AFLAGS");
            command.env_remove("ELIXIR_ERL_OPTIONS");
            command
        }
    }
}

fn observe(mut command: Command, timeout: Duration) -> Observation {
    #[cfg(unix)]
    command.process_group(0);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn().expect("execute bounded local comparator");
    let stdout = child.stdout.take().expect("capture stdout");
    let stderr = child.stderr.take().expect("capture stderr");
    let process_group = child.id();
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(1);
    let (stderr_sender, stderr_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout.take((MAX_CAPTURE + 1) as u64));
        let mut all = Vec::new();
        let mut samples = Vec::new();
        let mut ready = None;
        let mut hot_done = None;
        let mut finished = None;
        let mut overflow = false;
        loop {
            let mut line = Vec::new();
            let count = reader.read_until(b'\n', &mut line).expect("read stdout");
            if count == 0 {
                break;
            }
            let observed = started.elapsed();
            match line.as_slice() {
                b"ready\n" => ready = Some(observed),
                b"hot-done\n" => hot_done = Some(observed),
                _ if line.starts_with(b"sample,") => samples.push(observed),
                _ if line.starts_with(b"request-reply,")
                    || line.starts_with(b"contention,")
                    || line.starts_with(b"lifecycle,") =>
                {
                    finished = Some(observed)
                }
                _ => {}
            }
            let retained = line.len().min(MAX_CAPTURE.saturating_sub(all.len()));
            all.extend_from_slice(&line[..retained]);
            overflow |= retained != line.len();
        }
        let closed = started.elapsed();
        let _ = stdout_sender.send((all, samples, ready, hot_done, finished, overflow, closed));
    });
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 8192];
        let mut overflow = false;
        loop {
            let count = reader.read(&mut buffer).expect("read stderr");
            if count == 0 {
                break;
            }
            let retained = count.min(MAX_CAPTURE.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&buffer[..retained]);
            overflow |= retained != count;
        }
        let closed = started.elapsed();
        let _ = stderr_sender.send((bytes, overflow, closed));
    });
    // Retain ownership of the unreaped group leader until both captured pipes
    // close. A descendant retaining a pipe therefore cannot make us surrender
    // the group identity before the timeout cleanup.
    let deadline = started + timeout;
    let mut stdout_result = None;
    let mut stderr_result = None;
    while (stdout_result.is_none() || stderr_result.is_none()) && Instant::now() < deadline {
        if stdout_result.is_none() {
            stdout_result = stdout_receiver.try_recv().ok();
        }
        if stderr_result.is_none() {
            stderr_result = stderr_receiver.try_recv().ok();
        }
        thread::sleep(Duration::from_millis(5));
    }
    let mut timed_out = stdout_result.is_none() || stderr_result.is_none();
    if timed_out {
        signal_group(process_group, "-TERM");
        thread::sleep(Duration::from_millis(100));
        signal_group(process_group, "-KILL");
    }
    if stdout_result.is_none() || stderr_result.is_none() {
        stdout_result =
            stdout_result.or_else(|| stdout_receiver.recv_timeout(Duration::from_secs(2)).ok());
        stderr_result =
            stderr_result.or_else(|| stderr_receiver.recv_timeout(Duration::from_secs(2)).ok());
    }
    let (stdout, samples, ready, hot_done, finished, stdout_overflow, stdout_closed) =
        stdout_result.expect("stdout pipe did not close after process-group termination");
    let (stderr, stderr_overflow, stderr_closed) =
        stderr_result.expect("stderr pipe did not close after process-group termination");
    let status = if timed_out {
        child.wait().expect("reap timed-out comparator")
    } else {
        loop {
            if let Some(status) = child.try_wait().expect("poll comparator") {
                break status;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                signal_group(process_group, "-TERM");
                thread::sleep(Duration::from_millis(100));
                signal_group(process_group, "-KILL");
                break child.wait().expect("reap timed-out comparator");
            }
            thread::sleep(Duration::from_millis(5));
        }
    };
    let elapsed = started.elapsed();
    Observation {
        status,
        stdout,
        stderr,
        elapsed,
        samples,
        ready,
        hot_done,
        finished,
        streams_closed: stdout_closed.max(stderr_closed),
        output_overflow: stdout_overflow || stderr_overflow,
        timed_out,
    }
}

fn signal_group(process_group: u32, signal: &str) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args([signal, &format!("-{process_group}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = (process_group, signal);
}

fn validate_output(label: &str, workload: Workload, bytes: &[u8]) -> Result<(), String> {
    let output = std::str::from_utf8(bytes)
        .map_err(|error| format!("{label} output is not UTF-8: {error}"))?;
    let lines: Vec<_> = output.lines().collect();
    if lines.first() != Some(&"ready") {
        return Err(format!("{label} missing ready"));
    }
    let expected = expected(workload);
    let expected_last = expected.lines().last().unwrap();
    if lines.last() != Some(&expected_last) {
        return Err(format!("{label} summary mismatch"));
    }
    if let Workload::Contention { probes, .. } = workload {
        let mut sample = 0_u64;
        let mut hot_done = 0;
        for line in &lines[1..lines.len() - 1] {
            if *line == "hot-done" {
                hot_done += 1;
            } else {
                if *line != format!("sample,{sample}") {
                    return Err(format!("{label} probe {sample} mismatch: {line}"));
                }
                sample += 1;
            }
        }
        if sample != probes {
            return Err(format!("{label} probe count {sample}, expected {probes}"));
        }
        if hot_done != 1 {
            return Err(format!("{label} hot completion count {hot_done}"));
        }
    } else if bytes != expected.as_bytes() {
        return Err(format!("{label} output mismatch"));
    }
    Ok(())
}

fn retain_failure(output: &Path, run_name: &str, observation: &Observation, reason: &str) {
    let failures = output.join("failures");
    fs::create_dir_all(&failures).expect("create failure evidence directory");
    fs::write(
        failures.join(format!("{run_name}.stdout")),
        &observation.stdout,
    )
    .unwrap();
    fs::write(
        failures.join(format!("{run_name}.stderr")),
        &observation.stderr,
    )
    .unwrap();
    fs::write(failures.join(format!("{run_name}.txt")), reason).unwrap();
}

fn checked(
    inputs: &Inputs,
    variant: Variant,
    schedulers: u64,
    workload: Workload,
    output: &Path,
    run_name: &str,
) -> Observation {
    let command = command(inputs, variant, schedulers, workload);
    let observation = observe(command, Duration::from_secs(180));
    let validation = validate_output(variant.label(), workload, &observation.stdout);
    let failure = if observation.timed_out {
        Some("comparator exceeded 180 second limit".to_owned())
    } else if observation.output_overflow {
        Some("comparator exceeded captured output limit".to_owned())
    } else if !observation.status.success() {
        Some(format!("comparator exited with {}", observation.status))
    } else if !observation.stderr.is_empty() {
        Some("comparator emitted unexpected stderr".to_owned())
    } else {
        validation.err()
    };
    if let Some(reason) = &failure {
        retain_failure(output, run_name, &observation, reason);
    }
    assert!(
        failure.is_none(),
        "{}: {}",
        variant.label(),
        failure.unwrap_or_default()
    );
    assert!(
        observation.ready.is_some(),
        "{} missing ready timestamp",
        variant.label()
    );
    assert!(
        observation.finished.is_some(),
        "{} missing summary timestamp",
        variant.label()
    );
    if matches!(workload, Workload::Contention { .. }) {
        assert!(
            observation.hot_done.is_some(),
            "{} missing hot completion timestamp",
            variant.label()
        );
    }
    observation
}

fn percentile(mut values: Vec<f64>, percentile: usize) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let rank = (percentile * values.len()).div_ceil(100).saturating_sub(1);
    values.get(rank).copied()
}

fn median(values: Vec<f64>) -> f64 {
    percentile(values, 50).expect("nonempty measured samples")
}

fn probe_statistics(
    observation: &Observation,
) -> (
    Option<f64>,
    Option<f64>,
    usize,
    Option<f64>,
    Option<f64>,
    Option<f64>,
) {
    let ready = observation.ready.expect("validated ready timestamp");
    let first = observation
        .samples
        .first()
        .map(|value| (*value - ready).as_secs_f64() * 1_000.0);
    let hot = observation
        .hot_done
        .map(|value| (value - ready).as_secs_f64() * 1_000.0);
    let intervals: Vec<_> = observation
        .samples
        .windows(2)
        .filter(|pair| observation.hot_done.map_or(true, |done| pair[1] <= done))
        .map(|pair| (pair[1] - pair[0]).as_secs_f64() * 1_000.0)
        .collect();
    (
        first,
        hot,
        intervals.len(),
        percentile(intervals.clone(), 50),
        percentile(intervals.clone(), 95),
        percentile(intervals, 99),
    )
}

fn decimal(value: Option<f64>) -> String {
    value.map_or_else(String::new, |number| format!("{number:.6}"))
}

fn hash(path: &Path) -> String {
    let output = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("hash benchmark input");
    assert!(output.status.success(), "cannot hash {}", path.display());
    String::from_utf8(output.stdout)
        .expect("shasum emits UTF-8")
        .split_whitespace()
        .next()
        .expect("shasum emits a digest")
        .to_owned()
}

fn collect_rust_sources(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read Rust source directory") {
        let path = entry.expect("read Rust source entry").path();
        if path.is_dir() {
            collect_rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn source_manifest(output: &Path) {
    let mut files = vec![
        PathBuf::from("Cargo.lock"),
        PathBuf::from("Cargo.toml"),
        PathBuf::from("rust-toolchain.toml"),
        PathBuf::from("crates/morrow/Cargo.toml"),
        PathBuf::from("crates/morrow-runtime/Cargo.toml"),
        PathBuf::from("crates/morrow-runtime-native/Cargo.toml"),
    ];
    for directory in [
        "crates/morrow/src",
        "crates/morrow-runtime/src",
        "crates/morrow-runtime-native/src",
    ] {
        collect_rust_sources(Path::new(directory), &mut files);
    }
    files.sort();
    files.dedup();

    let mut manifest = String::new();
    for path in files {
        assert!(path.is_file(), "source input missing: {}", path.display());
        manifest.push_str(&format!(
            "{}  {}  {}\n",
            hash(&path),
            fs::metadata(&path).unwrap().len(),
            path.display()
        ));
    }
    fs::write(output.join("source-manifest.sha256"), manifest).unwrap();
}

fn metadata(inputs: &Inputs, output: &Path, include_tuned: bool) {
    let mut text = format!(
        "Arguments: {:?}\nMORROW_REDUCTIONS baseline: 1\nOptional stealing reductions=32: {include_tuned}\nAmbient BEAM scheduler flags are replaced; ERL_FLAGS supplies explicit +S.\n",
        env::args().collect::<Vec<_>>()
    );
    let current_exe = env::current_exe().expect("find benchmark runner");
    for (label, path) in [
        ("Morrow workload", inputs.morrow.as_path()),
        ("Elixir launcher", inputs.elixir.as_path()),
        ("benchmark runner", current_exe.as_path()),
        ("Morrow compiler", Path::new("target/release/morrow")),
        (
            "Morrow runtime archive",
            Path::new("target/release/libmorrow_runtime_native.a"),
        ),
    ] {
        text.push_str(&format!(
            "{label}: {} bytes; {}; {}\n",
            fs::metadata(path).unwrap().len(),
            hash(path),
            path.display()
        ));
    }
    let mut beam_files: Vec<_> = fs::read_dir(&inputs.beam)
        .expect("read compiled BEAM directory")
        .map(|entry| entry.expect("read BEAM entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "beam")
        })
        .collect();
    beam_files.sort();
    assert!(!beam_files.is_empty(), "compiled BEAM directory is empty");
    for path in beam_files {
        text.push_str(&format!(
            "BEAM module: {} bytes; {}; {}\n",
            fs::metadata(&path).unwrap().len(),
            hash(&path),
            path.display()
        ));
    }
    for path in [
        format!("{ROOT}/programs/actors.mr"),
        format!("{ROOT}/programs/actors.ex"),
        format!("{ROOT}/src/actor_comparison.rs"),
    ] {
        let path = Path::new(&path);
        text.push_str(&format!(
            "benchmark source: {} bytes; {}; {}\n",
            fs::metadata(path).unwrap().len(),
            hash(path),
            path.display()
        ));
    }
    let mut version_command = Command::new(&inputs.elixir);
    version_command
        .arg("--version")
        .env_remove("ERL_FLAGS")
        .env_remove("ERL_AFLAGS")
        .env_remove("ELIXIR_ERL_OPTIONS");
    let version = observe(version_command, Duration::from_secs(10));
    assert!(!version.timed_out, "Elixir version command timed out");
    assert!(
        !version.output_overflow,
        "Elixir version output exceeded limit"
    );
    assert!(version.status.success(), "Elixir version command failed");
    text.push_str("\nElixir version:\n");
    text.push_str(&String::from_utf8_lossy(&version.stdout));
    text.push_str(&String::from_utf8_lossy(&version.stderr));
    let host = Command::new("/usr/bin/uname")
        .arg("-a")
        .output()
        .expect("read host identity");
    assert!(host.status.success());
    text.push_str("\nHost:\n");
    text.push_str(&String::from_utf8_lossy(&host.stdout));
    fs::write(output.join("inputs.txt"), text).unwrap();
    source_manifest(output);
}

fn verification(inputs: &Inputs, variants: &[Variant], output: &Path) {
    let mut count = 0;
    for schedulers in [1, 2, 4] {
        let workloads = [
            Workload::RequestReply {
                clients: 3,
                requests: 4,
            },
            Workload::Contention {
                schedulers,
                probes: 3,
                work: 2,
            },
            Workload::Lifecycle {
                workers: 3,
                faults: 2,
            },
        ];
        for variant in variants {
            for workload in workloads {
                let run_name = format!(
                    "verify-{}-{}-{schedulers}",
                    variant.label(),
                    workload.name()
                );
                checked(inputs, *variant, schedulers, workload, output, &run_name);
                count += 1;
            }
        }
    }
    fs::write(
        output.join("verification.txt"),
        format!("{count} independent exact-output process checks passed\n"),
    )
    .unwrap();
}

fn measure(inputs: &Inputs, variants: &[Variant], output: &Path) {
    let raw = output.join("raw");
    fs::create_dir(&raw).unwrap();
    let mut csv = fs::File::create(output.join("measurements.csv")).unwrap();
    writeln!(csv, "implementation,workload,schedulers,round,phase,operations,process_wall_ms,ready_to_completion_ms,operations_per_second,first_probe_after_ready_ms,hot_done_after_ready_ms,load_covered_probe_intervals,observed_completion_gap_p50_ms,observed_completion_gap_p95_ms,observed_completion_gap_p99_ms").unwrap();
    let mut summaries = Vec::new();
    for schedulers in [1, 2, 4] {
        let workloads = [
            Workload::RequestReply {
                clients: 32,
                requests: 500,
            },
            Workload::Contention {
                schedulers,
                probes: 257,
                work: 2_000_000,
            },
            Workload::Lifecycle {
                workers: 512,
                faults: 128,
            },
        ];
        for workload in workloads {
            let mut process_wall: Vec<Vec<f64>> = vec![Vec::new(); variants.len()];
            let mut elapsed: Vec<Vec<f64>> = vec![Vec::new(); variants.len()];
            let mut throughput: Vec<Vec<f64>> = vec![Vec::new(); variants.len()];
            let mut p99: Vec<Vec<f64>> = vec![Vec::new(); variants.len()];
            let mut covered: Vec<Vec<usize>> = vec![Vec::new(); variants.len()];
            for round in 0..6 {
                for offset in 0..variants.len() {
                    let index = (round + offset) % variants.len();
                    let variant = variants[index];
                    let stem = format!(
                        "{}-{}-{schedulers}-{round}",
                        variant.label(),
                        workload.name()
                    );
                    let observation = checked(inputs, variant, schedulers, workload, output, &stem);
                    let wall_milliseconds = observation.elapsed.as_secs_f64() * 1_000.0;
                    let ready = observation.ready.expect("validated ready timestamp");
                    let finished = observation.finished.expect("validated summary timestamp");
                    let workload_elapsed = if matches!(workload, Workload::Lifecycle { .. }) {
                        observation.streams_closed - ready
                    } else {
                        finished - ready
                    };
                    let milliseconds = workload_elapsed.as_secs_f64() * 1_000.0;
                    let operations_per_second =
                        workload.operations() as f64 / workload_elapsed.as_secs_f64();
                    let (first, hot, covered_intervals, p50, p95, observed_p99) =
                        probe_statistics(&observation);
                    let phase = if round == 0 { "warmup" } else { "measured" };
                    writeln!(
                        csv,
                        "{},{},{schedulers},{round},{phase},{},{wall_milliseconds:.6},{milliseconds:.6},{operations_per_second:.3},{},{},{covered_intervals},{},{},{}",
                        variant.label(),
                        workload.name(),
                        workload.operations(),
                        decimal(first),
                        decimal(hot),
                        decimal(p50),
                        decimal(p95),
                        decimal(observed_p99)
                    )
                    .unwrap();
                    csv.flush().unwrap();
                    fs::write(raw.join(format!("{stem}.stdout")), &observation.stdout).unwrap();
                    fs::write(raw.join(format!("{stem}.stderr")), &observation.stderr).unwrap();
                    let mut sample_csv = format!(
                        "event,index,observed_after_process_start_ns\nready,,{}\n",
                        ready.as_nanos()
                    );
                    for (sample, timestamp) in observation.samples.iter().enumerate() {
                        sample_csv.push_str(&format!("sample,{sample},{}\n", timestamp.as_nanos()));
                    }
                    if let Some(timestamp) = observation.hot_done {
                        sample_csv.push_str(&format!("hot-done,,{}\n", timestamp.as_nanos()));
                    }
                    sample_csv.push_str(&format!("summary,,{}\n", finished.as_nanos()));
                    sample_csv.push_str(&format!(
                        "streams-closed,,{}\n",
                        observation.streams_closed.as_nanos()
                    ));
                    fs::write(raw.join(format!("{stem}.samples.csv")), sample_csv).unwrap();
                    if round > 0 {
                        process_wall[index].push(wall_milliseconds);
                        elapsed[index].push(milliseconds);
                        throughput[index].push(operations_per_second);
                        covered[index].push(covered_intervals);
                        if let Some(value) = observed_p99 {
                            p99[index].push(value);
                        }
                    }
                }
            }
            for (index, variant) in variants.iter().enumerate() {
                let p99_median = (!p99[index].is_empty()).then(|| median(p99[index].clone()));
                let p99_max = p99[index].iter().copied().reduce(f64::max);
                let minimum_covered = covered[index].iter().copied().min().unwrap_or(0);
                summaries.push(format!(
                    "| {} | {schedulers} | {} | {:.3} | {:.3} | {:.0} | {minimum_covered} | {} | {} |",
                    workload.name(),
                    variant.label(),
                    median(process_wall[index].clone()),
                    median(elapsed[index].clone()),
                    median(throughput[index].clone()),
                    decimal(p99_median),
                    decimal(p99_max)
                ));
            }
        }
    }
    let mut summary = String::from(
        "| Workload | Schedulers | Implementation | Median process wall ms | Median ready-to-completion ms | Median operations/s | Minimum load-covered probe intervals | Median run p99 observed completion gap ms | Maximum run p99 observed completion gap ms |\n| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for row in summaries {
        summary.push_str(&row);
        summary.push('\n');
    }
    fs::write(output.join("summary.md"), &summary).unwrap();
    print!("{summary}");
}

fn main() {
    assert!(Path::new(ROOT).is_dir(), "run from the repository root");
    let arguments: Vec<_> = env::args().collect();
    assert!(
        (5..=7).contains(&arguments.len()),
        "actor_comparison MORROW_BINARY ELIXIR COMPILED_BEAM_DIRECTORY NEW_OUTPUT_DIRECTORY [--smoke] [--include-tuned-32]"
    );
    let smoke = arguments.iter().any(|argument| argument == "--smoke");
    let include_tuned = arguments
        .iter()
        .any(|argument| argument == "--include-tuned-32");
    for flag in arguments.iter().skip(5) {
        assert!(
            flag == "--smoke" || flag == "--include-tuned-32",
            "unknown option {flag}"
        );
    }
    let inputs = Inputs {
        morrow: fs::canonicalize(&arguments[1]).expect("Morrow binary must exist"),
        elixir: fs::canonicalize(&arguments[2]).expect("Elixir launcher must exist"),
        beam: fs::canonicalize(&arguments[3]).expect("compiled BEAM directory must exist"),
    };
    let output = Path::new(&arguments[4]);
    fs::create_dir(output).expect("new exclusive evidence directory");
    let mut variants = vec![
        Variant::Morrow {
            label: "morrow-pinned-r1",
            stealing: false,
            reductions: 1,
        },
        Variant::Morrow {
            label: "morrow-stealing-r1",
            stealing: true,
            reductions: 1,
        },
        Variant::Elixir,
    ];
    if include_tuned {
        variants.push(Variant::Morrow {
            label: "morrow-stealing-r32-experimental",
            stealing: true,
            reductions: 32,
        });
    }
    metadata(&inputs, output, include_tuned);
    verification(&inputs, &variants, output);
    if !smoke {
        measure(&inputs, &variants, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independently_known_outputs() {
        assert_eq!(
            expected(Workload::RequestReply {
                clients: 3,
                requests: 4,
            }),
            "ready\nrequest-reply,3,4,12,3600066\n"
        );
        assert_eq!(
            expected(Workload::Contention {
                schedulers: 2,
                probes: 3,
                work: 2,
            }),
            "ready\nsample,0\nsample,1\nsample,2\nhot-done\ncontention,3,2,9,1278240558\n"
        );
        assert_eq!(
            expected(Workload::Lifecycle {
                workers: 3,
                faults: 2,
            }),
            "ready\nlifecycle,3,2,3,4,54,2\n"
        );
        assert_eq!(
            expected(Workload::Lifecycle {
                workers: 3,
                faults: 0,
            }),
            "ready\nlifecycle,3,0,3,0,54,0\n"
        );
    }

    #[test]
    fn comparator_sources_exist() {
        assert!(Path::new(&format!("{ROOT}/programs/actors.mr")).is_file());
        assert!(Path::new(&format!("{ROOT}/programs/actors.ex")).is_file());
    }

    #[test]
    fn contention_accepts_hot_completion_before_probes() {
        let workload = Workload::Contention {
            schedulers: 2,
            probes: 3,
            work: 2,
        };
        validate_output(
            "test",
            workload,
            b"ready\nhot-done\nsample,0\nsample,1\nsample,2\ncontention,3,2,9,1278240558\n",
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retained_descendant_pipe_is_bounded() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & exit 0"]);
        let started = Instant::now();
        let observation = observe(command, Duration::from_millis(50));
        assert!(observation.timed_out);
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
