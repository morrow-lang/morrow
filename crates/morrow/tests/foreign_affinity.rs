//! Foreign calls pin scheduler affinity even when hidden in ordinary helpers.
use morrow_compiler::machine::{Operand, Operation, Statement};

#[test]
fn every_foreign_boundary_marks_affinity_before_invocation() {
    let source = r#"
foreign "C" fn absolute(value: Int) -> Int as "llabs"
fn helper(value: Int) -> Int:
    absolute(value)
fn main():
    println(helper(-17))
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let mut calls = 0;
    for function in &program.functions {
        for (index, statement) in function.body.iter().enumerate() {
            if matches!(
                statement,
                Statement::Assign {
                    operation: Operation::ForeignCall { .. },
                    ..
                } | Statement::Effect(Operation::ForeignCall { .. })
            ) {
                calls += 1;
                assert!(index > 0);
                assert!(matches!(&function.body[index - 1],
                    Statement::Effect(Operation::Call {
                        callee: Operand::Symbol(name), args, variadic: None,
                    }) if morrow_compiler::machine::bare(name) == "morrow_managed_pin_current"
                        && args.is_empty()));
            }
        }
    }
    assert!(calls > 0);
    let signature = morrow_compiler::runtime_abi::signature("morrow_managed_pin_current").unwrap();
    assert!(signature.params.is_empty());
    assert_eq!(signature.result, None);
    morrow_compiler::cranelift::emit_object(&program).unwrap();
}
