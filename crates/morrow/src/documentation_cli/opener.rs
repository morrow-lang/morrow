//! Best-effort platform launch after complete artifact publication, without shell expansion.
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Retain the artifact on launcher failure; diagnostics do not change generation's success status.
pub(super) fn open(path: &Path) {
    let result = path
        .canonicalize()
        .map_err(|error| error.to_string())
        .and_then(|path| {
            eprintln!("Generated docs: {}", path.display());
            let command = platform().ok_or("no platform documentation opener available")?;
            launch(command, &path, Duration::from_secs(10))
        });
    if let Err(error) = result {
        eprintln!(
            "note: could not open generated docs ({}): {error}",
            path.display()
        );
    }
}

/// Platform names are fixed executables resolved by PATH, never shell command strings.
fn platform() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("open")
    } else if cfg!(target_os = "linux") {
        Some("xdg-open")
    } else {
        None
    }
}

/// A literal absolute OS path is one argument; terminal input and captured-output growth are excluded.
fn launch(command: &str, path: &Path, timeout: Duration) -> Result<(), String> {
    let mut child = Command::new(command)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{command}: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{command} failed: {status}")),
            Err(error) => {
                stop(&mut child);
                return Err(format!("{command}: {error}"));
            }
            Ok(None) => {}
        }
        if started.elapsed() >= timeout {
            stop(&mut child);
            return Err(format!("{command} timed out; generated artifact retained"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Reap only the owned launcher; an independently launched browser is outside this ownership.
fn stop(child: &mut Child) {
    let _ = child.kill();
    // A kernel-uninterruptible process can delay this reap beyond the polling deadline.
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        ffi::OsString,
        fs,
        os::unix::{ffi::OsStringExt, fs::symlink},
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(helper: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "morrow-opener-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let link = path.join(if helper == "timeout" {
                "opener-timeout"
            } else {
                "opener"
            });
            let current = std::env::current_exe().unwrap();
            let profile = current
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name == "debug" || name == "release")
                })
                .unwrap();
            let source = profile.join("morrow-opener-fixture");
            assert!(
                source.is_file(),
                "build Rust supervisor fixtures before tests"
            );
            symlink(source, &link).unwrap();
            Self(path)
        }
        fn command(&self) -> String {
            self.0
                .join(if self.0.join("opener-timeout").exists() {
                    "opener-timeout"
                } else {
                    "opener"
                })
                .into_os_string()
                .into_string()
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn timeout_reaps_only_the_direct_launcher() {
        let fixture = Fixture::new("timeout");
        let error = launch(&fixture.command(), &fixture.0, Duration::from_millis(20)).unwrap_err();
        assert!(error.contains("timed out"));
        assert!(error.contains("retained"));
    }
    #[test]
    fn launcher_preserves_non_utf8_argument_without_filesystem_decoding() {
        let fixture = Fixture::new("capture");
        let path = PathBuf::from(OsString::from_vec(b"/literal-\xff.html".to_vec()));
        launch(&fixture.command(), &path, Duration::from_secs(1)).unwrap();
        assert_eq!(
            fs::read(fixture.0.join("opener.result")).unwrap(),
            b"/literal-\xff.html"
        );
    }
}
