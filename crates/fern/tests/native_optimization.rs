//! Native optimization must remove repeated pure work without changing the ABI.
use fern_compiler::{cranelift, machine::*};

fn repeated_squares(count: usize) -> Program {
    let mut body = vec![Statement::Label("start".into())];
    let mut total = Operand::Int(0);
    for index in 0..count {
        let square = format!("square{index}");
        body.push(Statement::Assign {
            destination: square.clone(),
            ty: Scalar::I64,
            operation: Operation::Binary(
                BinaryOp::Mul,
                Operand::Temp("input".into()),
                Operand::Temp("input".into()),
            ),
        });
        let sum = format!("sum{index}");
        body.push(Statement::Assign {
            destination: sum.clone(),
            ty: Scalar::I64,
            operation: Operation::Binary(BinaryOp::Add, total, Operand::Temp(square)),
        });
        total = Operand::Temp(sum);
    }
    body.push(Statement::Return(Some(total)));
    Program {
        data: vec![],
        functions: vec![Function {
            name: "squares".into(),
            export: true,
            result: Some(Scalar::I64),
            params: vec![(Scalar::I64, "input".into())],
            body,
        }],
    }
}

#[test]
fn native_codegen_eliminates_repeated_pure_work() {
    // Compare objects from the same target, avoiding format-specific header sizes.
    // 128 identical products must not each survive as a multiply/add pair.
    // The generous bound permits uncombined additions and normal target padding.
    for target in ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"] {
        let one = cranelift::emit_object_for_target(&repeated_squares(1), target).unwrap();
        let many = cranelift::emit_object_for_target(&repeated_squares(128), target).unwrap();
        assert!(
            many.len() <= one.len() + 768,
            "{target}: repeated arithmetic grew by {} bytes",
            many.len() - one.len()
        );
    }
}
