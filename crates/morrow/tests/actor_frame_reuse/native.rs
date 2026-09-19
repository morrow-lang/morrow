use super::*;

#[test]
fn private_self_tail_frame_keeps_owned_values_without_callback_heap_churn() {
    use morrow_compiler::native_library::{self, Export};
    let source = r#"
fn busy(remaining: Int, left: Int, right: Int, text: String):
    if remaining == 0:
        println(left)
        println(right)
        println(text)
    else:
        busy(remaining: remaining - 1, left: right, right: left, text: text)
pub fn start() -> ():
    let text = String.repeat("retained", 2)
    let first: Pid(()) = spawn(() -> busy(remaining: 513, left: -9223372036854775808, right: 9223372036854775807, text: text))
    let sibling: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check_library(&morrow_compiler::parse::parse(source).unwrap())
            .unwrap();
    let program = native_library::lower(&checked, &[Export::new("start", "start")]).unwrap();
    let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_heap_size() -> usize;
}
fn main() {
    let mut fault = 0;
    unsafe {
        let exec = morrow_library_open(&mut fault);
        assert_ne!(exec, 0);
        morrow_export_start(&mut fault, exec);
        for _ in 0..8 { assert_eq!(morrow_managed_poll(exec, 1), 2); }
        let before = morrow_gc_heap_size();
        assert_eq!(morrow_managed_poll(exec, 64), 2);
        let after = morrow_gc_heap_size();
        assert_eq!(after, before, "private scalar transitions must reuse their owned frame");
        assert_eq!(morrow_managed_poll(exec, 4096), 0);
        assert_eq!(fault, 0);
        morrow_managed_close(exec);
    }
}
"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"sibling\n9223372036854775807\n-9223372036854775808\nretainedretained\n"
    );
}

#[test]
fn changed_owned_captures_fall_back_once_in_written_argument_order_under_gc() {
    let source = r#"
fn effect(label: String, value: Int) -> Int:
    println(label)
    value
fn swap(remaining: Int, left: Int, right: Int, text: String):
    if remaining == 0:
        println(left)
        println(right)
        println(text)
    else:
        swap(right: effect("right", left), text: text + "x", remaining: remaining - 1, left: effect("left", right))
fn main():
    let ordinary = swap
    ordinary(2, -9223372036854775808, 9223372036854775807, "ordinary")
    let worker: Pid(()) = spawn(() -> swap(remaining: 3, left: -9223372036854775808, right: 9223372036854775807, text: "actor"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let mut reuse_calls = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "morrow_managed_continue_reuse")
            {
                reuse_calls += 1;
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
        reuse_calls > 0,
        "GC oracle must cover the staged reuse attempt"
    );
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }";
    assert_eq!(NativeFixture::new().execute_linked(&program, harness, &[core_runtime_archive().into_os_string()]),
        b"right\nleft\nright\nleft\n-9223372036854775808\n9223372036854775807\nordinaryxx\nright\nleft\nright\nleft\nright\nleft\n9223372036854775807\n-9223372036854775808\nactorxxx\n");
}

#[test]
fn checked_argument_fault_stops_before_frame_publication_or_later_effects() {
    use morrow_compiler::native_library::{self, Export};
    let source = r#"
fn effect(value: Int) -> Int:
    println("evaluated")
    value
fn busy(remaining: Int, left: Int, right: Int):
    if remaining > 0:
        busy(left: effect(right), right: 7 / (remaining - 1), remaining: effect(remaining - 1))
pub fn start() -> ():
    let worker: Pid(()) = spawn(() -> busy(remaining: 1, left: -9223372036854775808, right: 9223372036854775807))
    ()
"#;
    let checked =
        morrow_compiler::check::check_library(&morrow_compiler::parse::parse(source).unwrap())
            .unwrap();
    let program = native_library::lower(&checked, &[Export::new("start", "start")]).unwrap();
    let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_close(exec: usize);
}
fn main() {
    let mut fault = 0;
    unsafe {
        let exec = morrow_library_open(&mut fault);
        morrow_export_start(&mut fault, exec);
        assert_eq!(morrow_managed_poll(exec, 64), 3);
        assert_eq!(fault, 1);
        morrow_managed_close(exec);
    }
}
"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"evaluated\n"
    );
}

#[test]
fn cleanup_and_return_factory_frames_keep_ordinary_publication() {
    let source = r#"
fn scoped(remaining: Int):
    defer println(remaining)
    if remaining > 0: scoped(remaining - 1)
fn sum(remaining: Int) -> Int:
    if remaining == 0: 0
    else: remaining + sum(remaining - 1)
fn main():
    let scoped_actor: Pid(()) = spawn(() -> scoped(3))
    let value_actor: Pid(()) = spawn(() -> println(sum(4)))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    assert!(!program.functions.iter().flat_map(|function| &function.body).any(|statement|
        matches!(statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "morrow_managed_continue_reuse")),
        "escaping return factories and cleanup activations cannot use mutable frames");
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"0\n1\n2\n3\n10\n"
    );
}
