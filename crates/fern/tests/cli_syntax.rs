use std::{fs, process::Command};
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn invoke(action: &str, source: &[u8]) -> std::process::Output {
    invoke_controls(action, source, &["--quiet", "--color=always"])
}
fn invoke_controls(action: &str, source: &[u8], controls: &[&str]) -> std::process::Output {
    let work = std::env::temp_dir().join(format!(
        "fern-syntax-{}-{action}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&work).unwrap();
    let path = work.join("literal source.fn");
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fern"))
        .args(controls)
        .arg(action)
        .arg(&path)
        .env("FERN_QBE", "missing-backend")
        .output()
        .unwrap();
    fs::remove_dir_all(work).unwrap();
    output
}
#[test]
fn lex_exposes_actual_tokens_without_parsing_or_type_checking() {
    let output = invoke("lex", b"fn missing() +");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("0..2") && text.contains("Name(\"fn\")"),
        "{text}"
    );
    assert!(!text.contains('\u{1b}'));
}
#[test]
fn parse_exposes_unresolved_source_ast_without_loading_imports() {
    let source = b"import absent.dependency\nfn main():missing_name\n";
    fern_compiler::parse::parse(std::str::from_utf8(source).unwrap()).unwrap();
    let output = invoke("parse", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Program")
            && text.contains("missing_name")
            && text.contains("absent.dependency"),
        "{text}"
    );
    assert!(!text.contains('\u{1b}'));
}
#[test]
fn malformed_syntax_fails_atomically_with_a_location() {
    let output = invoke("parse", b"fn main(:\n");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("literal source.fn:1:"));
}

#[test]
fn lex_uses_byte_spans_and_escapes_control_payloads() {
    let source = "fn café():\"🌿\\n\\t\\r\\\"\"\n";
    let text = fern_compiler::parse::debug_tokens(source).unwrap();
    assert!(text.contains("3..8 Name(\"café\")"), "{text}");
    assert!(text.contains("🌿\\n\\t\\r\\\""), "{text}");
    let start = source.find('"').unwrap();
    let end = source.rfind('"').unwrap() + 1;
    assert!(text.contains(&format!("{start}..{end} Text(")), "{text}");
    for line in text.lines() {
        let span = line.split_once(' ').unwrap().0;
        let (start, end) = span.split_once("..").unwrap();
        let start: usize = start.parse().unwrap();
        let end: usize = end.parse().unwrap();
        assert!(start <= end && end <= source.len());
        assert!(source.is_char_boundary(start) && source.is_char_boundary(end));
        assert!(!line.contains('\0') && !line.contains('\t'));
    }
}

#[test]
fn valid_large_ast_hits_dump_budget_without_partial_stdout() {
    let mut source = String::new();
    for index in 0..220 {
        source.push_str(&format!("fn item{index}():{}1\n", "-".repeat(96)));
    }
    fern_compiler::parse::parse(&source).unwrap();
    fern_compiler::check::check_library(&fern_compiler::parse::parse(&source).unwrap()).unwrap();
    let error = fern_compiler::parse::debug_ast(&source).unwrap_err();
    assert!(error.message.contains("16 MiB"), "{}", error.message);
    let output = invoke("parse", source.as_bytes());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("16 MiB"));
}

#[test]
fn source_limit_precedes_truncated_utf8_read_errors() {
    let mut source = vec![b' '; 1024 * 1024];
    source.extend_from_slice("🌿".as_bytes());
    for action in ["lex", "parse"] {
        let output = invoke(action, &source);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("source size exceeds 1 MiB"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn token_and_depth_limits_fail_without_partial_dumps() {
    let tokens = "x ".repeat(65536);
    let deep = format!("fn main():{}1\n", "-".repeat(129));
    for (action, source, expected) in [
        ("lex", tokens, "token limit"),
        ("parse", deep, "depth limit"),
    ] {
        let output = invoke(action, source.as_bytes());
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
}

#[test]
fn exact_source_cap_is_accepted_and_invalid_utf8_is_not() {
    let mut source = vec![b' '; 1024 * 1024];
    source[0] = b'#';
    for action in ["lex", "parse"] {
        let valid = invoke(action, &source);
        assert!(valid.status.success(), "{valid:?}");
        assert!(!valid.stdout.is_empty());
        let invalid = invoke(action, b"fn main():\xff\n");
        assert!(!invalid.status.success());
        assert!(invalid.stdout.is_empty());
        assert!(String::from_utf8_lossy(&invalid.stderr).contains("UTF-8"));
    }
}

#[test]
fn terminal_controls_are_escaped_in_both_dumps() {
    for action in ["lex", "parse"] {
        let output = invoke(action, "fn main():\"\u{1b}\u{7}\"\n".as_bytes());
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("\\u{1b}") && text.contains("\\u{7}"),
            "{text}"
        );
        assert!(!text.contains('\u{1b}') && !text.contains('\u{7}'));
    }
}

#[test]
fn diagnostics_count_unicode_columns_and_crlf_lines() {
    let output = invoke("lex", "# 🌿\r\nfn café(): @\r\n".as_bytes());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("literal source.fn:2:12:"),
        "{output:?}"
    );
}

#[test]
fn inspection_cli_rejects_invalid_arguments_before_reading_or_writing() {
    for args in [
        vec!["lex"],
        vec!["parse"],
        vec!["lex", "missing.fn", "second.fn"],
        vec!["parse", "missing.fn", "-o", "output.fn"],
        vec!["lex", "--check", "missing.fn"],
        vec!["parse", "missing.fn", "--", "tail"],
        vec!["lex", "--unknown", "missing.fn"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_fern"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("No such file"));
    }
}

#[test]
fn missing_directory_and_oversized_paths_fail_without_stdout() {
    let missing = std::env::temp_dir().join(format!("fern-missing-syntax-{}", std::process::id()));
    for action in ["lex", "parse"] {
        for path in [
            missing.clone(),
            std::env::temp_dir(),
            std::path::PathBuf::from("x".repeat(4097)),
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_fern"))
                .arg(action)
                .arg(&path)
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(!output.stderr.is_empty());
            if path.as_os_str().len() > 4096 {
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("path exceeds 4096 bytes")
                );
            }
        }
    }
}

#[test]
fn lex_reports_real_delimiter_layout_errors_and_help_lists_commands() {
    let error = invoke("lex", b"fn main(): (\n");
    assert!(!error.status.success());
    assert!(error.stdout.is_empty());
    assert!(String::from_utf8_lossy(&error.stderr).contains("unclosed delimiter"));
    let help = Command::new(env!("CARGO_BIN_EXE_fern"))
        .args(["--quiet", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("lex") && text.contains("parse"));
}

#[test]
fn verbose_reports_action_only_and_never_changes_dump_bytes() {
    for action in ["lex", "parse"] {
        let source = b"fn main():unknown\n";
        let quiet = invoke(action, source);
        let verbose = invoke_controls(action, source, &["--verbose", "--color=always"]);
        assert!(quiet.status.success() && verbose.status.success());
        assert_eq!(quiet.stdout, verbose.stdout);
        assert_eq!(
            verbose.stderr,
            format!("verbose: command={action}\n").as_bytes()
        );
    }
}
