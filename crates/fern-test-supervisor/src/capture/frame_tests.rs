use super::*;

fn frame(kind: &str, code: &str, out: &[u8], err: &[u8]) -> Vec<u8> {
    let mut bytes = format!("FERN_TEST 1 {kind} {code} {} {}\n", out.len(), err.len()).into_bytes();
    bytes.extend(out);
    bytes.extend(err);
    bytes.extend(b"\nFERN_TEST_END 1\n");
    bytes
}

#[test]
fn framed_binary_streams_and_native_125_are_preserved() {
    let result = decode(&frame("N", "32000", b"\0\xffout", b"\xfeerr")).unwrap();
    assert_eq!(result.status.code(), Some(125));
    assert_eq!(result.stdout, b"\0\xffout");
    assert_eq!(result.stderr, b"\xfeerr");
    assert!(
        decode(&frame("E", "3", b"", b""))
            .err()
            .unwrap()
            .contains("timed out")
    );
}

#[test]
fn protocol_rejects_noncanonical_domains_and_incomplete_records() {
    let valid = frame("N", "0", b"ok", b"err");
    for end in 0..valid.len() {
        assert!(decode(&valid[..end]).is_err(), "accepted truncation {end}");
    }
    for (kind, code) in [
        ("X", "0"),
        ("N", "00"),
        ("N", "+0"),
        ("N", "65536"),
        ("N", "2561"),
        ("N", "127"),
        ("E", "0"),
        ("E", "9"),
    ] {
        assert!(decode(&frame(kind, code, b"", b"")).is_err());
    }
    let mut extra = valid;
    extra.push(0);
    assert!(decode(&extra).is_err());
    assert!(decode(&frame("N", "0", &vec![0; OUTPUT_MAX + 1], b"")).is_err());
    assert!(decode(b"FERN_TEST 1 N 0 18446744073709551615 0\n").is_err());
}

#[test]
fn exact_caps_and_subsecond_rounding_are_bounded() {
    let result = decode(&frame("N", "0", &vec![0; OUTPUT_MAX], &vec![1; OUTPUT_MAX])).unwrap();
    assert_eq!(result.stdout.len(), OUTPUT_MAX);
    assert_eq!(result.stderr.len(), OUTPUT_MAX);
    assert_eq!(timeout_ms(Duration::from_nanos(1)).unwrap(), 1);
    assert_eq!(timeout_ms(Duration::from_micros(1001)).unwrap(), 2);
    assert_eq!(timeout_ms(Duration::from_secs(60)).unwrap(), 60000);
    assert!(timeout_ms(Duration::ZERO).is_err());
    assert!(timeout_ms(Duration::from_secs(60) + Duration::from_nanos(1)).is_err());
}

#[test]
fn transport_requires_helper_success_and_an_exact_complete_frame() {
    for body in [
        "printf 'FERN_TEST 1 N 0 0 0\\n\\nFERN_TEST_END 1\\n'; exit 125",
        "printf 'FERN_TEST 1 N 0 0 0\\n'",
        "printf garbage",
    ] {
        let mut helper = Command::new("/bin/sh");
        helper.args(["-c", body]);
        assert!(
            capture(
                Command::new("/usr/bin/true"),
                helper,
                Duration::from_secs(1)
            )
            .is_err()
        );
    }
    let mut helper = Command::new("/bin/sh");
    helper.args([
        "-c",
        "printf 'FERN_TEST 1 N 32000 0 0\\n\\nFERN_TEST_END 1\\n'",
    ]);
    assert_eq!(
        capture(
            Command::new("/usr/bin/true"),
            helper,
            Duration::from_secs(1)
        )
        .unwrap()
        .status
        .code(),
        Some(125)
    );
    assert!(
        capture(
            Command::new("/usr/bin/true"),
            Command::new("/missing/fern-supervisor"),
            Duration::from_secs(1)
        )
        .err()
        .unwrap()
        .contains("cannot execute test supervisor")
    );
}

#[test]
fn real_helper_preserves_signal_status_and_native_failure_codes() {
    for code in [0, 7, 125, 127, 255] {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &format!("exit {code}")]);
        assert_eq!(
            run_command(command, Duration::from_secs(2))
                .unwrap()
                .status
                .code(),
            Some(code)
        );
    }
    use std::os::unix::process::ExitStatusExt;
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "kill -TERM $$"]);
    assert_eq!(
        run_command(command, Duration::from_secs(2))
            .unwrap()
            .status
            .signal(),
        Some(15)
    );
}

#[test]
fn real_binary_caps_literal_arguments_and_closed_streams_use_same_adapter() {
    let mut command = Command::new(tests::fixture().unwrap());
    command.arg("streams");
    let result = run_command(command, Duration::from_secs(2)).unwrap();
    assert_eq!(result.stdout, b"out\0\xf0\x9f\x8c\xbf");
    assert_eq!(result.stderr, b"err\n");
    for stream in ["1", "2"] {
        let mut command = Command::new(tests::fixture().unwrap());
        command.args(["emit", "262144", stream]);
        let result = run_command(command, Duration::from_secs(2)).unwrap();
        assert_eq!(result.stdout.len() + result.stderr.len(), OUTPUT_MAX);
    }
    let mut command = Command::new(tests::fixture().unwrap());
    command.args(["args", "", "🌿;$HOME"]);
    assert_eq!(
        run_command(command, Duration::from_secs(2)).unwrap().stdout,
        "0:\n10:🌿;$HOME\n".as_bytes()
    );
    let mut command = Command::new(tests::fixture().unwrap());
    command.arg("close_wait");
    assert!(
        run_command(command, Duration::from_millis(100))
            .err()
            .unwrap()
            .contains("timed out")
    );
}

#[test]
fn escaped_output_holder_times_out_without_detached_rust_readers() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("FERN_CAPTURE_THREAD_PROBE").is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "frame_tests::escaped_output_holder_times_out_without_detached_rust_readers",
            ])
            .env("FERN_CAPTURE_THREAD_PROBE", "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let fixture = tests::fixture().unwrap();
    #[cfg(target_os = "linux")]
    let threads = std::fs::read_dir("/proc/self/task").unwrap().count();
    let workspace = Directory::new().unwrap();
    let started = std::time::Instant::now();
    for index in 0..3 {
        let mut command = Command::new(&fixture);
        command
            .arg("escaped")
            .arg(workspace.0.join(format!("ready{index}")));
        assert!(
            run_command(command, Duration::from_millis(150))
                .err()
                .unwrap()
                .contains("timed out")
        );
    }
    assert!(started.elapsed() < Duration::from_secs(3));
    #[cfg(target_os = "linux")]
    assert_eq!(
        std::fs::read_dir("/proc/self/task").unwrap().count(),
        threads
    );
}

#[test]
fn header_and_declared_length_boundaries_never_publish_partial_payloads() {
    for bytes in [
        b"FERN_TEST 1 N 0 18446744073709551615 1\n".as_slice(),
        b"FERN_TEST 1 N 0 1 18446744073709551615\n".as_slice(),
        b"FERN_TEST 1 N 0 10 0\nshort\nFERN_TEST_END 1\n".as_slice(),
        b"FERN_TEST 1 N 0 0 0\n\nFERN_TEST_END 1\nextra".as_slice(),
        b"FERN_TEST 1 N 0 0 0 extra\n\nFERN_TEST_END 1\n".as_slice(),
    ] {
        assert!(decode(bytes).is_err());
    }
    assert!(decode(&vec![b'x'; FRAME_MAX + 1]).is_err());
    for status in [128, 127, 65535, usize::MAX] {
        assert!(decode(&frame("N", &status.to_string(), b"", b"")).is_err());
    }
}

#[test]
fn complete_exit_status_domain_and_embedded_protocol_bytes_are_preserved() {
    for code in 0..=255 {
        let raw = code * 256;
        let captured = decode(&frame("N", &raw.to_string(), b"", b"")).unwrap();
        assert_eq!(captured.status.code(), Some(code));
    }
    let payload = b"FERN_TEST 1 N 0 0 0\n\nFERN_TEST_END 1\n\0\xff";
    let captured = decode(&frame("N", "0", payload, payload)).unwrap();
    assert_eq!(captured.stdout, payload);
    assert_eq!(captured.stderr, payload);
}
