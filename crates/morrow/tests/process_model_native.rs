//! Native Process lifecycle oracles execute compiler output against the managed runtime.
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
            "morrow-process-model-native-{}-{}",
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
fn isolated_fault_monitors_and_copied_identities_preserve_typed_messages() {
    let source = include_str!("process_model/monitors.mr");
    let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_library_string_port(exec: usize) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_port_read(exec: usize, port: usize, output: *mut u8, capacity: usize) -> i64;
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize;
    fn morrow_gc_frame_leave(token: usize);
    fn morrow_gc_collect_precise();
}
fn main() {
    let mut fault = 0;
    unsafe {
        let exec = morrow_library_open(&mut fault);
        assert_ne!(exec, 0);
        let port = morrow_library_string_port(exec);
        let root = morrow_gc_frame_enter(&port, 1);
        morrow_export_start(&mut fault, exec, port);
        let mut received = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for step in 0..5000 {
            let status = morrow_managed_poll(exec, 1);
            // This host thread collects only its own domain. Worker heaps stay
            // owned by their schedulers; the native port remains explicitly rooted.
            morrow_gc_collect_precise();
            assert_eq!(fault, 0, "isolated actor fault escaped at poll {step}, status {status}");
            let mut bytes = [0u8; 128];
            let len = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if len >= 0 { received.push(String::from_utf8(bytes[..len as usize].to_vec()).unwrap()); }
            if received.len() == 5 { break; }
            assert!(std::time::Instant::now() < deadline, "message deadline: {received:?}");
            if len < 0 { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        assert_eq!(received.len(), 5, "{received:?}");
        // The observer's reports are causally ordered. The independent sibling
        // may appear anywhere; requiring a global cross-actor order would race.
        assert_eq!(received.iter().filter(|message| message.as_str() == "sibling").count(), 1);
        let observer: Vec<_> = received.iter().filter(|message| message.as_str() != "sibling").map(String::as_str).collect();
        assert_eq!(observer, ["identities", "wide=9007199254740993", "first", "second"]);
        morrow_managed_close(exec);
        morrow_gc_frame_leave(root);
    }
    println!("isolated, copied, ordered and rooted");
}
"#;
    execute_source(source, harness, b"isolated, copied, ordered and rooted\n");
}

fn execute_source(source: &str, harness: &str, expected: &[u8]) {
    let checked =
        morrow_compiler::check::check_library(&morrow_compiler::parse::parse(source).unwrap())
            .unwrap();
    let mut program = morrow_compiler::native_library::lower(
        &checked,
        &[morrow_compiler::native_library::Export::new(
            "start", "start",
        )],
    )
    .unwrap();
    // Execute collections in the callback's current owner domain, including
    // if ownership migrates. Existing generated native root slots retain every
    // staged argument before these explicit-context process call boundaries.
    let mut collection_points = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement,
                Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
                if matches!(machine::bare(name),
                    "morrow_process_spawn" | "morrow_process_spawn_monitor"
                    | "morrow_process_self" | "morrow_process_id"
                    | "morrow_process_monitor" | "morrow_process_demonitor"
                    | "morrow_process_receive_event"))
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
    assert!(
        collection_points > 0,
        "oracle must collect in actor callbacks"
    );
    NativeFixture::new().execute_linked(
        &program,
        harness,
        &[core_runtime_archive().into_os_string()],
        expected,
    );
}

#[test]
fn demonitor_flush_and_info_dead_processes_and_event_message_view() {
    // Pinned OTP oracle docs/process-model/monitor_reference.*: monitoring self
    // returns an inert reference, so demonitor with info reports false.
    let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_library_string_port(exec: usize) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_port_read(exec: usize, port: usize, output: *mut u8, capacity: usize) -> i64;
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize;
    fn morrow_gc_frame_leave(token: usize);
    fn morrow_gc_collect_precise();
}
fn main() {
    let mut fault = 0;
    unsafe {
        let exec = morrow_library_open(&mut fault);
        assert_ne!(exec, 0);
        let port = morrow_library_string_port(exec);
        let root = morrow_gc_frame_enter(&port, 1);
        morrow_export_start(&mut fault, exec, port);
        let mut received = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for step in 0..5000 {
            let status = morrow_managed_poll(exec, 1);
            // This host thread collects only its own domain. Worker heaps stay
            // owned by their schedulers; the native port remains explicitly rooted.
            morrow_gc_collect_precise();
            assert_eq!(fault, 0, "actor fault at poll {step}, status {status}");
            let mut bytes = [0u8; 128];
            let len = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if len >= 0 { received.push(String::from_utf8(bytes[..len as usize].to_vec()).unwrap()); }
            if received.len() == 7 { break; }
            assert!(std::time::Instant::now() < deadline, "message deadline: {received:?}");
            if len < 0 { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        assert_eq!(received, ["cancelled", "inactive", "normal", "flushed", "empty", "dead", "message"]);
        morrow_managed_close(exec);
        morrow_gc_frame_leave(root);
    }
    println!("demonitor and event views");
}
"#;
    execute_source(
        include_str!("process_model/demonitor.mr"),
        harness,
        b"demonitor and event views\n",
    );
}
