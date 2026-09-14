use super::*;

fn scalar_function() -> Buffer {
    let mut buffer = Buffer::new();
    buffer.begin("answer", Some(Scalar::I64), vec![], true);
    buffer.statement(Statement::Label("start".into()));
    buffer.statement(Statement::Assign {
        destination: "value".into(),
        ty: Scalar::I64,
        operation: Operation::Binary(BinaryOp::Add, Operand::Int(40), Operand::Int(2)),
    });
    buffer.statement(Statement::Return(Some(Operand::Temp("value".into()))));
    buffer.end();
    buffer
}

#[test]
fn typed_scalar_function_renders_deterministically() {
    let program = scalar_function().finish().unwrap();
    assert_eq!(
        program.to_text(),
        "export function l $answer() {\n@start\n    %value =l add 40, 2\n    ret %value\n}\n\n"
    );
    assert_eq!(program.to_text(), program.to_text());
}

#[test]
fn unfinished_and_duplicate_functions_are_rejected() {
    let mut unfinished = Buffer::new();
    unfinished.begin("bad", None, vec![], false);
    assert!(unfinished.finish().unwrap_err().contains("unterminated"));
    let mut duplicate = scalar_function();
    duplicate.extend(&scalar_function());
    assert!(duplicate.finish().unwrap_err().contains("duplicate"));
}

#[test]
fn missing_control_target_and_unbound_values_are_rejected() {
    let mut buffer = Buffer::new();
    buffer.begin("bad", Some(Scalar::I64), vec![], false);
    buffer.statement(Statement::Label("start".into()));
    buffer.statement(Statement::Jump("missing".into()));
    buffer.end();
    assert!(buffer.finish().unwrap_err().contains("target"));
    let mut buffer = scalar_function();
    buffer.items[2] = Item::Statement(Statement::Assign {
        destination: "value".into(),
        ty: Scalar::I64,
        operation: Operation::Unary(UnaryOp::Copy, Operand::Temp("absent".into())),
    });
    assert!(buffer.finish().unwrap_err().contains("unknown temporary"));
}

#[test]
fn data_preserves_bytes_and_symbol_relocations() {
    let mut buffer = scalar_function();
    buffer.data(
        "text",
        vec![DataValue::Bytes(vec![b'a', b'"', b'\\', 0, 255])],
    );
    buffer.data(
        "table",
        vec![
            DataValue::Word(Operand::Symbol("text".into())),
            DataValue::Zero(8),
        ],
    );
    let program = buffer.finish().unwrap();
    assert!(program.to_text().starts_with(
        "data $text = { b \"a\", b 34, b 92, b 0, b 255 }\ndata $table = { l $text, z 8 }\n"
    ));
}

#[test]
fn instructions_cannot_follow_a_terminator() {
    let mut buffer = scalar_function();
    buffer.items.insert(4, Item::Statement(Statement::Trap));
    assert!(buffer.finish().unwrap_err().contains("terminator"));
}

#[test]
fn hoisted_stack_slot_is_inserted_at_entry() {
    let mut buffer = scalar_function();
    let mut slot = Buffer::new();
    slot.statement(Statement::Assign {
        destination: "slot".into(),
        ty: Scalar::I64,
        operation: Operation::StackAlloc { bytes: 8, align: 8 },
    });
    buffer.insert(2, &slot);
    let program = buffer.finish().unwrap();
    assert!(
        program
            .to_text()
            .contains("@start\n    %slot =l alloc8 8\n    %value")
    );
}

#[test]
fn ascii_data_runs_keep_qbe_string_compatibility() {
    let mut buffer = scalar_function();
    buffer.data("ascii", vec![DataValue::Bytes(vec![b'a'; 513])]);
    let text = buffer.finish().unwrap().to_text();
    assert!(text.contains(&format!("b \"{}\", b \"a\"", "a".repeat(512))));
}

fn loop_function() -> Buffer {
    let mut buffer = Buffer::new();
    buffer.begin("count", Some(Scalar::I64), vec![], false);
    buffer.statement(Statement::Label("start".into()));
    buffer.statement(Statement::Jump("loop".into()));
    buffer.statement(Statement::Label("loop".into()));
    buffer.statement(Statement::Assign {
        destination: "i".into(),
        ty: Scalar::I64,
        operation: Operation::Phi(vec![
            ("start".into(), Operand::Int(0)),
            ("step".into(), Operand::Temp("next".into())),
        ]),
    });
    buffer.statement(Statement::Assign {
        destination: "more".into(),
        ty: Scalar::I32,
        operation: Operation::Binary(
            BinaryOp::Compare(Comparison::SLt, Scalar::I64),
            Operand::Temp("i".into()),
            Operand::Int(10),
        ),
    });
    buffer.statement(Statement::Branch {
        condition: Operand::Temp("more".into()),
        then_label: "step".into(),
        else_label: "done".into(),
    });
    buffer.statement(Statement::Label("step".into()));
    buffer.statement(Statement::Assign {
        destination: "next".into(),
        ty: Scalar::I64,
        operation: Operation::Binary(BinaryOp::Add, Operand::Temp("i".into()), Operand::Int(1)),
    });
    buffer.statement(Statement::Jump("loop".into()));
    buffer.statement(Statement::Label("done".into()));
    buffer.statement(Statement::Return(Some(Operand::Temp("i".into()))));
    buffer.end();
    buffer
}

#[test]
fn loop_phis_accept_forward_values_and_exact_predecessors() {
    assert!(loop_function().finish().is_ok());
}

#[test]
fn phi_must_cover_actual_control_predecessors() {
    let mut buffer = loop_function();
    if let Item::Statement(Statement::Assign {
        operation: Operation::Phi(incoming),
        ..
    }) = &mut buffer.items[4]
    {
        incoming[1].0 = "done".into();
    }
    assert!(buffer.finish().unwrap_err().contains("phi predecessor"));
}

#[test]
fn phi_cannot_follow_an_ordinary_instruction() {
    let mut buffer = loop_function();
    buffer.items.swap(4, 5);
    assert!(buffer.finish().unwrap_err().contains("phi follows"));
}

#[test]
fn scalar_operations_reject_float_integer_confusion_before_backend() {
    for operation in [
        Operation::Binary(BinaryOp::And, Operand::Float(0), Operand::Float(0)),
        Operation::Unary(UnaryOp::Cast, Operand::Temp("word".into())),
    ] {
        let mut buffer = Buffer::new();
        buffer.begin(
            "bad",
            Some(Scalar::F64),
            vec![(Scalar::I32, "word".into())],
            false,
        );
        buffer.statement(Statement::Label("start".into()));
        buffer.statement(Statement::Assign {
            destination: "bad".into(),
            ty: Scalar::F64,
            operation,
        });
        buffer.statement(Statement::Return(Some(Operand::Temp("bad".into()))));
        buffer.end();
        assert!(buffer.finish().is_err());
    }
}

#[test]
fn float_branch_condition_is_rejected_before_integer_compare_builder() {
    let mut buffer = loop_function();
    buffer.items[6] = Item::Statement(Statement::Branch {
        condition: Operand::Float(0),
        then_label: "step".into(),
        else_label: "done".into(),
    });
    assert!(buffer.finish().is_err());
}

#[test]
fn same_block_use_before_definition_is_rejected() {
    let mut buffer = scalar_function();
    buffer.items.insert(
        2,
        Item::Statement(Statement::Assign {
            destination: "early".into(),
            ty: Scalar::I64,
            operation: Operation::Unary(UnaryOp::Copy, Operand::Temp("value".into())),
        }),
    );
    assert!(buffer.finish().unwrap_err().contains("dominate"));
}

#[test]
fn branch_local_value_cannot_escape_without_phi() {
    let mut buffer = Buffer::new();
    buffer.begin("bad", Some(Scalar::I64), vec![], false);
    buffer.statement(Statement::Label("start".into()));
    buffer.statement(Statement::Branch {
        condition: Operand::Int(1),
        then_label: "yes".into(),
        else_label: "no".into(),
    });
    buffer.statement(Statement::Label("yes".into()));
    buffer.statement(Statement::Assign {
        destination: "one".into(),
        ty: Scalar::I64,
        operation: Operation::Unary(UnaryOp::Copy, Operand::Int(1)),
    });
    buffer.statement(Statement::Jump("done".into()));
    buffer.statement(Statement::Label("no".into()));
    buffer.statement(Statement::Jump("done".into()));
    buffer.statement(Statement::Label("done".into()));
    buffer.statement(Statement::Return(Some(Operand::Temp("one".into()))));
    buffer.end();
    assert!(buffer.finish().unwrap_err().contains("dominate"));
}

#[test]
fn static_relocations_must_resolve_known_symbols() {
    let mut buffer = scalar_function();
    buffer.data(
        "bad",
        vec![DataValue::Word(Operand::Symbol("absent".into()))],
    );
    assert!(buffer.finish().unwrap_err().contains("symbol"));
}

#[test]
fn direct_call_must_match_canonical_signature() {
    let mut buffer = Buffer::new();
    buffer.begin("bad", None, vec![], false);
    buffer.statement(Statement::Label("start".into()));
    buffer.statement(Statement::Effect(Operation::Call {
        callee: Operand::Symbol("morrow_alloc".into()),
        args: vec![],
        variadic: None,
    }));
    buffer.statement(Statement::Return(None));
    buffer.end();
    assert!(buffer.finish().unwrap_err().contains("arity"));
}

#[test]
fn a_backedge_cannot_target_the_function_parameter_entry_block() {
    let mut buffer = Buffer::new();
    buffer.begin("bad", None, vec![(Scalar::I64, "arg".into())], false);
    buffer.statement(Statement::Label("start".into()));
    buffer.statement(Statement::Jump("start".into()));
    buffer.end();
    assert!(buffer.finish().unwrap_err().contains("entry"));
}

#[test]
fn dominance_work_is_bounded_across_the_module() {
    let mut buffer = Buffer::new();
    buffer.begin("bounded", Some(Scalar::I64), vec![], false);
    buffer.statement(Statement::Label("entry".into()));
    buffer.statement(Statement::Assign {
        destination: "origin".into(),
        ty: Scalar::I64,
        operation: Operation::Unary(UnaryOp::Copy, Operand::Int(1)),
    });
    buffer.statement(Statement::Jump("b0".into()));
    for index in 0..5000 {
        buffer.statement(Statement::Label(format!("b{index}")));
        buffer.statement(Statement::Assign {
            destination: format!("v{index}"),
            ty: Scalar::I64,
            operation: Operation::Unary(UnaryOp::Copy, Operand::Temp("origin".into())),
        });
        if index + 1 < 5000 {
            buffer.statement(Statement::Jump(format!("b{}", index + 1)));
        } else {
            buffer.statement(Statement::Return(Some(Operand::Temp("origin".into()))));
        }
    }
    buffer.end();
    assert!(
        buffer
            .finish()
            .unwrap_err()
            .contains("dominance work limit")
    );
}

#[test]
fn native_symbols_and_ssa_names_cannot_begin_with_digits() {
    let mut buffer = scalar_function();
    if let Item::Begin { name, .. } = &mut buffer.items[0] {
        *name = "123invalid".into();
    }
    assert!(buffer.finish().is_err());
}

#[test]
fn oversized_machine_stack_offsets_fail_before_native_layout() {
    let mut buffer = scalar_function();
    buffer.items.insert(
        2,
        Item::Statement(Statement::Assign {
            destination: "huge".into(),
            ty: Scalar::I64,
            operation: Operation::StackAlloc {
                bytes: u32::MAX,
                align: 16,
            },
        }),
    );
    assert!(
        buffer
            .finish()
            .unwrap_err()
            .contains("stack allocation limit")
    );
}

#[test]
fn nan_serialization_preserves_sign_payload_and_signaling_bits() {
    for (bits, raw) in [
        (0x7ff8_0000_0000_0123, "9221120237041090851"),
        (0xfff8_0000_0000_0456, "-2251799813684138"),
        (0x7ff0_0000_0000_0789, "9218868437227407241"),
    ] {
        let mut buffer = Buffer::new();
        buffer.data("bits", vec![DataValue::Word(Operand::Float(bits))]);
        buffer.begin("nan", Some(Scalar::F64), vec![], true);
        buffer.statement(Statement::Label("start".into()));
        buffer.statement(Statement::Return(Some(Operand::Float(bits))));
        buffer.end();
        let text = buffer.finish().unwrap().to_text();
        assert!(
            text.starts_with(&format!("data $bits = {{ l {raw} }}\n")),
            "{text}"
        );
        assert!(text.contains(&format!("    ret {raw}\n")), "{text}");
    }
}
