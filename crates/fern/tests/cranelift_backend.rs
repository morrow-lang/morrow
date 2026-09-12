//! Backend acceptance keeps independent expected results and rejects malformed imports.
use fern_compiler::{
    cranelift,
    machine::{self, *},
};

fn scalar_program() -> Program {
    Program {
        data: vec![],
        functions: vec![Function {
            name: "main".into(),
            export: true,
            result: Some(Scalar::I32),
            params: vec![],
            body: vec![
                Statement::Label("start".into()),
                Statement::Return(Some(Operand::Int(0))),
            ],
        }],
    }
}

#[test]
fn emits_real_native_object_without_qbe() {
    let object = cranelift::emit_object(&scalar_program()).unwrap();
    assert!(object.len() > 100);
    #[cfg(target_os = "macos")]
    assert_eq!(&object[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
    #[cfg(target_os = "linux")]
    assert_eq!(&object[..4], b"\x7fELF");
}

#[test]
fn unknown_native_abi_is_rejected_without_guessing() {
    let mut p = scalar_program();
    p.functions[0].body.insert(
        1,
        Statement::Effect(Operation::Call {
            callee: Operand::Symbol("unknown_external".into()),
            args: vec![],
            variadic: None,
        }),
    );
    let error = cranelift::emit_object(&p).unwrap_err();
    assert!(error.contains("unknown_external"), "{error}");
}

#[test]
fn malformed_machine_program_is_rejected_before_codegen() {
    let mut p = scalar_program();
    p.functions[0]
        .body
        .insert(1, Statement::Jump("missing".into()));
    assert!(cranelift::emit_object(&p).is_err());
}

#[test]
fn fixed_runtime_abi_preserves_real_result_width() {
    let signature = fern_compiler::runtime_abi::signature("fern_result_is_ok").unwrap();
    assert_eq!(signature.result, Some(machine::Scalar::I64));
}

/// Native acceptance invokes only the host linker and the generated object, never QBE.
struct NativeFixture(std::path::PathBuf);
impl NativeFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fern-cranelift-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn execute(&self, program: &Program, harness: &str) -> Vec<u8> {
        self.execute_linked(program, harness, &[])
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
        let result = std::process::Command::new(executable).output().unwrap();
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

fn temp(name: &str) -> Operand {
    Operand::Temp(name.into())
}
fn symbol(name: &str) -> Operand {
    Operand::Symbol(name.into())
}
fn assign(name: &str, ty: Scalar, operation: Operation) -> Statement {
    Statement::Assign {
        destination: name.into(),
        ty,
        operation,
    }
}
fn probe(body: Vec<Statement>, params: Vec<(Scalar, String)>) -> Function {
    Function {
        name: "probe".into(),
        export: true,
        result: Some(Scalar::I64),
        params,
        body,
    }
}

#[test]
fn parallel_loop_phis_preserve_previous_iteration_full_width_values() {
    use Scalar::{I32, I64};
    let program = Program {
        data: vec![],
        functions: vec![probe(
            vec![
                Statement::Label("start".into()),
                Statement::Jump("loop".into()),
                Statement::Label("loop".into()),
                assign(
                    "count",
                    I64,
                    Operation::Phi(vec![
                        ("start".into(), Operand::Int(0)),
                        ("back".into(), temp("next")),
                    ]),
                ),
                assign(
                    "left",
                    I64,
                    Operation::Phi(vec![
                        ("start".into(), Operand::Int(i64::MIN + 17)),
                        ("back".into(), temp("right")),
                    ]),
                ),
                assign(
                    "right",
                    I64,
                    Operation::Phi(vec![
                        ("start".into(), Operand::Int(i64::MAX - 23)),
                        ("back".into(), temp("left")),
                    ]),
                ),
                assign(
                    "done",
                    I32,
                    Operation::Binary(
                        BinaryOp::Compare(Comparison::Eq, I64),
                        temp("count"),
                        Operand::Int(101),
                    ),
                ),
                Statement::Branch {
                    condition: temp("done"),
                    then_label: "end".into(),
                    else_label: "back".into(),
                },
                Statement::Label("back".into()),
                assign(
                    "next",
                    I64,
                    Operation::Binary(BinaryOp::Add, temp("count"), Operand::Int(1)),
                ),
                Statement::Jump("loop".into()),
                Statement::Label("end".into()),
                Statement::Return(Some(temp("left"))),
            ],
            vec![],
        )],
    };
    let harness = "unsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"{}\", unsafe { probe() }); }\n";
    assert_eq!(
        NativeFixture::new().execute(&program, harness),
        b"9223372036854775784\n"
    );
}

#[test]
fn mixed_integer_float_abi_crosses_both_register_banks_and_stack_arguments() {
    use Scalar::{F64, I32, I64};
    let mut params = Vec::new();
    let mut body = vec![Statement::Label("start".into())];
    let mut rust_parameters = Vec::new();
    let mut rust_arguments = Vec::new();
    for index in 0..10 {
        let integer = i64::MAX - index;
        let float = index as f64 + 0.125;
        params.push((I64, format!("i{index}")));
        params.push((F64, format!("f{index}")));
        rust_parameters.extend([format!("i{index}: i64"), format!("f{index}: f64")]);
        rust_arguments.push(format!("{integer}i64, {float}f64"));
        body.push(assign(
            &format!("int_ok{index}"),
            I32,
            Operation::Binary(
                BinaryOp::Compare(Comparison::Eq, I64),
                temp(&format!("i{index}")),
                Operand::Int(integer),
            ),
        ));
        body.push(assign(
            &format!("float_ok{index}"),
            I32,
            Operation::Binary(
                BinaryOp::Compare(Comparison::Eq, F64),
                temp(&format!("f{index}")),
                Operand::Float(float.to_bits()),
            ),
        ));
        body.push(assign(
            &format!("pair{index}"),
            I32,
            Operation::Binary(
                BinaryOp::And,
                temp(&format!("int_ok{index}")),
                temp(&format!("float_ok{index}")),
            ),
        ));
        let previous = if index == 0 {
            Operand::Int(1)
        } else {
            temp(&format!("all{}", index - 1))
        };
        body.push(assign(
            &format!("all{index}"),
            I32,
            Operation::Binary(BinaryOp::And, previous, temp(&format!("pair{index}"))),
        ));
    }
    body.extend([
        Statement::Branch {
            condition: temp("all9"),
            then_label: "ok".into(),
            else_label: "bad".into(),
        },
        Statement::Label("ok".into()),
        Statement::Return(Some(Operand::Int(0x0123_4567_89ab_cdef))),
        Statement::Label("bad".into()),
        Statement::Return(Some(Operand::Int(-1))),
    ]);
    let program = Program {
        data: vec![],
        functions: vec![probe(body, params)],
    };
    let harness = format!(
        "unsafe extern \"C\" {{ fn probe({}) -> i64; }}\nfn main() {{ println!(\"{{}}\", unsafe {{ probe({}) }}); }}\n",
        rust_parameters.join(", "),
        rust_arguments.join(", ")
    );
    assert_eq!(
        NativeFixture::new().execute(&program, &harness),
        b"81985529216486895\n"
    );
}

#[test]
fn immutable_relocations_support_internal_function_and_data_addresses() {
    use Scalar::I64;
    let program = Program {
        data: vec![
            Data {
                name: "payload".into(),
                values: vec![DataValue::Word(Operand::Int(i64::MIN + 91))],
            },
            Data {
                name: "descriptor".into(),
                values: vec![
                    DataValue::Word(symbol("identity")),
                    DataValue::Word(symbol("payload")),
                ],
            },
        ],
        functions: vec![
            Function {
                name: "identity".into(),
                export: false,
                params: vec![(I64, "value".into())],
                result: Some(I64),
                body: vec![
                    Statement::Label("start".into()),
                    Statement::Return(Some(temp("value"))),
                ],
            },
            probe(
                vec![
                    Statement::Label("start".into()),
                    assign(
                        "function",
                        I64,
                        Operation::Load(LoadKind::I64, symbol("descriptor")),
                    ),
                    assign(
                        "field",
                        I64,
                        Operation::Binary(BinaryOp::Add, symbol("descriptor"), Operand::Int(8)),
                    ),
                    assign(
                        "address",
                        I64,
                        Operation::Load(LoadKind::I64, temp("field")),
                    ),
                    assign(
                        "value",
                        I64,
                        Operation::Load(LoadKind::I64, temp("address")),
                    ),
                    assign(
                        "result",
                        I64,
                        Operation::Call {
                            callee: temp("function"),
                            args: vec![(I64, temp("value"))],
                            variadic: None,
                        },
                    ),
                    Statement::Return(Some(temp("result"))),
                ],
                vec![],
            ),
        ],
    };
    let harness = "unsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"{}\", unsafe { probe() }); }\n";
    assert_eq!(
        NativeFixture::new().execute(&program, harness),
        b"-9223372036854775717\n"
    );
}

#[test]
fn immutable_relocation_resolves_known_external_without_a_direct_call() {
    use Scalar::I64;
    let program = Program {
        data: vec![Data {
            name: "callback".into(),
            values: vec![DataValue::Word(symbol("fern_str_eq"))],
        }],
        functions: vec![probe(
            vec![
                Statement::Label("start".into()),
                assign(
                    "function",
                    I64,
                    Operation::Load(LoadKind::I64, symbol("callback")),
                ),
                assign(
                    "result",
                    I64,
                    Operation::Call {
                        callee: temp("function"),
                        args: vec![(I64, Operand::Int(0)), (I64, Operand::Int(0))],
                        variadic: None,
                    },
                ),
                Statement::Return(Some(temp("result"))),
            ],
            vec![],
        )],
    };
    // A safe surrogate has the canonical C signature and observes the indirect call;
    // it intentionally accepts NULL instead of invoking the runtime's non-null API.
    let harness = "#[unsafe(no_mangle)] extern \"C\" fn fern_str_eq(a:*const u8,b:*const u8)->i64 {if a==b {4294967297} else {-1}}\nunsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"{}\", unsafe { probe() }); }\n";
    assert_eq!(
        NativeFixture::new().execute(&program, harness),
        b"4294967297\n"
    );
}

#[test]
fn indirect_unit_return_is_discarded_and_c_predicate_result_is_narrowed() {
    use Scalar::{I32, I64};
    let program = Program {
        data: vec![],
        functions: vec![
            Function {
                name: "unit".into(),
                export: false,
                result: Some(I32),
                params: vec![],
                body: vec![
                    Statement::Label("start".into()),
                    Statement::Effect(Operation::Call {
                        callee: symbol("fern_print_int"),
                        args: vec![(I64, Operand::Int(73))],
                        variadic: None,
                    }),
                    Statement::Return(Some(Operand::Int(0))),
                ],
            },
            probe(
                vec![
                    Statement::Label("start".into()),
                    assign(
                        "callback",
                        I64,
                        Operation::Unary(UnaryOp::Copy, symbol("unit")),
                    ),
                    Statement::Effect(Operation::Call {
                        callee: temp("callback"),
                        args: vec![],
                        variadic: None,
                    }),
                    assign(
                        "predicate",
                        I32,
                        Operation::Call {
                            callee: symbol("fern_result_is_ok"),
                            args: vec![(I64, Operand::Int(0))],
                            variadic: None,
                        },
                    ),
                    assign(
                        "wide",
                        I64,
                        Operation::Unary(UnaryOp::ExtUw, temp("predicate")),
                    ),
                    Statement::Return(Some(temp("wide"))),
                ],
                vec![],
            ),
        ],
    };
    let harness = "#[unsafe(no_mangle)] extern \"C\" fn fern_print_int(n:i64) {println!(\"callback={n}\");}\n#[unsafe(no_mangle)] extern \"C\" fn fern_result_is_ok(_:i64)->i64 {1}\nunsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"predicate={}\", unsafe { probe() }); }\n";
    assert_eq!(
        NativeFixture::new().execute(&program, harness),
        b"callback=73\npredicate=1\n"
    );
}

#[test]
fn provided_word_argument_is_truncated_before_canonical_int64_extension() {
    use Scalar::{I32, I64};
    let program = Program {
        data: vec![],
        functions: vec![probe(
            vec![
                Statement::Label("start".into()),
                assign(
                    "wide",
                    I64,
                    Operation::Unary(UnaryOp::Copy, Operand::Int(0x1_ffff_ffff)),
                ),
                assign(
                    "result",
                    I64,
                    Operation::Call {
                        callee: symbol("fern_result_ok"),
                        args: vec![(I32, temp("wide"))],
                        variadic: None,
                    },
                ),
                Statement::Return(Some(temp("result"))),
            ],
            vec![],
        )],
    };
    let harness = "#[unsafe(no_mangle)] extern \"C\" fn fern_result_ok(payload:i64)->i64 {payload}\nunsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"{}\", unsafe { probe() }); }\n";
    assert_eq!(
        NativeFixture::new().execute(&program, harness),
        b"4294967295\n"
    );
}

#[test]
fn sole_native_pointer_stays_visible_to_rust_collector_across_collecting_calls() {
    use Scalar::I64;
    let program = Program {
        data: vec![],
        functions: vec![probe(
            vec![
                Statement::Label("start".into()),
                assign(
                    "retained",
                    I64,
                    Operation::Call {
                        callee: symbol("fern_alloc"),
                        args: vec![(I64, Operand::Int(8))],
                        variadic: None,
                    },
                ),
                Statement::Store {
                    kind: LoadKind::I64,
                    value: Operand::Int(i64::MIN + 91),
                    address: temp("retained"),
                },
                Statement::Effect(Operation::Call {
                    callee: symbol("fern_gc_collect"),
                    args: vec![],
                    variadic: None,
                }),
                assign(
                    "payload",
                    I64,
                    Operation::Load(LoadKind::I64, temp("retained")),
                ),
                Statement::Return(Some(temp("payload"))),
            ],
            vec![],
        )],
    };
    let archive = std::env::var_os("FERN_RUNTIME_CORE_LIB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let current = std::env::current_exe().unwrap();
            current
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name == "debug" || name == "release")
                })
                .unwrap()
                .join("libfern_runtime.a")
        });
    assert!(
        archive.is_file(),
        "build the Rust runtime core archive first: {}",
        archive.display()
    );
    // Only the generated function holds the allocation address across collection.
    let harness = "unsafe extern \"C\" { fn probe() -> i64; } fn main() { println!(\"{}\", unsafe { probe() }); }";
    assert_eq!(
        NativeFixture::new().execute_linked(&program, harness, &[archive.into_os_string()]),
        b"-9223372036854775717\n"
    );
}

/// Quiet/signaling NaNs retain sign and payload across constants, loads and bitcasts.
#[test]
fn exact_ieee_bits_survive_native_codegen_without_decimal_canonicalization() {
    use Scalar::{F64, I64};
    let patterns: [u64; 9] = [
        0x0000_0000_0000_0000, // positive zero
        0x8000_0000_0000_0000, // negative zero
        0x0000_0000_0000_0001, // smallest subnormal
        0x7fef_ffff_ffff_ffff, // largest finite value
        0x7ff0_0000_0000_0000, // infinity
        0xfff0_0000_0000_0000, // negative infinity
        0x7ff8_0000_0000_0123, // quiet NaN with payload
        0xfff8_0000_0000_0456, // negative quiet NaN
        0x7ff0_0000_0000_0789, // signaling NaN
    ];
    let mut program = Program::default();
    let mut declarations = String::new();
    let mut calls = String::new();
    let mut expected = String::new();
    for (index, bits) in patterns.into_iter().enumerate() {
        let data = format!("float_data{index}");
        program.data.push(Data {
            name: data.clone(),
            values: vec![DataValue::Word(Operand::Float(bits))],
        });
        for (kind, operation) in [
            (
                "constant",
                Operation::Unary(UnaryOp::Copy, Operand::Float(bits)),
            ),
            ("loaded", Operation::Load(LoadKind::F64, symbol(&data))),
        ] {
            let name = format!("{kind}{index}");
            program.functions.push(Function {
                name: name.clone(),
                export: true,
                result: Some(I64),
                params: vec![],
                body: vec![
                    Statement::Label("start".into()),
                    assign("float", F64, operation),
                    assign("bits", I64, Operation::Unary(UnaryOp::Cast, temp("float"))),
                    Statement::Return(Some(temp("bits"))),
                ],
            });
            declarations.push_str(&format!("fn {name}() -> u64;\n"));
            calls.push_str(&format!(
                "println!(\"{{:016x}}\", unsafe {{ {name}() }});\n"
            ));
            expected.push_str(&format!("{bits:016x}\n"));
        }
    }
    let harness = format!("unsafe extern \"C\" {{ {declarations} }}\nfn main() {{ {calls} }}\n");
    let fixture = NativeFixture::new();
    assert_eq!(fixture.execute(&program, &harness), expected.as_bytes());
}
