//! Native applications link checked Fern libraries without a compiler at runtime.
use fern_compiler::{
    check, cranelift,
    native_library::{self, Export},
    parse,
};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn checked(source: &str) -> fern_compiler::ir::Program {
    check::check_library(&parse::parse(source).unwrap()).unwrap()
}

#[test]
fn library_has_explicit_exports_without_a_process_entry() {
    let program = checked("pub fn increment(value: Int) -> Int: value + 1\n");
    let library =
        native_library::lower(&program, &[Export::new("increment", "increment")]).unwrap();
    assert!(!library.functions.iter().any(|f| f.name == "$fern_main"));
    let exports: Vec<_> = library.functions.iter().filter(|f| f.export).collect();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].name, "fern_export_increment");
    assert_eq!(
        exports[0].params.len(),
        3,
        "fault, execution context, source argument"
    );
    for target in ["aarch64-unknown-linux-musl", "x86_64-unknown-linux-musl"] {
        let bytes = cranelift::emit_object_for_target(&library, target).unwrap();
        assert_eq!(&bytes[..7], b"\x7fELF\x02\x01\x01");
        let expected = if target.starts_with("aarch64") {
            183
        } else {
            62
        };
        assert_eq!(u16::from_le_bytes([bytes[18], bytes[19]]), expected);
    }
}

#[test]
fn invalid_or_colliding_exports_reject_before_object_emission() {
    let program = checked("pub fn value() -> Int: 42\n");
    for exports in [
        vec![],
        vec![Export::new("absent", "value")],
        vec![Export::new("value", "bad-name")],
        vec![Export::new("value", "same"), Export::new("value", "same")],
    ] {
        assert!(native_library::lower(&program, &exports).is_err());
    }
    let library = native_library::lower(&program, &[Export::new("value", "value")]).unwrap();
    assert!(cranelift::emit_object_for_target(&library, "wasm32-unknown-unknown").is_err());
}

#[test]
fn linked_rust_host_preserves_full_width_values_and_observes_recoverable_faults() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "fern-library-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let program = checked(
        "pub fn increment(value: Int) -> Int: value + 1\npub fn divide(value: Int, divisor: Int) -> Int: value / divisor\n",
    );
    let library = native_library::lower(
        &program,
        &[
            Export::new("increment", "increment"),
            Export::new("divide", "divide"),
        ],
    )
    .unwrap();
    let object = directory.join("library.o");
    fs::write(&object, cranelift::emit_object(&library).unwrap()).unwrap();
    let source = directory.join("host.rs");
    fs::write(&source, r#"
unsafe extern "C" {
    fn fern_export_increment(fault: *mut i64, exec: usize, value: i64) -> i64;
    fn fern_export_divide(fault: *mut i64, exec: usize, value: i64, divisor: i64) -> i64;
}
fn main() {
    let mut fault = 0;
    unsafe {
        assert_eq!(fern_export_increment(&mut fault, 0, 9_007_199_254_740_993), 9_007_199_254_740_994);
        assert_eq!(fern_export_increment(&mut fault, 0, i64::MIN), i64::MIN + 1);
        assert_eq!(fault, 0);
        let _ = fern_export_divide(&mut fault, 0, 42, 0);
        assert_ne!(fault, 0);
        fault = 0;
        assert_eq!(fern_export_divide(&mut fault, 0, 84, 2), 42);
        assert_eq!(fault, 0);
    }
    println!("native library survived fault");
}
"#).unwrap();
    let archive = std::env::var_os("FERN_RUNTIME_CORE_LIB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            // Cargo may use an external target directory and hashed build layout.
            // Select this executable's prepared profile, as the native ABI suite does.
            std::env::current_exe()
                .unwrap()
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name == "debug" || name == "release")
                })
                .expect("Cargo test executable has a profile directory")
                .join("libfern_runtime.a")
        });
    assert!(
        archive.is_file(),
        "build fern-runtime before native library tests: {}",
        archive.display()
    );
    let executable = directory.join("host");
    let mut command = Command::new("rustc");
    command
        .args(["--edition=2024", "-Copt-level=2"])
        .arg(&source)
        .arg("-C")
        .arg(format!("link-arg={}", object.display()))
        .arg("-C")
        .arg(format!("link-arg={}", archive.display()));
    #[cfg(target_os = "linux")]
    command.arg("-C").arg("link-arg=-lc");
    let output = command.arg("-o").arg(&executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"native library survived fault\n");
}
