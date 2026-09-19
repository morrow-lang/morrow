//! Exact native observations exercise terminal source flow and the pinned OTP link cases.
#[path = "support/process_native.rs"]
mod support;

#[test]
fn terminal_exit_never_resumes_and_preserves_cleanup_order_and_full_width_reason() {
    support::execute_main(include_str!("process_links/terminal.mr"),
        b"before\nouter=0\ndown=shutdown\ninner-cleanup\nouter=1\ndown=failure:explicit\n42\nouter=2\ndown=normal\nouter=3\ndown=fault:1\nouter=4\ndown=kill\nouter=5\ndown=fault:-9223372036854775808\nouter=6\ndown=fault:9223372036854775807\nouter=7\ndown=failure:retained-7\nsettled\nouter=8\ndown=detail:owed-8\nforced=killed\n");
}

#[test]
fn typed_links_match_pinned_otp_observations() {
    support::execute_main(
        include_str!("process_links/links.mr"),
        include_bytes!("../../../docs/process-model/link_reference.expected"),
    );
}

#[test]
fn terminal_scope_leave_returns_before_publishing_any_successor() {
    // Independently inject the supported terminal status at an ordinary helper's
    // scope boundary. The runtime's control-delivery barrier test covers why a
    // normal scope leave may terminate; this oracle covers the compiler ABI.
    let source = r#"
fn helper() -> Unit:
    let own: Pid(()) = Process.self()
    defer println("inner")
    println("body")
fn child() -> Unit:
    defer println("outer")
    helper()
    println("resumed")
fn main():
    let started: Result(Pid(()), Process.Error) = Process.spawn(child)
    match started:
        Ok(_) -> ()
        Err(_) -> println("spawn-error")
"#;
    let harness = r#"
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
static TERMINATED: AtomicBool = AtomicBool::new(false);
static SUCCESSORS: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" {
    fn morrow_main() -> i32;
    fn morrow_managed_scope_leave(exec: i64) -> i64;
    fn morrow_managed_continue(exec: i64, next: i64) -> i64;
}
#[unsafe(no_mangle)]
unsafe extern "C" fn test_scope_leave(exec: i64) -> i64 {
    let status = unsafe { morrow_managed_scope_leave(exec) };
    if status == 0 && !TERMINATED.swap(true, Ordering::SeqCst) { 2 } else { status }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn test_continue(exec: i64, next: i64) -> i64 {
    if TERMINATED.load(Ordering::SeqCst) { SUCCESSORS.fetch_add(1, Ordering::SeqCst); }
    unsafe { morrow_managed_continue(exec, next) }
}
fn main() {
    assert_eq!(unsafe { morrow_main() }, 0);
    println!("successors={}", SUCCESSORS.load(Ordering::SeqCst));
}
"#;
    support::execute_main_with_patch(
        source,
        harness,
        b"body\ninner\nouter\nsuccessors=0\n",
        |program| {
            use morrow_compiler::machine::{self, Operand, Operation, Statement};
            for function in &mut program.functions {
                for statement in &mut function.body {
                    let operation = match statement {
                        Statement::Assign { operation, .. } | Statement::Effect(operation) => {
                            operation
                        }
                        _ => continue,
                    };
                    if let Operation::Call {
                        callee: Operand::Symbol(name),
                        args,
                        ..
                    } = operation
                    {
                        let symbol = match machine::bare(name) {
                            "morrow_managed_scope_leave" => "test_scope_leave",
                            "morrow_managed_continue" => "test_continue",
                            _ => continue,
                        };
                        *operation = Operation::ForeignCall {
                            declaration: morrow_compiler::ffi::Declaration {
                                symbol: symbol.into(),
                                library: None,
                                params: vec![morrow_compiler::ffi::AbiType::I64; args.len()],
                                result: morrow_compiler::ffi::AbiType::I64,
                            },
                            args: args.clone(),
                        };
                    }
                }
            }
        },
    );
}
