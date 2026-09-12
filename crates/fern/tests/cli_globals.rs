//! Literal global controls preserve data streams and explicit subcommand arguments.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern global flags {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("source.fn"), "fn main():println(42)\n").unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fern"))
            .current_dir(&self.0)
            .args(args)
            .env("FERN_QBE", self.0.join("absent backend"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn quiet_check_accepts_global_positions_and_preserves_errors() {
    let d = Directory::new();
    for args in [
        ["--quiet", "check", "source.fn"],
        ["check", "--quiet", "source.fn"],
        ["check", "source.fn", "--quiet"],
    ] {
        let out = d.run(&args);
        assert!(out.status.success(), "{out:?}");
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    }
    fs::write(d.0.join("bad.fn"), "fn main():unknown\n").unwrap();
    let out = d.run(&["--quiet", "check", "bad.fn"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bad.fn:1:"));
}
#[test]
fn verbose_reports_only_the_action_on_stderr() {
    let d = Directory::new();
    let out = d.run(&["check", "source.fn", "--verbose"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("No type errors"));
    assert_eq!(out.stderr, b"verbose: command=check\n");
}
#[test]
fn quiet_does_not_discard_ir_or_documents_or_literal_output_names() {
    let d = Directory::new();
    let ordinary = d.run(&["emit", "source.fn"]);
    let quiet = d.run(&["--quiet", "emit", "source.fn"]);
    assert!(quiet.status.success(), "{quiet:?}");
    assert_eq!(quiet.stdout, ordinary.stdout);
    let out = d.run(&["emit", "source.fn", "-o", "--quiet"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(d.0.join("--quiet")).unwrap(), ordinary.stdout);
    let ordinary = d.run(&["doc", "source.fn"]);
    let quiet = d.run(&["--quiet", "doc", "source.fn"]);
    assert!(quiet.status.success(), "{quiet:?}");
    assert_eq!(quiet.stdout, ordinary.stdout);
}
#[test]
fn color_policy_is_explicit_and_does_not_color_ir() {
    let d = Directory::new();
    fs::write(d.0.join("bad.fn"), "fn main():unknown\n").unwrap();
    for mode in ["--color=never", "--color=auto", "--color=always"] {
        let out = d.run(&[mode, "check", "bad.fn"]);
        assert!(!out.status.success());
        let ansi = out.stderr.windows(2).any(|s| s == b"\x1b[");
        assert_eq!(ansi, mode == "--color=always", "{out:?}");
        let ir = d.run(&[mode, "emit", "source.fn"]);
        assert!(ir.status.success(), "{ir:?}");
        assert!(!ir.stdout.contains(&27));
    }
    let out = d.run(&["--color=invalid", "check", "source.fn"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("color"));
}
#[test]
fn version_alias_and_missing_action_have_stable_status() {
    let d = Directory::new();
    let a = d.run(&["-v"]);
    let b = d.run(&["--version"]);
    assert!(a.status.success());
    assert_eq!(a.stdout, b.stdout);
    let out = d.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage:"));
}

#[test]
fn quiet_test_summaries_keep_failures_and_apply_requested_color() {
    let d = Directory::new();
    fs::write(d.0.join("empty.fn"), "fn helper():()\n").unwrap();
    for action in [vec!["test", "empty.fn"], vec!["test", "--doc", "empty.fn"]] {
        let ordinary = d.run(&action);
        assert!(ordinary.status.success(), "{ordinary:?}");
        assert!(String::from_utf8_lossy(&ordinary.stdout).contains("0/0 passed"));
        let mut quiet = vec!["--quiet"];
        quiet.extend(action);
        let output = d.run(&quiet);
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stdout.is_empty() && output.stderr.is_empty(),
            "{output:?}"
        );
    }
    fs::write(d.0.join("failed.fn"), "fn test_bad():false\n").unwrap();
    let out = d.run(&["--quiet", "--color=always", "test", "failed.fn"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("test_bad failed"));
    assert!(
        out.stderr.windows(2).any(|bytes| bytes == b"\x1b["),
        "{out:?}"
    );
}

#[test]
fn explicit_help_and_version_remain_visible_in_quiet_mode() {
    let d = Directory::new();
    for action in ["--help", "--version", "-v"] {
        let out = d.run(&["--quiet", action]);
        assert!(out.status.success(), "{out:?}");
        assert!(!out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    }
    let out = d.run(&["--quiet", "--verbose", "--color=never"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(out.stderr.starts_with(b"Usage:"), "{out:?}");
}

#[cfg(unix)]
impl Directory {
    /// Keep the process-boundary test independent of installed native libraries.
    fn native(&self, args: &[&str]) -> Output {
        use std::os::unix::fs::PermissionsExt;
        for (name, text) in [
            ("qbe", "#!/bin/sh\nprintf assembly > \"$2\"\n"),
            (
                "cc",
                "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n if [ \"$1\" = -o ]; then shift; out=$1; fi\n shift\ndone\ncp \"$FERN_TEST_EXECUTABLE\" \"$out\"\nchmod 700 \"$out\"\n",
            ),
            (
                "program",
                "#!/bin/sh\nprintf 'program stdout\\n'\nprintf 'program stderr\\n' >&2\nprintf '<%s>\\n' \"$@\"\nexit 17\n",
            ),
        ] {
            let path = self.0.join(name);
            fs::write(&path, text).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(self.0.join("runtime.a"), "stub").unwrap();
        Command::new(env!("CARGO_BIN_EXE_fern"))
            .current_dir(&self.0)
            .args(args)
            .env("FERN_QBE", self.0.join("qbe"))
            .env("FERN_RUNTIME_LIB", self.0.join("runtime.a"))
            .env("CC", self.0.join("cc"))
            .env("FERN_TEST_EXECUTABLE", self.0.join("program"))
            .output()
            .unwrap()
    }
}

#[cfg(unix)]
#[test]
fn quiet_run_preserves_program_streams_status_and_all_forwarded_controls() {
    let d = Directory::new();
    let out = d.native(&[
        "--quiet",
        "--color=always",
        "run",
        "source.fn",
        "--",
        "--quiet",
        "--verbose",
        "--color=invalid",
        "-v",
        "two words",
        "$(literal)",
        "",
    ]);
    assert_eq!(out.status.code(), Some(17), "{out:?}");
    assert_eq!(out.stderr, b"program stderr\n");
    assert_eq!(out.stdout, b"program stdout\n<--quiet>\n<--verbose>\n<--color=invalid>\n<-v>\n<two words>\n<$(literal)>\n<>\n");
}

#[cfg(unix)]
#[test]
fn quiet_build_hides_only_the_success_summary() {
    let d = Directory::new();
    let ordinary = d.native(&["build", "source.fn", "-o", "first"]);
    assert!(ordinary.status.success(), "{ordinary:?}");
    assert!(String::from_utf8_lossy(&ordinary.stdout).contains("Created executable: first"));
    let quiet = d.native(&["build", "source.fn", "--quiet", "-o", "--verbose"]);
    assert!(quiet.status.success(), "{quiet:?}");
    assert!(
        quiet.stdout.is_empty() && quiet.stderr.is_empty(),
        "{quiet:?}"
    );
    assert_eq!(
        fs::read(d.0.join("first")).unwrap(),
        fs::read(d.0.join("--verbose")).unwrap()
    );
}
