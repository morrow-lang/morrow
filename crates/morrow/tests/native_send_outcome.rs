//! Immediate send matches execute compiler output against the managed runtime.
use morrow_compiler::{
    cranelift,
    machine::{self, Operand, Operation, Program, Statement},
};
struct NativeFixture(std::path::PathBuf);
impl NativeFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-send-outcome-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn execute_linked(
        &self,
        program: &Program,
        harness: &str,
        libraries: &[std::ffi::OsString],
        expected: &[u8],
    ) {
        let object = cranelift::emit_object(program).unwrap();
        let object_path = self.0.join("native.o");
        let harness_path = self.0.join("harness.rs");
        let executable = self.0.join("program");
        std::fs::write(&object_path, object).unwrap();
        std::fs::write(&harness_path, harness).unwrap();
        let mut command = std::process::Command::new("rustc");
        command
            .arg("--edition=2024")
            .arg("-Copt-level=2")
            .arg(&harness_path)
            .arg("-C")
            .arg(format!("link-arg={}", object_path.display()));
        for library in libraries {
            command
                .arg("-C")
                .arg(format!("link-arg={}", library.to_string_lossy()));
        }
        // rustc appends explicit archives after its system libraries. Revisit
        // libc's linker script so --as-needed can resolve native dependency
        // references introduced by the archive, including ARM stack protection.
        #[cfg(target_os = "linux")]
        if !libraries.is_empty() {
            command.arg("-C").arg("link-arg=-lc");
        }
        let result = command
            .arg("-o")
            .arg(&executable)
            .env_remove("LIBRARY_PATH")
            .output()
            .unwrap();
        assert!(result.status.success(), "native link: {result:?}");
        for schedulers in ["1", "2", "4"] {
            for stealing in ["0", "1"] {
                let configuration = format!("schedulers={schedulers}, stealing={stealing}");
                let mut child = std::process::Command::new(&executable)
                    .env("MORROW_SCHEDULERS", schedulers)
                    .env("MORROW_REDUCTIONS", "1")
                    .env("MORROW_WORK_STEALING", stealing)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
                let mut timed_out = false;
                while child.try_wait().unwrap().is_none() {
                    if std::time::Instant::now() >= deadline {
                        timed_out = true;
                        let _ = child.kill();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let result = child.wait_with_output().unwrap();
                assert!(!timed_out, "native timeout ({configuration}): {result:?}");
                assert!(
                    result.status.success(),
                    "native execution ({configuration}): {result:?}"
                );
                assert!(
                    result.stderr.is_empty(),
                    "native stderr ({configuration}): {result:?}"
                );
                assert_eq!(result.stdout, expected, "native output ({configuration})");
            }
        }
    }
}
impl Drop for NativeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn core_runtime_archive() -> std::path::PathBuf {
    let archive = std::env::var_os("MORROW_RUNTIME_CORE_LIB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name == "debug" || name == "release")
                })
                .unwrap()
                .join("libmorrow_runtime.a")
        });
    assert!(
        archive.is_file(),
        "build the Rust runtime archive first: {}",
        archive.display()
    );
    archive
}

#[test]
fn immediate_match_preserves_order_guards_roots_and_avoids_result_allocation() {
    let source = include_str!("send_outcome/immediate.mr");
    let checked =
        morrow_compiler::check::check_library(&morrow_compiler::parse::parse(source).unwrap())
            .unwrap();
    let exports = ["probe", "boxed", "aliased", "faulting", "wide"];
    let mut program = morrow_compiler::native_library::lower(
        &checked,
        &exports.map(|name| morrow_compiler::native_library::Export::new(name, name)),
    )
    .unwrap();
    let mut collection_points = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
                if matches!(machine::bare(name), "morrow_managed_send" | "morrow_managed_send_outcome" | "morrow_str_concat")
                    || machine::bare(name).strip_prefix('f').is_some_and(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit())))
            {
                collection_points += 1;
                body.push(Statement::Effect(Operation::Call {
                    callee: Operand::Symbol("morrow_gc_collect_precise".into()),
                    args: vec![],
                    variadic: None,
                }));
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert!(collection_points >= 3);
    NativeFixture::new().execute_linked(&program, include_str!("send_outcome/harness.rs"),
        &[core_runtime_archive().into_os_string()],
        b"pid\nmessage\nfirst\nsecond\npid\nmessage\nfirst\nsecond\npid\nmessage\nerror\npid-fault\ncleanup\npid-fault\nmessage-fault\ncleanup\npid-fault\nmessage-fault\nguard-fault\ncleanup\nordered, rooted, full-width and unboxed\n");
}
