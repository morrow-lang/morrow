use super::*;
#[test]
#[ignore = "owned child fixture invoked by startup diagnostics tests"]
fn child_fixture() {
    use std::io::Write;
    match std::env::var("FERN_STARTUP_FIXTURE").as_deref() {
        Ok("exit") => {
            eprintln!("browser startup fixture: sandbox diagnostic marker");
            std::process::exit(7);
        }
        Ok("ready" | "hang") => {
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(&vec![b'x'; 3 * startup::LOG_BYTES as usize])
                .unwrap();
            stderr
                .write_all(b"\nlast browser diagnostic marker\n")
                .unwrap();
            stderr.flush().unwrap();
            fs::write(std::env::var_os("FERN_STARTUP_READY").unwrap(), "ready").unwrap();
            thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("fixture requires an explicit mode"),
    }
}

#[test]
fn early_browser_exit_reports_status_and_stderr_without_waiting_for_readiness_timeout() {
    let directory = crate::Temporary::new(&std::env::temp_dir()).unwrap();
    let ready = directory.0.join("DevToolsActivePort");
    let log = directory.0.join("browser.log");
    let child = Process::spawn(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "web::acceptance::startup_tests::child_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("FERN_STARTUP_FIXTURE", "exit")
            .stdout(Stdio::null())
            .stderr(fs::File::create(&log).unwrap()),
    )
    .unwrap();
    let started = Instant::now();
    let error = readiness(&child, &ready, &log, |_| None).unwrap_err();
    assert!(error.contains("sandbox diagnostic marker"), "{error}");
    assert!(error.contains("exit status 7"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

fn running_child(ready: &Path, log: &Path) -> Process {
    Process::spawn(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "web::acceptance::startup_tests::child_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("FERN_STARTUP_FIXTURE", "hang")
            .env("FERN_STARTUP_READY", ready)
            .stdout(Stdio::null())
            .stderr(fs::File::create(log).unwrap()),
    )
    .unwrap()
}
#[test]
fn timeout_preserves_bounded_stderr_tail_without_accepting_a_false_readiness_probe() {
    let directory = crate::Temporary::new(&std::env::temp_dir()).unwrap();
    let ready = directory.0.join("ready");
    let log = directory.0.join("browser.log");
    let child = running_child(&ready, &log);
    assert_eq!(
        readiness(&child, &ready, &log, |text| (text == "ready")
            .then(|| "ready".into()))
        .unwrap(),
        "ready"
    );
    let started = Instant::now();
    let error = startup::until(
        &child,
        &ready,
        &log,
        |_| None,
        Instant::now() + Duration::from_millis(50),
    )
    .unwrap_err();
    assert!(error.contains("readiness timeout"), "{error}");
    assert!(error.contains("last browser diagnostic marker"), "{error}");
    assert!(error.len() < startup::LOG_BYTES as usize + 1024);
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[test]
fn already_expired_deadline_never_publishes_a_ready_file() {
    let directory = crate::Temporary::new(&std::env::temp_dir()).unwrap();
    let ready = directory.0.join("ready");
    let log = directory.0.join("browser.log");
    let child = running_child(&ready, &log);
    readiness(&child, &ready, &log, |text| {
        (text == "ready").then(|| "ready".into())
    })
    .unwrap();
    let error = startup::until(
        &child,
        &ready,
        &log,
        |_| Some("invalid late success".into()),
        Instant::now(),
    )
    .unwrap_err();
    assert!(error.contains("readiness timeout"));
}
#[test]
fn nonregular_readiness_files_cannot_block_the_startup_deadline() {
    let directory = crate::Temporary::new(&std::env::temp_dir()).unwrap();
    let ready = directory.0.join("ready");
    let log = directory.0.join("browser.log");
    let child = running_child(&ready, &log);
    readiness(&child, &ready, &log, |text| {
        (text == "ready").then(|| "ready".into())
    })
    .unwrap();
    let linked = directory.0.join("linked");
    std::os::unix::fs::symlink(&ready, &linked).unwrap();
    let error = startup::until(
        &child,
        &linked,
        &log,
        |_| Some("bad linked readiness".into()),
        Instant::now() + Duration::from_millis(25),
    )
    .unwrap_err();
    assert!(error.contains("readiness timeout"));
}
