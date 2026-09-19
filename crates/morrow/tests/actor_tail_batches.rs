//! Bounded pure recurrence paths must retain native ABI, roots and scheduler fairness.
use morrow_compiler::{cranelift, machine::Program};

struct NativeFixture(std::path::PathBuf);
impl NativeFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-tail-batch-native-{}-{}",
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
    ) -> Vec<u8> {
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
        let result = std::process::Command::new(executable)
            .env("MORROW_SCHEDULERS", "1")
            .env("MORROW_REDUCTIONS", "1")
            .env("MORROW_WORK_STEALING", "0")
            .output()
            .unwrap();
        assert!(result.status.success(), "native execution: {result:?}");
        assert!(result.stderr.is_empty(), "{result:?}");
        result.stdout
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
fn pure_tail_batches_complete_with_bounded_turns_and_full_width_values() {
    run_tail_oracle(
        r#"
fn sum(remaining: Int, value: Int, kept: String) -> Int:
    if remaining == 0: value + String.len(kept)
    else: sum(remaining: remaining - 1, value: value + 1, kept: kept)
fn busy(remaining: Int, value: Int, reply: Pid(String)):
    if remaining == 0:
        match send(reply, "done={value}"):
            Ok(()) -> ()
            Err(_) -> ()
    else: busy(remaining: remaining - 1, value: value + 1, reply: reply)
pub fn start(reply: Pid(String)) -> ():
    let wide = sum(remaining: 3, value: 9223372036854775805, kept: "abc")
    println(wide)
    let first: Pid(()) = spawn(() -> busy(remaining: 1024, value: 9223372036854775700, reply: reply))
    let second: Pid(()) = spawn(() ->
        let answer = sum(remaining: 1024, value: 9223372036854775700, kept: "abc" + "🌿")
        match send(reply, "sum={answer}"):
            Ok(()) -> ()
            Err(_) -> ()
    )
    let third: Pid(()) = spawn(() ->
        match send(reply, "sibling"):
            Ok(()) -> ()
            Err(_) -> ()
    )
    ()
"#,
    );
}

fn run_tail_oracle(source: &str) {
    use morrow_compiler::native_library::{self, Export};
    let checked =
        morrow_compiler::check::check_library(&morrow_compiler::parse::parse(source).unwrap())
            .unwrap();
    let program = native_library::lower(&checked, &[Export::new("start", "start")]).unwrap();
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
        let mut turns = 0;
        while turns < 900 && received.len() < 3 {
            morrow_managed_poll(exec, 1);
            turns += 1;
            morrow_gc_collect_precise();
            let mut bytes = [0u8; 128];
            let len = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if len >= 0 {
                received.push(String::from_utf8(bytes[..len as usize].to_vec()).unwrap());
                if received.len() == 1 {
                    assert_eq!(received[0], "sibling", "a bounded batch must yield to the sibling");
                    assert!(turns <= 10, "sibling delayed for {turns} turns");
                }
            }
        }
        assert_eq!(fault, 0);
        assert_eq!(received.len(), 3, "bounded batching must finish both recurrences within 900 turns; received {received:?}");
        assert!(turns >= 64, "1024 source iterations cannot run in unrestricted native loops: {turns}");
        received.sort();
        assert_eq!(received, ["done=-9223372036854774892", "sibling", "sum=-9223372036854774885"]);
        morrow_managed_close(exec);
        morrow_gc_frame_leave(root);
    }
    println!("bounded, rooted and fair");
}
"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"-9223372036854775805\nbounded, rooted and fair\n"
    );
}

#[test]
fn batched_parameter_swaps_and_scalar_faults_preserve_cleanup() {
    run_program(
        r#"
fn swap(remaining: Int, left: Int, right: Int) -> Int:
    if remaining == 0: left
    else:
        let next = left + 1
        swap(right: next, remaining: remaining - 1, left: right)
fn divide(remaining: Int, value: Int) -> Int:
    if remaining == 0: value
    else: divide(value: value / (remaining - 1000), remaining: remaining - 1)
fn broken():
    defer println("cleanup")
    println(divide(remaining: 1024, value: 9223372036854775807))
fn main():
    println(swap(remaining: 1025, left: 9223372036854775800, right: 9007199254740993))
    let failed: Pid(()) = supervise(broken, 0)
    let sibling: Pid(()) = spawn(() -> println("sibling"))
    let swapped: Pid(()) = spawn(() -> println(swap(remaining: 1025, left: 9223372036854775800, right: 9007199254740993)))
    ()
"#,
        b"9007199254741505\nsibling\ncleanup\n9007199254741505\n",
    );
}

#[test]
fn effectful_recursion_keeps_each_original_boundary_and_order() {
    run_program(
        r#"
fn effect(remaining: Int) -> Int:
    println(remaining)
    if remaining == 0: 7
    else: effect(remaining - 1)
fn main():
    let first: Pid(()) = spawn(() -> println(effect(4)))
    let sibling: Pid(()) = spawn(() -> println("sibling"))
    ()
"#,
        b"sibling\n4\n3\n2\n1\n0\n7\n",
    );
}

fn run_program(source: &str, expected: &[u8]) {
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = r#"unsafe extern "C" { fn morrow_main() -> i32; }
fn main() { assert_eq!(unsafe { morrow_main() }, 0); }"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        expected
    );
}
