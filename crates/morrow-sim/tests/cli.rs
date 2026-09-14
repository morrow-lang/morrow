use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_morrow-sim"))
}

#[test]
fn json_cli_executes_the_native_application_and_reports_virtual_time() {
    let output = binary()
        .args([
            "--seed",
            "0x2a",
            "--steps",
            "3",
            "--clients",
            "1",
            "--rooms",
            "1",
            "--days",
            "1",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: morrow_sim::Report = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report.config.seed, 42);
    assert_eq!(report.config.duration_ms, 86_400_000);
    assert_eq!(report.counts.healing_commits, 1);
}

#[test]
fn unknown_duplicate_overflow_and_incompatible_arguments_fail() {
    for arguments in [
        vec!["--seed", "1", "--seed", "2"],
        vec!["--typo", "1"],
        vec!["--days", "18446744073709551615"],
        vec!["--drop-per-mille", "1001"],
        vec!["--replay", "missing.json", "--seed", "2"],
    ] {
        assert!(!binary().args(arguments).output().unwrap().status.success());
    }
}

struct ReplayFile(std::path::PathBuf);
impl ReplayFile {
    fn new(bytes: &[u8]) -> Self {
        use std::io::Write;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-sim-cli-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(bytes).unwrap();
        Self(path)
    }
    fn replay(&self) -> std::process::Output {
        binary()
            .arg("--replay")
            .arg(&self.0)
            .arg("--json")
            .output()
            .unwrap()
    }
}
impl Drop for ReplayFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn both_report_modes_replay_and_reject_tampering() {
    for flags in [
        vec!["--steps", "2", "--json"],
        vec!["--actors", "--seed", "42", "--steps", "8", "--json"],
    ] {
        let first = binary().args(flags).output().unwrap();
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let recorded = ReplayFile::new(&first.stdout);
        let replay = recorded.replay();
        assert!(
            replay.status.success(),
            "{}",
            String::from_utf8_lossy(&replay.stderr)
        );
        assert_eq!(first.stdout, replay.stdout);
        let mut value: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
        value["simulator_version"] = 999.into();
        assert!(
            !ReplayFile::new(&serde_json::to_vec(&value).unwrap())
                .replay()
                .status
                .success()
        );
    }
}

#[test]
fn failure_replay_preserves_failure_exit_and_full_reproducible_configuration() {
    let expected = morrow_sim::run(morrow_sim::Config {
        steps: 0,
        ..Default::default()
    })
    .unwrap_err();
    let output = ReplayFile::new(&serde_json::to_vec(&expected).unwrap()).replay();
    assert!(!output.status.success());
    let actual: morrow_sim::Failure = serde_json::from_slice(&output.stderr)
        .expect("a reproduced failure stays a machine-readable failure report");
    assert_eq!(actual, expected);
}

#[test]
fn replay_rejects_nonregular_and_oversized_inputs() {
    assert!(
        !binary()
            .arg("--replay")
            .arg(std::env::temp_dir())
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        !ReplayFile::new(&vec![b' '; 2 * 1024 * 1024 + 1])
            .replay()
            .status
            .success()
    );
}

#[test]
fn fifo_replay_is_rejected_without_waiting_for_a_writer() {
    let fixture = ReplayFile::new(b"");
    std::fs::remove_file(&fixture.0).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&fixture.0)
            .status()
            .unwrap()
            .success()
    );
    let output = fixture.replay();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("regular file"));
}

#[test]
fn malformed_duplicate_and_unknown_report_fields_are_rejected() {
    let report = morrow_sim::run(morrow_sim::Config {
        steps: 1,
        durable: false,
        ..Default::default()
    })
    .unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    let duplicate = format!("{{\"simulator_version\":1,{}", &encoded[1..]);
    let mut unknown = serde_json::to_value(&report).unwrap();
    unknown["unexpected"] = true.into();
    for bytes in [
        b"{}".to_vec(),
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{".to_vec(),
        duplicate.into_bytes(),
        serde_json::to_vec(&unknown).unwrap(),
    ] {
        assert!(!ReplayFile::new(&bytes).replay().status.success());
    }
}

#[test]
fn non_utf8_arguments_fail_without_a_panic() {
    use std::os::unix::ffi::OsStringExt;
    let output = binary()
        .arg(std::ffi::OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("arguments must be valid UTF-8"));
}
