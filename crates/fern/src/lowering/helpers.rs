//! Shared, typed builders for compiler-owned helpers and scalar representations.
//! Helper control flow is authored as structured Rust operations, never parsed from QBE text.
use super::*;

/// Interpret one compiler-owned SSA/symbol/constant identity, never an instruction or source string.
pub(super) fn native_operand(value: &str) -> Operand {
    Operand::named(value)
}

/// Convert the checked semantic representation to its native register class.
pub(super) fn machine_width(width: char) -> Scalar {
    Scalar::from_width(width)
}

/// Append the bounded actor fault tail native helper operations.
pub(super) fn actor_fault_tail(output: &mut Buffer) {
    output.statement(Statement::Branch {
        condition: native_operand("%is_6"),
        then_label: "@message_6".to_owned(),
        else_label: "@check_7".to_owned(),
    });
    output.statement(Statement::Label("@message_6".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_6")),
            (Scalar::I64, native_operand("77")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_7".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_7".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("7"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_7"),
        then_label: "@message_7".to_owned(),
        else_label: "@check_8".to_owned(),
    });
    output.statement(Statement::Label("@message_7".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_7")),
            (Scalar::I64, native_operand("61")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_8".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_8".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_8"),
        then_label: "@message_8".to_owned(),
        else_label: "@check_9".to_owned(),
    });
    output.statement(Statement::Label("@message_8".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_8")),
            (Scalar::I64, native_operand("77")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_9".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_9".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("9"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_9"),
        then_label: "@message_9".to_owned(),
        else_label: "@check_10".to_owned(),
    });
    output.statement(Statement::Label("@message_9".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_9")),
            (Scalar::I64, native_operand("51")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_10".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_10".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("10"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_10"),
        then_label: "@message_10".to_owned(),
        else_label: "@check_11".to_owned(),
    });
    output.statement(Statement::Label("@message_10".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_10")),
            (Scalar::I64, native_operand("74")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_11".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_11".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("11"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_11"),
        then_label: "@message_11".to_owned(),
        else_label: "@check_12".to_owned(),
    });
    output.statement(Statement::Label("@message_11".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_11")),
            (Scalar::I64, native_operand("56")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_12".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_12")),
            (Scalar::I64, native_operand("51")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.end();
    output.data(
        "$fern_rs_fault_8",
        vec![
            DataValue::Bytes(
                "fern: runtime error: actor timeout must be between 0 and 600000 milliseconds"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_9",
        vec![
            DataValue::Bytes(
                "fern: runtime error: actor resource limit exceeded"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_10",
        vec![
            DataValue::Bytes(
                "fern: runtime error: actor deadlock: no runnable actor or pending timeout"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_11",
        vec![
            DataValue::Bytes(
                "fern: runtime error: invalid actor execution descriptor"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_12",
        vec![
            DataValue::Bytes(
                "fern: runtime error: actor monotonic clock failure"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
}

/// Append the bounded control native helper operations.
pub(super) fn control(output: &mut Buffer) {
    output.begin(
        "$fern_rs_run_defers",
        None,
        vec![
            (Scalar::I64, "%headslot".to_owned()),
            (Scalar::I64, "%fault".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%saved".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::StackAlloc { bytes: 8, align: 8 },
    });
    output.statement(Statement::Assign {
        destination: "%original".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%original"),
        address: native_operand("%saved"),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%node".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%headslot")),
    });
    output.statement(Statement::Assign {
        destination: "%present".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Ne, Scalar::I64),
            native_operand("%node"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%present"),
        then_label: "@run".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@run".to_owned()));
    output.statement(Statement::Assign {
        destination: "%closure".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%node")),
    });
    output.statement(Statement::Assign {
        destination: "%previous_slot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%node"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%previous".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%previous_slot")),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%previous"),
        address: native_operand("%headslot"),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("0"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Assign {
        destination: "%code".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%closure")),
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("%code"),
        args: vec![
            (Scalar::I64, native_operand("%closure")),
            (Scalar::I64, native_operand("%fault")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Assign {
        destination: "%first".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%saved")),
    });
    output.statement(Statement::Assign {
        destination: "%had_first".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Ne, Scalar::I64),
            native_operand("%first"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%had_first"),
        then_label: "@loop".to_owned(),
        else_label: "@remember".to_owned(),
    });
    output.statement(Statement::Label("@remember".to_owned()));
    output.statement(Statement::Assign {
        destination: "%latest".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%latest"),
        address: native_operand("%saved"),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Assign {
        destination: "%primary".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%saved")),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%primary"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Return(None));
    output.end();
}

/// Append the bounded fault native helper operations.
pub(super) fn fault(output: &mut Buffer) {
    output.data(
        "$fern_rs_fault_1",
        vec![
            DataValue::Bytes(
                "fern: runtime error: integer division by zero"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_2",
        vec![
            DataValue::Bytes(
                "fern: runtime error: negative integer exponent"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_3",
        vec![
            DataValue::Bytes(
                "fern: runtime error: list index out of bounds"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_4",
        vec![
            DataValue::Bytes(
                "fern: runtime error: head of empty list"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_5",
        vec![
            DataValue::Bytes(
                "fern: runtime error: string size limit exceeded"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_6",
        vec![
            DataValue::Bytes(
                "fern: runtime error: String.slice indices must be UTF-8 character boundaries"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.data(
        "$fern_rs_fault_7",
        vec![
            DataValue::Bytes(
                "fern: runtime error: String.split requires valid UTF-8 input"
                    .as_bytes()
                    .to_vec(),
            ),
            DataValue::Bytes(vec![10]),
        ],
    );
    output.begin(
        "$fern_rs_report_fault",
        None,
        vec![(Scalar::I64, "%code".to_owned())],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_1".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_1"),
        then_label: "@message_1".to_owned(),
        else_label: "@check_2".to_owned(),
    });
    output.statement(Statement::Label("@message_1".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_1")),
            (Scalar::I64, native_operand("46")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_2".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_2".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("2"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_2"),
        then_label: "@message_2".to_owned(),
        else_label: "@check_3".to_owned(),
    });
    output.statement(Statement::Label("@message_2".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_2")),
            (Scalar::I64, native_operand("47")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_3".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_3".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("3"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_3"),
        then_label: "@message_3".to_owned(),
        else_label: "@check_4".to_owned(),
    });
    output.statement(Statement::Label("@message_3".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_3")),
            (Scalar::I64, native_operand("46")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_4".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_4".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("4"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_4"),
        then_label: "@message_4".to_owned(),
        else_label: "@check_5".to_owned(),
    });
    output.statement(Statement::Label("@message_4".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_4")),
            (Scalar::I64, native_operand("40")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_5".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_5".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("5"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_5"),
        then_label: "@message_5".to_owned(),
        else_label: "@check_6".to_owned(),
    });
    output.statement(Statement::Label("@message_5".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_5")),
            (Scalar::I64, native_operand("48")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_6".to_owned()));
    output.statement(Statement::Assign {
        destination: "%is_6".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%code"),
            native_operand("6"),
        ),
    });
}

/// Append the bounded fault tail native helper operations.
pub(super) fn fault_tail(output: &mut Buffer) {
    output.statement(Statement::Branch {
        condition: native_operand("%is_6"),
        then_label: "@message_6".to_owned(),
        else_label: "@check_7".to_owned(),
    });
    output.statement(Statement::Label("@message_6".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_6")),
            (Scalar::I64, native_operand("77")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@check_7".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$write"),
        args: vec![
            (Scalar::I32, native_operand("2")),
            (Scalar::I64, native_operand("$fern_rs_fault_7")),
            (Scalar::I64, native_operand("61")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.end();
}

/// Append the bounded float contains native helper operations.
pub(super) fn float_contains(output: &mut Buffer) {
    output.begin(
        "$fern_rs_list_contains_float",
        Some(Scalar::I32),
        vec![
            (Scalar::I64, "%list".to_owned()),
            (Scalar::F64, "%needle".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%length".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%list"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%index".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@start".to_owned(), native_operand("0")),
            ("@next".to_owned(), native_operand("%incremented")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%available".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%index"),
            native_operand("%length"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%available"),
        then_label: "@item".to_owned(),
        else_label: "@missing".to_owned(),
    });
    output.statement(Statement::Label("@item".to_owned()));
    output.statement(Statement::Assign {
        destination: "%raw".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%list")),
                (Scalar::I64, native_operand("%index")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%value".to_owned(),
        ty: Scalar::F64,
        operation: NativeOperation::Unary(MachineUnary::Cast, native_operand("%raw")),
    });
    output.statement(Statement::Assign {
        destination: "%equal".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::F64),
            native_operand("%value"),
            native_operand("%needle"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%equal"),
        then_label: "@found".to_owned(),
        else_label: "@next".to_owned(),
    });
    output.statement(Statement::Label("@next".to_owned()));
    output.statement(Statement::Assign {
        destination: "%incremented".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%index"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@found".to_owned()));
    output.statement(Statement::Return(Some(native_operand("1"))));
    output.statement(Statement::Label("@missing".to_owned()));
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.end();
}

/// Append the bounded iteration native helper operations.
pub(super) fn iteration(output: &mut Buffer) {
    output.begin(
        "$fern_rs_list_enumerate",
        Some(Scalar::I64),
        vec![(Scalar::I64, "%list".to_owned())],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%length".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%list"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%empty".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%length"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%extra".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Unary(MachineUnary::ExtUw, native_operand("%empty")),
    });
    output.statement(Statement::Assign {
        destination: "%capacity".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%length"),
            native_operand("%extra"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%output".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%capacity"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@head".to_owned()));
    output.statement(Statement::Label("@head".to_owned()));
    output.statement(Statement::Assign {
        destination: "%index".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@start".to_owned(), native_operand("0")),
            ("@body".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%available".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%index"),
            native_operand("%length"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%available"),
        then_label: "@body".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@body".to_owned()));
    output.statement(Statement::Assign {
        destination: "%raw".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%list")),
                (Scalar::I64, native_operand("%index")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%tuple".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_alloc"),
            args: vec![(Scalar::I64, native_operand("24"))],
            variadic: None,
        },
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("0"),
        address: native_operand("%tuple"),
    });
    output.statement(Statement::Assign {
        destination: "%islot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%tuple"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%index"),
        address: native_operand("%islot"),
    });
    output.statement(Statement::Assign {
        destination: "%vslot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%tuple"),
            native_operand("16"),
        ),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%raw"),
        address: native_operand("%vslot"),
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%output")),
            (Scalar::I64, native_operand("%tuple")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%index"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@head".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%output"))));
    output.end();
}

/// Append the bounded json native helper operations.
pub(super) fn json(output: &mut Buffer) {
    output.begin(
        "$fern_rs_json_object",
        Some(Scalar::I64),
        vec![(Scalar::I64, "%map".to_owned())],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%len".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%map"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%negative".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%len"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%large".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SGt, Scalar::I64),
            native_operand("%len"),
            native_operand("49999"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%bad".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Or,
            native_operand("%negative"),
            native_operand("%large"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%bad"),
        then_label: "@error".to_owned(),
        else_label: "@allocate".to_owned(),
    });
    output.statement(Statement::Label("@error".to_owned()));
    output.statement(Statement::Assign {
        destination: "%failure".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_json_value_limit_error"),
            args: vec![],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%failure"))));
    output.statement(Statement::Label("@allocate".to_owned()));
    output.statement(Statement::Assign {
        destination: "%empty".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%len"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%extra".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Unary(MachineUnary::ExtUw, native_operand("%empty")),
    });
    output.statement(Statement::Assign {
        destination: "%capacity".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%len"),
            native_operand("%extra"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%keys".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%capacity"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%values".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%capacity"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%i".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@allocate".to_owned(), native_operand("0")),
            ("@step".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%i"),
            native_operand("%len"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@read".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@read".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%i")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%key".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%pair")),
    });
    output.statement(Statement::Assign {
        destination: "%slot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%pair"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%value".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%slot")),
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%keys")),
            (Scalar::I64, native_operand("%key")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%values")),
            (Scalar::I64, native_operand("%value")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Jump("@step".to_owned()));
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%i"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Assign {
        destination: "%result".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_json_value_from_object"),
            args: vec![
                (Scalar::I64, native_operand("%keys")),
                (Scalar::I64, native_operand("%values")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%result"))));
    output.end();

    output.begin(
        "$fern_rs_json_members",
        Some(Scalar::I64),
        vec![(Scalar::I64, "%result".to_owned())],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%ok".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_result_is_ok"),
            args: vec![(Scalar::I64, native_operand("%result"))],
            variadic: None,
        },
    });
    output.statement(Statement::Branch {
        condition: native_operand("%ok"),
        then_label: "@allocate".to_owned(),
        else_label: "@failed".to_owned(),
    });
    output.statement(Statement::Label("@failed".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%result"))));
    output.statement(Statement::Label("@allocate".to_owned()));
    output.statement(Statement::Assign {
        destination: "%native".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_result_unwrap"),
            args: vec![(Scalar::I64, native_operand("%result"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%len".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%native"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%empty".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%len"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%extra".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Unary(MachineUnary::ExtUw, native_operand("%empty")),
    });
    output.statement(Statement::Assign {
        destination: "%capacity".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%len"),
            native_operand("%extra"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%list".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%capacity"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%i".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@allocate".to_owned(), native_operand("0")),
            ("@step".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%i"),
            native_operand("%len"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@read".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@read".to_owned()));
    output.statement(Statement::Assign {
        destination: "%member".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%native")),
                (Scalar::I64, native_operand("%i")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%key".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%member")),
    });
    output.statement(Statement::Assign {
        destination: "%slot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%member"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%value".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%slot")),
    });
    output.statement(Statement::Assign {
        destination: "%tuple".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_alloc"),
            args: vec![(Scalar::I64, native_operand("24"))],
            variadic: None,
        },
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("0"),
        address: native_operand("%tuple"),
    });
    output.statement(Statement::Assign {
        destination: "%first".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%tuple"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%key"),
        address: native_operand("%first"),
    });
    output.statement(Statement::Assign {
        destination: "%second".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%tuple"),
            native_operand("16"),
        ),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%value"),
        address: native_operand("%second"),
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%list")),
            (Scalar::I64, native_operand("%tuple")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Jump("@step".to_owned()));
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%i"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Assign {
        destination: "%converted".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_result_ok"),
            args: vec![(Scalar::I64, native_operand("%list"))],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%converted"))));
    output.end();
}

/// Append the bounded list access native helper operations.
pub(super) fn list_access(output: &mut Buffer) {
    output.begin(
        "$fern_rs_list_access",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%fault".to_owned()),
            (Scalar::I64, "%list".to_owned()),
            (Scalar::I64, "%index".to_owned()),
            (Scalar::I32, "%head".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%length".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%list"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%nonnegative".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SGe, Scalar::I64),
            native_operand("%index"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%within".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%index"),
            native_operand("%length"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%valid".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::And,
            native_operand("%nonnegative"),
            native_operand("%within"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%valid"),
        then_label: "@access".to_owned(),
        else_label: "@invalid".to_owned(),
    });
    output.statement(Statement::Label("@invalid".to_owned()));
    output.statement(Statement::Branch {
        condition: native_operand("%head"),
        then_label: "@empty_head".to_owned(),
        else_label: "@bad_index".to_owned(),
    });
    output.statement(Statement::Label("@empty_head".to_owned()));
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("4"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.statement(Statement::Label("@bad_index".to_owned()));
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("3"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.statement(Statement::Label("@access".to_owned()));
    output.statement(Statement::Branch {
        condition: native_operand("%head"),
        then_label: "@head".to_owned(),
        else_label: "@get".to_owned(),
    });
    output.statement(Statement::Label("@head".to_owned()));
    output.statement(Statement::Assign {
        destination: "%first".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_head"),
            args: vec![(Scalar::I64, native_operand("%list"))],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%first"))));
    output.statement(Statement::Label("@get".to_owned()));
    output.statement(Statement::Assign {
        destination: "%value".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%list")),
                (Scalar::I64, native_operand("%index")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%value"))));
    output.end();
}

/// Append the bounded maps native helper operations.
pub(super) fn maps(output: &mut Buffer) {
    output.begin(
        "$fern_rs_map_index_word",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%map".to_owned()),
            (Scalar::I64, "%key".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%len".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%map"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%i".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@start".to_owned(), native_operand("0")),
            ("@step".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%i"),
            native_operand("%len"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@read".to_owned(),
        else_label: "@absent".to_owned(),
    });
    output.statement(Statement::Label("@read".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%i")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%stored".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%pair")),
    });
    output.statement(Statement::Assign {
        destination: "%equal".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%key"),
            native_operand("%stored"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%equal"),
        then_label: "@found".to_owned(),
        else_label: "@step".to_owned(),
    });
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%i"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@found".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%i"))));
    output.statement(Statement::Label("@absent".to_owned()));
    output.statement(Statement::Return(Some(native_operand("-1"))));
    output.end();

    output.begin(
        "$fern_rs_map_index_string",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%map".to_owned()),
            (Scalar::I64, "%key".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%len".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%map"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%i".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@start".to_owned(), native_operand("0")),
            ("@step".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%i"),
            native_operand("%len"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@read".to_owned(),
        else_label: "@absent".to_owned(),
    });
    output.statement(Statement::Label("@read".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%i")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%stored".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%pair")),
    });
    output.statement(Statement::Assign {
        destination: "%equal".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_str_eq"),
            args: vec![
                (Scalar::I64, native_operand("%key")),
                (Scalar::I64, native_operand("%stored")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Branch {
        condition: native_operand("%equal"),
        then_label: "@found".to_owned(),
        else_label: "@step".to_owned(),
    });
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%i"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@found".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%i"))));
    output.statement(Statement::Label("@absent".to_owned()));
    output.statement(Statement::Return(Some(native_operand("-1"))));
    output.end();

    output.begin(
        "$fern_rs_map_pair",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%key".to_owned()),
            (Scalar::I64, "%value".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_alloc"),
            args: vec![(Scalar::I64, native_operand("16"))],
            variadic: None,
        },
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%key"),
        address: native_operand("%pair"),
    });
    output.statement(Statement::Assign {
        destination: "%slot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%pair"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%value"),
        address: native_operand("%slot"),
    });
    output.statement(Statement::Return(Some(native_operand("%pair"))));
    output.end();

    output.begin(
        "$fern_rs_map_literal_put",
        None,
        vec![
            (Scalar::I64, "%map".to_owned()),
            (Scalar::I64, "%key".to_owned()),
            (Scalar::I64, "%value".to_owned()),
            (Scalar::I64, "%index".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%found".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SGe, Scalar::I64),
            native_operand("%index"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%found"),
        then_label: "@replace".to_owned(),
        else_label: "@append".to_owned(),
    });
    output.statement(Statement::Label("@replace".to_owned()));
    output.statement(Statement::Assign {
        destination: "%existing".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%index")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%slot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%existing"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("%value"),
        address: native_operand("%slot"),
    });
    output.statement(Statement::Return(None));
    output.statement(Statement::Label("@append".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_rs_map_pair"),
            args: vec![
                (Scalar::I64, native_operand("%key")),
                (Scalar::I64, native_operand("%value")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%map")),
            (Scalar::I64, native_operand("%pair")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Return(None));
    output.end();

    output.begin(
        "$fern_rs_map_get",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%map".to_owned()),
            (Scalar::I64, "%index".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%found".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SGe, Scalar::I64),
            native_operand("%index"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%found"),
        then_label: "@present".to_owned(),
        else_label: "@absent".to_owned(),
    });
    output.statement(Statement::Label("@present".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%index")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%slot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%pair"),
            native_operand("8"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%payload".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%slot")),
    });
    output.statement(Statement::Assign {
        destination: "%some".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_result_ok"),
            args: vec![(Scalar::I64, native_operand("%payload"))],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%some"))));
    output.statement(Statement::Label("@absent".to_owned()));
    output.statement(Statement::Assign {
        destination: "%none".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_result_err"),
            args: vec![(Scalar::I64, native_operand("0"))],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%none"))));
    output.end();

    output.begin(
        "$fern_rs_map_put",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%map".to_owned()),
            (Scalar::I64, "%key".to_owned()),
            (Scalar::I64, "%value".to_owned()),
            (Scalar::I64, "%index".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%len".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%map"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%capacity".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%len"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%output".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%capacity"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%new".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_rs_map_pair"),
            args: vec![
                (Scalar::I64, native_operand("%key")),
                (Scalar::I64, native_operand("%value")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%i".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@start".to_owned(), native_operand("0")),
            ("@step".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%i"),
            native_operand("%len"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@choose".to_owned(),
        else_label: "@end".to_owned(),
    });
    output.statement(Statement::Label("@choose".to_owned()));
    output.statement(Statement::Assign {
        destination: "%replace".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%i"),
            native_operand("%index"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%replace"),
        then_label: "@replacement".to_owned(),
        else_label: "@original".to_owned(),
    });
    output.statement(Statement::Label("@replacement".to_owned()));
    output.statement(Statement::Jump("@copy".to_owned()));
    output.statement(Statement::Label("@original".to_owned()));
    output.statement(Statement::Assign {
        destination: "%old".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%i")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@copy".to_owned()));
    output.statement(Statement::Label("@copy".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@replacement".to_owned(), native_operand("%new")),
            ("@original".to_owned(), native_operand("%old")),
        ]),
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%output")),
            (Scalar::I64, native_operand("%pair")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Jump("@step".to_owned()));
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%i"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@end".to_owned()));
    output.statement(Statement::Assign {
        destination: "%absent".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%index"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%absent"),
        then_label: "@append".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@append".to_owned()));
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%output")),
            (Scalar::I64, native_operand("%new")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Jump("@done".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%output"))));
    output.end();

    output.begin(
        "$fern_rs_map_delete",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%map".to_owned()),
            (Scalar::I64, "%index".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%absent".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%index"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%absent"),
        then_label: "@unchanged".to_owned(),
        else_label: "@allocate".to_owned(),
    });
    output.statement(Statement::Label("@unchanged".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%map"))));
    output.statement(Statement::Label("@allocate".to_owned()));
    output.statement(Statement::Assign {
        destination: "%len".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%map"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%output".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%len"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%i".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@allocate".to_owned(), native_operand("0")),
            ("@step".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%i"),
            native_operand("%len"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@choose".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@choose".to_owned()));
    output.statement(Statement::Assign {
        destination: "%skip".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%i"),
            native_operand("%index"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%skip"),
        then_label: "@step".to_owned(),
        else_label: "@copy".to_owned(),
    });
    output.statement(Statement::Label("@copy".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%i")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%output")),
            (Scalar::I64, native_operand("%pair")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Jump("@step".to_owned()));
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%i"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%output"))));
    output.end();

    output.begin(
        "$fern_rs_map_project",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%map".to_owned()),
            (Scalar::I64, "%offset".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%len".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%map"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%capacity".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%len"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%output".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%capacity"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%i".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@start".to_owned(), native_operand("0")),
            ("@step".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%i"),
            native_operand("%len"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@copy".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@copy".to_owned()));
    output.statement(Statement::Assign {
        destination: "%pair".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%map")),
                (Scalar::I64, native_operand("%i")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%slot".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%pair"),
            native_operand("%offset"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%value".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Load(LoadKind::I64, native_operand("%slot")),
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%output")),
            (Scalar::I64, native_operand("%value")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Jump("@step".to_owned()));
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%i"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%output"))));
    output.end();
}

/// Append the bounded numeric native helper operations.
pub(super) fn numeric(output: &mut Buffer) {
    output.begin(
        "$fern_rs_int_div",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%fault".to_owned()),
            (Scalar::I64, "%a".to_owned()),
            (Scalar::I64, "%b".to_owned()),
            (Scalar::I32, "%remainder".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%zero".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%b"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%zero"),
        then_label: "@invalid".to_owned(),
        else_label: "@overflow_check".to_owned(),
    });
    output.statement(Statement::Label("@invalid".to_owned()));
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("1"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.statement(Statement::Label("@overflow_check".to_owned()));
    output.statement(Statement::Assign {
        destination: "%minimum".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%a"),
            native_operand("-9223372036854775808"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%negative_one".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%b"),
            native_operand("-1"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%overflow".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::And,
            native_operand("%minimum"),
            native_operand("%negative_one"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%overflow"),
        then_label: "@wrapped".to_owned(),
        else_label: "@normal".to_owned(),
    });
    output.statement(Statement::Label("@wrapped".to_owned()));
    output.statement(Statement::Branch {
        condition: native_operand("%remainder"),
        then_label: "@wrapped_rem".to_owned(),
        else_label: "@wrapped_div".to_owned(),
    });
    output.statement(Statement::Label("@wrapped_rem".to_owned()));
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.statement(Statement::Label("@wrapped_div".to_owned()));
    output.statement(Statement::Return(Some(native_operand(
        "-9223372036854775808",
    ))));
    output.statement(Statement::Label("@normal".to_owned()));
    output.statement(Statement::Branch {
        condition: native_operand("%remainder"),
        then_label: "@rem".to_owned(),
        else_label: "@div".to_owned(),
    });
    output.statement(Statement::Label("@rem".to_owned()));
    output.statement(Statement::Assign {
        destination: "%rest".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Rem,
            native_operand("%a"),
            native_operand("%b"),
        ),
    });
    output.statement(Statement::Return(Some(native_operand("%rest"))));
    output.statement(Statement::Label("@div".to_owned()));
    output.statement(Statement::Assign {
        destination: "%quotient".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Div,
            native_operand("%a"),
            native_operand("%b"),
        ),
    });
    output.statement(Statement::Return(Some(native_operand("%quotient"))));
    output.end();

    output.begin(
        "$fern_rs_int_pow",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%fault".to_owned()),
            (Scalar::I64, "%base".to_owned()),
            (Scalar::I64, "%exponent".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%negative".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%exponent"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%negative"),
        then_label: "@invalid".to_owned(),
        else_label: "@init".to_owned(),
    });
    output.statement(Statement::Label("@invalid".to_owned()));
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("2"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.statement(Statement::Label("@init".to_owned()));
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%x".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@init".to_owned(), native_operand("%base")),
            ("@step".to_owned(), native_operand("%square")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%n".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@init".to_owned(), native_operand("%exponent")),
            ("@step".to_owned(), native_operand("%half")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%acc".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@init".to_owned(), native_operand("1")),
            ("@step".to_owned(), native_operand("%updated")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%done".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%n"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%done"),
        then_label: "@end".to_owned(),
        else_label: "@bit".to_owned(),
    });
    output.statement(Statement::Label("@bit".to_owned()));
    output.statement(Statement::Assign {
        destination: "%odd".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::And,
            native_operand("%n"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%odd"),
        then_label: "@multiply".to_owned(),
        else_label: "@skip".to_owned(),
    });
    output.statement(Statement::Label("@multiply".to_owned()));
    output.statement(Statement::Assign {
        destination: "%product".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Mul,
            native_operand("%acc"),
            native_operand("%x"),
        ),
    });
    output.statement(Statement::Jump("@step".to_owned()));
    output.statement(Statement::Label("@skip".to_owned()));
    output.statement(Statement::Jump("@step".to_owned()));
    output.statement(Statement::Label("@step".to_owned()));
    output.statement(Statement::Assign {
        destination: "%updated".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@multiply".to_owned(), native_operand("%product")),
            ("@skip".to_owned(), native_operand("%acc")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%square".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Mul,
            native_operand("%x"),
            native_operand("%x"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%half".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Shr,
            native_operand("%n"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@end".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%acc"))));
    output.end();
}

/// Append the bounded pattern tail native helper operations.
pub(super) fn pattern_tail(output: &mut Buffer) {
    output.begin(
        "$fern_rs_pattern_tail",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%source".to_owned()),
            (Scalar::I64, "%offset".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%length".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_len"),
            args: vec![(Scalar::I64, native_operand("%source"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%remaining".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Sub,
            native_operand("%length"),
            native_operand("%offset"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%nonempty".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SGt, Scalar::I64),
            native_operand("%remaining"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%nonempty"),
        then_label: "@capacity".to_owned(),
        else_label: "@empty".to_owned(),
    });
    output.statement(Statement::Label("@capacity".to_owned()));
    output.statement(Statement::Jump("@allocate".to_owned()));
    output.statement(Statement::Label("@empty".to_owned()));
    output.statement(Statement::Jump("@allocate".to_owned()));
    output.statement(Statement::Label("@allocate".to_owned()));
    output.statement(Statement::Assign {
        destination: "%capacity".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@capacity".to_owned(), native_operand("%remaining")),
            ("@empty".to_owned(), native_operand("1")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%output".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_with_capacity"),
            args: vec![(Scalar::I64, native_operand("%capacity"))],
            variadic: None,
        },
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@loop".to_owned()));
    output.statement(Statement::Assign {
        destination: "%index".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Phi(vec![
            ("@allocate".to_owned(), native_operand("%offset")),
            ("@body".to_owned(), native_operand("%next")),
        ]),
    });
    output.statement(Statement::Assign {
        destination: "%more".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLt, Scalar::I64),
            native_operand("%index"),
            native_operand("%length"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%more"),
        then_label: "@body".to_owned(),
        else_label: "@done".to_owned(),
    });
    output.statement(Statement::Label("@body".to_owned()));
    output.statement(Statement::Assign {
        destination: "%item".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_list_get"),
            args: vec![
                (Scalar::I64, native_operand("%source")),
                (Scalar::I64, native_operand("%index")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Effect(NativeOperation::Call {
        callee: native_operand("$fern_list_push_mut"),
        args: vec![
            (Scalar::I64, native_operand("%output")),
            (Scalar::I64, native_operand("%item")),
        ],
        variadic: None,
    }));
    output.statement(Statement::Assign {
        destination: "%next".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::Add,
            native_operand("%index"),
            native_operand("1"),
        ),
    });
    output.statement(Statement::Jump("@loop".to_owned()));
    output.statement(Statement::Label("@done".to_owned()));
    output.statement(Statement::Return(Some(native_operand("%output"))));
    output.end();
}

/// Append the bounded repeat native helper operations.
pub(super) fn repeat(output: &mut Buffer) {
    output.data("$fern_rs_empty_string", vec![DataValue::Bytes(vec![0])]);

    output.begin(
        "$fern_rs_string_repeat",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%fault".to_owned()),
            (Scalar::I64, "%source".to_owned()),
            (Scalar::I64, "%count".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%nonpositive".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SLe, Scalar::I64),
            native_operand("%count"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%nonpositive"),
        then_label: "@empty".to_owned(),
        else_label: "@length".to_owned(),
    });
    output.statement(Statement::Label("@length".to_owned()));
    output.statement(Statement::Assign {
        destination: "%size".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_str_len"),
            args: vec![(Scalar::I64, native_operand("%source"))],
            variadic: None,
        },
    });
    output.statement(Statement::Assign {
        destination: "%is_empty".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::Eq, Scalar::I64),
            native_operand("%size"),
            native_operand("0"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%is_empty"),
        then_label: "@empty".to_owned(),
        else_label: "@limit".to_owned(),
    });
    output.statement(Statement::Label("@empty".to_owned()));
    output.statement(Statement::Return(Some(native_operand(
        "$fern_rs_empty_string",
    ))));
    output.statement(Statement::Label("@limit".to_owned()));
    output.statement(Statement::Assign {
        destination: "%allowed".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Binary(
            MachineBinary::UDiv,
            native_operand("16777216"),
            native_operand("%size"),
        ),
    });
    output.statement(Statement::Assign {
        destination: "%too_large".to_owned(),
        ty: Scalar::I32,
        operation: NativeOperation::Binary(
            MachineBinary::Compare(Comparison::SGt, Scalar::I64),
            native_operand("%count"),
            native_operand("%allowed"),
        ),
    });
    output.statement(Statement::Branch {
        condition: native_operand("%too_large"),
        then_label: "@invalid".to_owned(),
        else_label: "@repeat".to_owned(),
    });
    output.statement(Statement::Label("@invalid".to_owned()));
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("5"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.statement(Statement::Label("@repeat".to_owned()));
    output.statement(Statement::Assign {
        destination: "%value".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_str_repeat"),
            args: vec![
                (Scalar::I64, native_operand("%source")),
                (Scalar::I64, native_operand("%count")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%value"))));
    output.end();
}

/// Append the bounded slice native helper operations.
pub(super) fn slice(output: &mut Buffer) {
    output.begin(
        "$fern_rs_string_slice",
        Some(Scalar::I64),
        vec![
            (Scalar::I64, "%fault".to_owned()),
            (Scalar::I64, "%source".to_owned()),
            (Scalar::I64, "%start".to_owned()),
            (Scalar::I64, "%end".to_owned()),
        ],
        false,
    );
    output.statement(Statement::Label("@start".to_owned()));
    output.statement(Statement::Assign {
        destination: "%valid".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_str_slice_is_valid"),
            args: vec![
                (Scalar::I64, native_operand("%source")),
                (Scalar::I64, native_operand("%start")),
                (Scalar::I64, native_operand("%end")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Branch {
        condition: native_operand("%valid"),
        then_label: "@slice".to_owned(),
        else_label: "@invalid".to_owned(),
    });
    output.statement(Statement::Label("@invalid".to_owned()));
    output.statement(Statement::Store {
        kind: LoadKind::I64,
        value: native_operand("6"),
        address: native_operand("%fault"),
    });
    output.statement(Statement::Return(Some(native_operand("0"))));
    output.statement(Statement::Label("@slice".to_owned()));
    output.statement(Statement::Assign {
        destination: "%value".to_owned(),
        ty: Scalar::I64,
        operation: NativeOperation::Call {
            callee: native_operand("$fern_str_slice"),
            args: vec![
                (Scalar::I64, native_operand("%source")),
                (Scalar::I64, native_operand("%start")),
                (Scalar::I64, native_operand("%end")),
            ],
            variadic: None,
        },
    });
    output.statement(Statement::Return(Some(native_operand("%value"))));
    output.end();
}
