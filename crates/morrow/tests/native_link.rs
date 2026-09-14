//! Sole-backend linker integration: Rust archive and system libraries only.
#![cfg(unix)]
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow-rust-link-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("source.fn"), "fn main(): ()\n").unwrap();
        fs::write(path.join("runtime ' λ.a"), b"archive fixture").unwrap();
        fs::write(path.join("linker.rs"),r#"use std::{env,fs,os::unix::ffi::OsStrExt};
fn main()->std::process::ExitCode {
 let args:Vec<_>=env::args_os().skip(1).collect();
 let mut bytes=Vec::new();for arg in &args{bytes.extend_from_slice(arg.as_bytes());bytes.push(0);}
 fs::write(env::var_os("LINK_LOG").unwrap(),bytes).unwrap();
 let object=fs::read(&args[0]).unwrap();assert!(object.starts_with(b"\x7fELF")||object.starts_with(&[0xcf,0xfa,0xed,0xfe]));
 let index=args.iter().position(|arg|arg=="-o").unwrap();fs::write(&args[index+1],b"linked artifact").unwrap();
 std::process::ExitCode::from(if env::var_os("FAIL_LINK").is_some(){17}else{0})
}"#).unwrap();
        let result = Command::new("rustc")
            .arg(path.join("linker.rs"))
            .arg("-o")
            .arg(path.join("linker"))
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        Self(path)
    }
    fn run(&self, fail: bool) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_morrow"));
        command
            .current_dir(&self.0)
            .args(["build", "source.fn", "-o", "output"])
            .env("PATH", &self.0)
            .env("CC", self.0.join("linker"))
            .env("MORROW_RUNTIME_LIB", self.0.join("runtime ' λ.a"))
            .env("LINK_LOG", self.0.join("log"));
        if fail {
            command.env("FAIL_LINK", "1");
        }
        command.output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn default_build_links_one_rust_archive_without_legacy_native_packages() {
    let f = Fixture::new();
    let result = f.run(false);
    assert!(result.status.success(), "{result:?}");
    let args = fs::read(f.0.join("log")).unwrap();
    let args: Vec<_> = args
        .split(|b| *b == 0)
        .filter(|arg| !arg.is_empty())
        .collect();
    assert_eq!(
        args[1],
        f.0.join("runtime ' λ.a").as_os_str().as_encoded_bytes()
    );
    for forbidden in [b"-lgc".as_slice(), b"-lsqlite3", b"-lssl", b"-lcrypto"] {
        assert!(
            !args.contains(&forbidden),
            "legacy native requirement {args:?}"
        );
    }
    let strip = if cfg!(target_os = "macos") {
        b"-Wl,-dead_strip".as_slice()
    } else {
        b"-Wl,--gc-sections".as_slice()
    };
    assert!(
        args.contains(&strip),
        "unused runtime code must be removed by the native linker"
    );
    assert_eq!(fs::read(f.0.join("output")).unwrap(), b"linked artifact");
}
#[test]
fn rust_object_link_failure_keeps_destination_and_cleans_owned_work() {
    let f = Fixture::new();
    fs::write(f.0.join("output"), b"retained").unwrap();
    let result = f.run(true);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("link failed"));
    assert_eq!(fs::read(f.0.join("output")).unwrap(), b"retained");
    assert!(!fs::read_dir(&f.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".morrow-rs-")
    }));
}
