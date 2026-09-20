//! Backend acceptance keeps independent expected results and rejects malformed imports.
use morrow_compiler::{
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
    let signature = morrow_compiler::runtime_abi::signature("morrow_result_is_ok").unwrap();
    assert_eq!(signature.result, Some(machine::Scalar::I64));
}

/// Native acceptance invokes only the host linker and the generated object, never QBE.
struct NativeFixture(std::path::PathBuf);
impl NativeFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-cranelift-native-{}-{}",
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
            values: vec![DataValue::Word(symbol("morrow_str_eq"))],
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
    let harness = "#[unsafe(no_mangle)] extern \"C\" fn morrow_str_eq(a:*const u8,b:*const u8)->i64 {if a==b {4294967297} else {-1}}\nunsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"{}\", unsafe { probe() }); }\n";
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
                        callee: symbol("morrow_print_int"),
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
                            callee: symbol("morrow_result_is_ok"),
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
    let harness = "#[unsafe(no_mangle)] extern \"C\" fn morrow_print_int(n:i64) {println!(\"callback={n}\");}\n#[unsafe(no_mangle)] extern \"C\" fn morrow_result_is_ok(_:i64)->i64 {1}\nunsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"predicate={}\", unsafe { probe() }); }\n";
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
                        callee: symbol("morrow_result_ok"),
                        args: vec![(I32, temp("wide"))],
                        variadic: None,
                    },
                ),
                Statement::Return(Some(temp("result"))),
            ],
            vec![],
        )],
    };
    let harness = "#[unsafe(no_mangle)] extern \"C\" fn morrow_result_ok(payload:i64)->i64 {payload}\nunsafe extern \"C\" { fn probe() -> i64; }\nfn main() { println!(\"{}\", unsafe { probe() }); }\n";
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
                        callee: symbol("morrow_alloc"),
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
                    callee: symbol("morrow_gc_collect"),
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
    let archive = std::env::var_os("MORROW_RUNTIME_CORE_LIB")
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
                .join("libmorrow_runtime.a")
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
fn typed_native_root_survives_collection_without_stack_or_register_scanning() {
    let source = "fn main() -> Int:\n    let text = \"native\" + \" root\"\n    String.len(text)\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    let position = function.body.iter().position(|statement| matches!(statement,
        Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
        if machine::bare(name) == "morrow_str_len"
    )).unwrap();
    // The missing-root branch avoids dereferencing freed memory in the negative
    // control. Only generated code holds the dynamic allocation across this GC.
    function.body.splice(
        position..position,
        [
            Statement::Effect(Operation::Call {
                callee: symbol("morrow_gc_collect_precise"),
                args: vec![],
                variadic: None,
            }),
            assign(
                "gc_bytes",
                Scalar::I64,
                Operation::Call {
                    callee: symbol("morrow_gc_heap_size"),
                    args: vec![],
                    variadic: None,
                },
            ),
            assign(
                "gc_alive",
                Scalar::I32,
                Operation::Binary(
                    BinaryOp::Compare(Comparison::Ne, Scalar::I64),
                    temp("gc_bytes"),
                    Operand::Int(0),
                ),
            ),
            Statement::Branch {
                condition: temp("gc_alive"),
                then_label: "gc_present".into(),
                else_label: "gc_missing".into(),
            },
            Statement::Label("gc_missing".into()),
            Statement::Store {
                kind: LoadKind::I64,
                value: Operand::Int(-999),
                address: temp("return_slot"),
            },
            Statement::Jump("return".into()),
            Statement::Label("gc_present".into()),
        ],
    );
    let archive = core_runtime_archive();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { println!(\"{}\", unsafe { morrow_main() }); }";
    assert_eq!(
        NativeFixture::new().execute_linked(&program, harness, &[archive.clone().into_os_string()]),
        b"11\n"
    );

    // Independently prove this oracle fails safely when explicit registration
    // is absent, even though live pointer copies remain in native stack slots.
    for function in &mut program.functions {
        for statement in &mut function.body {
            if let Statement::Assign { operation, .. } = statement
                && matches!(operation, Operation::Call { callee: Operand::Symbol(name), .. } if machine::bare(name) == "morrow_gc_frame_enter")
            {
                *operation = Operation::Unary(UnaryOp::Copy, Operand::Int(0));
            }
        }
        function.body.retain(|statement| !matches!(statement,
            Statement::Effect(Operation::Call { callee: Operand::Symbol(name), .. }) if machine::bare(name) == "morrow_gc_frame_leave"
        ));
    }
    assert_eq!(
        NativeFixture::new().execute_linked(&program, harness, &[archive.into_os_string()]),
        b"-999\n"
    );
}

#[test]
fn integer_matching_a_heap_address_is_not_a_precise_root_and_keeps_all_bits() {
    let source = "fn identity(value: Int) -> Int: value\nfn main(): ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    function.export = true;
    function.body.insert(
        1,
        Statement::Effect(Operation::Call {
            callee: symbol("morrow_gc_collect_precise"),
            args: vec![],
            variadic: None,
        }),
    );
    let harness = "unsafe extern \"C\" { fn f0(env:usize,fault:*mut i64,value:i64)->i64; fn morrow_alloc(size:usize)->usize; fn morrow_gc_heap_size()->usize; } fn main() { let mut fault=0; let address=unsafe{morrow_alloc(32)}; let value=unsafe{f0(0,&mut fault,address as i64)}; assert_eq!(value,address as i64); assert_eq!(unsafe{morrow_gc_heap_size()},0); for n in [i64::MIN,i64::MAX] { assert_eq!(unsafe{f0(0,&mut fault,n)},n); } println!(\"precise integers preserved\"); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"precise integers preserved\n"
    );
}

#[test]
fn generated_actor_callback_roots_belong_to_its_owned_heap() {
    let source = "fn worker():\n    let text = \"actor\" + \" root\"\n    println(String.len(text))\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    let position = function.body.iter().position(|statement| matches!(statement,
        Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
        if machine::bare(name) == "morrow_str_len"
    )).unwrap();
    // The actor's environment and string are both live. Comparing heap sizes
    // makes a missing registration fail without touching a reclaimed pointer.
    function.body.splice(
        position..position,
        [
            assign(
                "gc_before",
                Scalar::I64,
                Operation::Call {
                    callee: symbol("morrow_gc_heap_size"),
                    args: vec![],
                    variadic: None,
                },
            ),
            Statement::Effect(Operation::Call {
                callee: symbol("morrow_gc_collect_precise"),
                args: vec![],
                variadic: None,
            }),
            assign(
                "gc_after",
                Scalar::I64,
                Operation::Call {
                    callee: symbol("morrow_gc_heap_size"),
                    args: vec![],
                    variadic: None,
                },
            ),
            assign(
                "gc_preserved",
                Scalar::I32,
                Operation::Binary(
                    BinaryOp::Compare(Comparison::Eq, Scalar::I64),
                    temp("gc_before"),
                    temp("gc_after"),
                ),
            ),
            Statement::Branch {
                condition: temp("gc_preserved"),
                then_label: "gc_present".into(),
                else_label: "gc_missing".into(),
            },
            Statement::Label("gc_missing".into()),
            Statement::Store {
                kind: LoadKind::I64,
                value: Operand::Int(0),
                address: temp("return_slot"),
            },
            Statement::Jump("return".into()),
            Statement::Label("gc_present".into()),
        ],
    );
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { println!(\"code={}\", unsafe { morrow_main() }); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"10\ncode=0\n"
    );
}

#[test]
fn compiled_actor_range_capture_keeps_full_width_endpoints_and_inclusive_flag() {
    let source = "fn main():\n    let values = -2..=2\n    let boundary = 9223372036854775806..=9223372036854775807\n    let pid: Pid(()) = spawn(() ->\n        for value in values: println(value)\n        for value in boundary: println(value)\n    )\n    ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"-2\n-1\n0\n1\n2\n9223372036854775806\n9223372036854775807\n"
    );
}

#[test]
fn compiled_supervised_actor_recovers_checked_runtime_fault_and_drains_cleanup() {
    let source = "fn broken():\n    defer println(\"cleanup\")\n    println(\"attempt\")\n    let empty: List(Int) = []\n    println(List.head(empty))\nfn sibling(): println(\"sibling\")\nfn main():\n    let failed: Pid(()) = supervise(broken, 2)\n    let other: Pid(()) = spawn(sibling)\n    ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"attempt\ncleanup\nsibling\nattempt\ncleanup\nattempt\ncleanup\n"
    );
}

#[test]
fn compiled_supervision_lookup_observes_fresh_identity_without_redirecting_stale_sends() {
    let source = "fn broken():\n    let empty: List(Int) = []\n    println(List.head(empty))\nfn observe(original: Pid(())):\n    match send(original, ()):\n        Ok(()) -> println(\"wrong stale\")\n        Err(_) -> println(\"stale\")\n    match supervised_current(original):\n        Ok(current) ->\n            match send(current, ()):\n                Ok(()) -> println(\"fresh\")\n                Err(_) -> println(\"wrong fresh\")\n        Err(_) -> println(\"missing\")\nfn main():\n    let original: Pid(()) = supervise(broken, 1)\n    let other: Pid(()) = spawn(() -> observe(original))\n    ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"stale\nfresh\n"
    );
}

#[test]
fn compiled_supervision_recovers_terminal_layout_limit_without_process_exit() {
    let source = "fn broken():\n    let panel = Tui.Panel.new(\"body\")\n    let large = Tui.Panel.width(panel, 9223372036854775807)\n    println(Tui.Panel.render(large))\nfn sibling(): println(\"alive\")\nfn main():\n    let failed: Pid(()) = supervise(broken, 0)\n    let other: Pid(()) = spawn(sibling)\n    ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"alive\n"
    );
}

#[test]
fn compiled_supervision_recovers_regex_output_limit_without_process_exit() {
    let source = "fn broken():\n    let replacement = String.repeat(\"y\", 8388609)\n    println(Regex.replace_all(\"aa\", \"a\", replacement))\nfn sibling(): println(\"alive\")\nfn main():\n    let failed: Pid(()) = supervise(broken, 0)\n    let other: Pid(()) = spawn(sibling)\n    ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"alive\n"
    );
}

#[test]
fn unit_tail_helper_yields_to_sibling_through_the_native_host_poll_boundary() {
    unit_tail_poll(
        "    let first: Pid(()) = spawn(() -> busy(2048, reply))\n    ()",
        1,
    );
}

#[test]
fn unit_tail_helper_in_a_receive_arm_yields_before_completing() {
    unit_tail_poll(
        "    let first: Pid(()) = spawn(() ->\n        receive:\n            () -> busy(2048, reply)\n    )\n    match send(first, ()):\n        Ok(()) -> ()\n        Err(_) -> ()",
        2,
    );
}

#[test]
fn unit_tail_helper_in_a_receive_timeout_yields_before_completing() {
    unit_tail_poll(
        "    let first: Pid(()) = spawn(() ->\n        receive:\n            () -> ()\n            _ after 0 -> busy(2048, reply)\n    )\n    ()",
        2,
    );
}

fn unit_tail_poll(setup: &str, warmup: usize) {
    use morrow_compiler::native_library::{self, Export};
    let source = format!(
        r#"
fn busy(remaining: Int, reply: Pid(String)):
    if remaining == 0:
        match send(reply, "finished"):
            Ok(()) -> ()
            Err(_) -> ()
    else:
        busy(remaining - 1, reply)
fn sibling(reply: Pid(String)):
    match send(reply, "sibling"):
        Ok(()) -> ()
        Err(_) -> ()
pub fn start(reply: Pid(String)) -> ():
{setup}
pub fn queue_sibling(reply: Pid(String)) -> ():
    let second: Pid(()) = spawn(() -> sibling(reply))
    ()
"#
    );
    let checked =
        morrow_compiler::check::check_library(&morrow_compiler::parse::parse(&source).unwrap())
            .unwrap();
    let program = native_library::lower(
        &checked,
        &[
            Export::new("start", "start"),
            Export::new("queue_sibling", "queue_sibling"),
        ],
    )
    .unwrap();
    let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_library_string_port(exec: usize) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn morrow_export_queue_sibling(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_port_peek_len(exec: usize, port: usize) -> i64;
    fn morrow_managed_port_read(exec: usize, port: usize, output: *mut u8, capacity: usize) -> i64;
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize;
    fn morrow_gc_frame_leave(token: usize);
}
fn main() {
    let mut fault = Box::new(0);
    unsafe {
        let exec = morrow_library_open(&mut *fault);
        assert_ne!(exec, 0);
        let port = morrow_library_string_port(exec);
        let root = morrow_gc_frame_enter(&port, 1);
        morrow_export_start(&mut *fault, exec, port);
        assert_eq!(*fault, 0);
        for _ in 0..WARMUP {
            assert_eq!(morrow_managed_poll(exec, 1), 2);
            assert_eq!(morrow_managed_port_peek_len(exec, port), -1, "the busy helper must suspend before it finishes");
        }
        morrow_export_queue_sibling(&mut *fault, exec, port);
        for _ in 0..3 {
            assert_eq!(morrow_managed_poll(exec, 1), 2);
            if morrow_managed_port_peek_len(exec, port) >= 0 { break; }
        }
        let mut bytes = [0u8; 16];
        let count = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
        assert_eq!(count, 7, "sibling must make progress within four callbacks");
        assert_eq!(&bytes[..count as usize], b"sibling");
        assert_eq!(*fault, 0);
        morrow_managed_close(exec);
        morrow_gc_frame_leave(root);
    }
    println!("sibling progressed before busy helper completed");
}
"#.replace("WARMUP", &warmup.to_string());
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"sibling progressed before busy helper completed\n"
    );
}

#[test]
fn unit_tail_helper_behind_a_local_entry_alias_yields_without_changing_ordinary_calls() {
    use morrow_compiler::native_library::{self, Export};
    let source = r#"
fn busy(remaining: Int):
    if remaining == 0: println("finished")
    else: busy(remaining - 1)
pub fn start() -> ():
    let entry = () -> busy(2048)
    let first: Pid(()) = spawn(entry)
    let second: Pid(()) = spawn(() -> println("sibling"))
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
    let mut fault = Box::new(0);
    unsafe {
        let exec = morrow_library_open(&mut *fault);
        assert_ne!(exec, 0);
        morrow_export_start(&mut *fault, exec);
        assert_eq!(morrow_managed_poll(exec, 1), 2);
        println!("yielded");
        assert_eq!(morrow_managed_poll(exec, 1), 2);
        assert_eq!(*fault, 0);
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
        b"yielded\nsibling\n"
    );
}

#[test]
fn unit_tail_helpers_preserve_full_width_roots_across_a_hundred_thousand_transitions() {
    let source = r#"
fn left(remaining: Int, text: String, boundary: Int):
    if remaining == 0:
        println(text)
        println(boundary)
    else:
        return right(remaining: remaining - 1, text: text, boundary: boundary)
fn right(remaining: Int, text: String, boundary: Int):
    match remaining:
        0 ->
            println(text)
            println(boundary)
        _ -> left(remaining: remaining - 1, text: text, boundary: boundary)
fn main():
    let ordinary = left
    ordinary(2, "ordinary", 9223372036854775807)
    let text = String.repeat("morrow", 2)
    let busy: Pid(Int) = spawn(() -> left(remaining: 100000, text: text, boundary: -9223372036854775808))
    let sibling: Pid(String) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let mut safepoints = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "morrow_managed_continue")
            {
                body.push(Statement::Effect(Operation::Call {
                    callee: Operand::Symbol("morrow_gc_collect_precise".into()),
                    args: vec![],
                    variadic: None,
                }));
                safepoints += 1;
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert!(
        safepoints >= 3,
        "root oracle must cover the entry and both mutual helpers"
    );
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"ordinary\n9223372036854775807\nsibling\nmorrowmorrow\n-9223372036854775808\n"
    );
}

#[test]
fn unit_tail_helpers_preserve_synchronous_cleanup_non_tail_calls_and_numeric_results() {
    let source = r#"
fn leaf(value: Int): println(value)
fn deferred():
    defer println("cleanup")
    leaf(7)
fn non_tail():
    leaf(8)
    println("after")
fn numeric(value: Int) -> Int:
    if value == 0: 0
    else: 1 + numeric(value - 1)
fn main():
    let callback = leaf
    callback(3)
    deferred()
    println(numeric(3))
    let first: Pid(()) = spawn(deferred)
    let second: Pid(()) = spawn(non_tail)
    let third: Pid(()) = spawn(() -> leaf(9))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"3\n7\ncleanup\n3\n7\ncleanup\n8\nafter\n9\n"
    );
}

#[test]
fn compiled_json_actor_capture_and_mailbox_use_a_distinct_descriptor() {
    let source = "fn main():\n    let captured = json.from_int(-9223372036854775808)\n    let first: Pid(()) = spawn(() ->\n        match json.as_int(captured):\n            Ok(value) -> println(value)\n            Err(_) -> println(0)\n    )\n    let target: Pid(json.Value) = spawn(() ->\n        receive:\n            message ->\n                match json.as_int(message):\n                    Ok(value) -> println(value)\n                    Err(_) -> println(0)\n    )\n    match send(target, json.from_int(9223372036854775807)):\n        Ok(()) -> ()\n        Err(_) -> println(0)\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"-9223372036854775808\n9223372036854775807\n"
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

#[test]
fn local_feedback_and_authoritative_loading_have_expected_native_values() {
    use morrow_compiler::{
        check,
        native_library::{self, Export},
        parse,
    };
    let source = format!(
        "{}\n{}",
        include_str!("../../../examples/web/checklist.mr"),
        include_str!("fixtures/web_feedback.mr")
    );
    let checked = check::check_library(&parse::parse(&source).unwrap()).unwrap();
    let program =
        native_library::lower(&checked, &[Export::new("feedback_trace", "feedback_trace")])
            .unwrap();
    let harness = r#"
unsafe extern "C" {
    fn morrow_export_feedback_trace(fault: *mut i64, exec: usize, input: i64) -> i64;
}
fn main() {
    let mut fault = 0;
    for (index, expected) in [1, 1, 0, 1, 1, 1, 1, 256, 256, 1, 16, 1, 1, 0, 11].into_iter().enumerate() {
        assert_eq!(unsafe { morrow_export_feedback_trace(&mut fault, 0, index as i64) }, expected, "feedback trace {index}");
        assert_eq!(fault, 0);
    }
    println!("local preview and confirmed loading");
}
"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"local preview and confirmed loading\n"
    );
}

#[test]
fn compiled_decoded_map_capture_and_literal_mailbox_preserve_untagged_pairs() {
    let source = include_str!("fixtures/actor_map.mr");
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(NativeFixture::new().execute_linked(&program, harness, &[core_runtime_archive().into_os_string()]), b"2\n-9223372036854775808\n9223372036854775807\n0\n2\n-9223372036854775808\n9223372036854775807\n");
}

#[test]
fn immutable_sets_keep_string_keys_and_aliases_across_precise_collection() {
    let source = r#"
newtype Key = Key(String)
fn make(n: Int) -> Set(Key):
    let base = List.map(["a", "🌿", "café", "a"], (s) -> Key(s + "!"))
    let original = Set.from_list(base)
    if n == 0:
        original
    else:
        Set.union(original, make(n - 1))
fn main():
    let original = make(20)
    let changed = Set.insert(Set.delete(original, Key("a!")), Key("new!"))
    println(Set.len(original))
    println(Set.len(changed))
    println(Set.contains(original, Key("a" + "!")))
    println(Set.contains(changed, Key("a!")))
    println(Set.contains(original, Key("🌿!")))
    println(Set.len(Set.intersection(original, changed)))
    println(Set.equal(original, Set.from_list([Key("café!"), Key("a!"), Key("🌿!")])))
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let mut inserted = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in function.body.drain(..) {
            if matches!(&statement,
                Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
                if machine::bare(name).starts_with("morrow_rs_map_")
            ) {
                body.push(Statement::Effect(Operation::Call {
                    callee: symbol("morrow_gc_collect_precise"),
                    args: vec![],
                    variadic: None,
                }));
                inserted += 1;
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert!(
        inserted >= 10,
        "oracle must collect at actual map operation boundaries"
    );
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"3\n3\ntrue\nfalse\ntrue\n2\ntrue\n"
    );
}

#[test]
fn seeded_native_sets_match_independent_full_width_model() {
    use std::collections::BTreeSet;
    let keys = [i64::MIN, i64::MAX, 0, 1, -1, 4294967296, -4294967296];
    let mut state = BTreeSet::new();
    let mut rng = 0x5e7_f00du64;
    let mut source =
        String::from("fn main():\n    let state: Set(Int) = Set.new()\n    let original = state\n");
    let mut expected = String::new();
    for _ in 0..120 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let key = keys[((rng >> 32) as usize) % keys.len()];
        let method = if rng & 3 == 0 { "delete" } else { "insert" };
        if method == "delete" {
            state.remove(&key);
        } else {
            state.insert(key);
        }
        source.push_str(&format!("    let state = Set.{method}(state, {key})\n    println(Set.len(state))\n    println(Set.contains(state, {key}))\n    println(Set.is_empty(original))\n"));
        expected.push_str(&format!(
            "{}\n{}\ntrue\n",
            state.len(),
            state.contains(&key)
        ));
    }
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(&source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        expected.as_bytes()
    );
}

#[test]
fn map_put_roots_fresh_output_before_nested_pair_allocation() {
    let source = "fn main():\n    let original = %{\"a\": 4294967296}\n    let changed = Map.put(original, \"b\", -9223372036854775808)\n    println(Map.len(original))\n    println(Map.len(changed))\n    println(Option.unwrap_or(Map.get(changed, \"b\"), 0))\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let put = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == "morrow_rs_map_put")
        .unwrap();
    let index = put.body.iter().position(|s| matches!(s, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "morrow_rs_map_pair")).unwrap();
    put.body.insert(
        index,
        Statement::Effect(Operation::Call {
            callee: symbol("morrow_gc_collect_precise"),
            args: vec![],
            variadic: None,
        }),
    );
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"1\n2\n-9223372036854775808\n"
    );
}

#[test]
fn foreign_native_abi_preserves_signed_unsigned_narrow_and_float_results() {
    use morrow_compiler::ffi::{AbiType as C, Declaration};
    let mut program = Program::default();
    let mut declarations = String::new();
    let mut callbacks = String::new();
    let mut assertions = String::new();
    for (index, (abi, rust, input, expected)) in [
        (C::I8, "i8", "-127", "-127"),
        (C::U8, "u8", "254", "254"),
        (C::I16, "i16", "-32767", "-32767"),
        (C::U16, "u16", "65534", "65534"),
        (C::I32, "i32", "-2147483647", "-2147483647"),
        (C::U32, "u32", "4294967294", "4294967294"),
        (
            C::I64,
            "i64",
            "-9223372036854775808",
            "-9223372036854775808",
        ),
        (C::U64, "u64", "-1", "-1"),
    ]
    .into_iter()
    .enumerate()
    {
        let foreign = format!("foreign_width_{index}");
        let wrapper = format!("width_{index}");
        callbacks.push_str(&format!("#[unsafe(no_mangle)] pub extern \"C\" fn {foreign}(value: {rust}) -> {rust} {{ value }}\n"));
        declarations.push_str(&format!("fn {wrapper}(value: i64) -> i64;\n"));
        assertions.push_str(&format!(
            "assert_eq!(unsafe {{ {wrapper}({input}) }}, {expected});\n"
        ));
        assertions.push_str(&format!("for _ in 0..128 {{ seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1); let bits = seed as i64; assert_eq!(unsafe {{ {wrapper}(bits) }}, (bits as {rust}) as i64); }}\n"));
        program.functions.push(Function {
            name: wrapper,
            export: true,
            params: vec![(Scalar::I64, "value".into())],
            result: Some(Scalar::I64),
            body: vec![
                Statement::Label("start".into()),
                assign(
                    "returned",
                    Scalar::I64,
                    Operation::ForeignCall {
                        declaration: Declaration {
                            symbol: foreign,
                            library: None,
                            params: vec![abi.clone()],
                            result: abi,
                        },
                        args: vec![(Scalar::I64, temp("value"))],
                    },
                ),
                Statement::Return(Some(temp("returned"))),
            ],
        });
    }
    for (name, abi, rust, logical) in [
        ("float32", C::F32, "f32", Scalar::F64),
        ("float64", C::F64, "f64", Scalar::F64),
        ("boolean", C::Bool, "bool", Scalar::I32),
    ] {
        let foreign = format!("foreign_{name}");
        callbacks.push_str(&format!("#[unsafe(no_mangle)] pub extern \"C\" fn {foreign}(value: {rust}) -> {rust} {{ value }}\n"));
        let (wide, input) = if logical == Scalar::F64 {
            ("f64", "1.25")
        } else {
            ("u32", "1")
        };
        declarations.push_str(&format!("fn {name}(value: {wide}) -> {wide};\n"));
        assertions.push_str(&format!(
            "assert_eq!(unsafe {{ {name}({input}) }}, {input});\n"
        ));
        if name == "float32" {
            assertions.push_str("for value in [1.1f64, -0.0, f64::MIN_POSITIVE, f64::MAX, f64::INFINITY, f64::NEG_INFINITY] { assert_eq!(unsafe { float32(value) }.to_bits(), (value as f32 as f64).to_bits()); }\n");
        }
        if name == "boolean" {
            assertions.push_str(
                "assert_eq!(unsafe { boolean(0) }, 0); assert_eq!(unsafe { boolean(2) }, 1);\n",
            );
        }

        program.functions.push(Function {
            name: name.into(),
            export: true,
            params: vec![(logical, "value".into())],
            result: Some(logical),
            body: vec![
                Statement::Label("start".into()),
                assign(
                    "returned",
                    logical,
                    Operation::ForeignCall {
                        declaration: Declaration {
                            symbol: foreign,
                            library: None,
                            params: vec![abi.clone()],
                            result: abi,
                        },
                        args: vec![(logical, temp("value"))],
                    },
                ),
                Statement::Return(Some(temp("returned"))),
            ],
        });
    }
    let harness = format!(
        "{callbacks}\nunsafe extern \"C\" {{ {declarations} }}\nfn main() {{ let mut seed = 0x464649u64; {assertions} println!(\"foreign widths preserved\"); }}\n"
    );
    assert_eq!(
        NativeFixture::new().execute(&program, &harness),
        b"foreign widths preserved\n"
    );
}

#[test]
fn foreign_declarations_reject_conflicting_symbols_and_mismatched_logical_values() {
    use morrow_compiler::ffi::{AbiType as C, Declaration};
    let declaration = Declaration {
        symbol: "external_width".into(),
        library: None,
        params: vec![C::I8],
        result: C::I8,
    };
    let mut program = scalar_program();
    let call = |declaration: Declaration, ty: Scalar| {
        Statement::Effect(Operation::ForeignCall {
            declaration,
            args: vec![(ty, Operand::Int(1))],
        })
    };
    program.functions[0]
        .body
        .insert(1, call(declaration.clone(), Scalar::I64));
    assert!(program.validate().is_ok());
    let mut conflicting = program.clone();
    let mut wrong = declaration.clone();
    wrong.result = C::U8;
    conflicting.functions[0]
        .body
        .insert(1, call(wrong, Scalar::I64));
    assert!(
        conflicting
            .validate()
            .unwrap_err()
            .contains("conflicting foreign")
    );
    for symbol in ["main", "printf", "morrow_alloc"] {
        let mut forged = program.clone();
        let mut wrong = declaration.clone();
        wrong.symbol = symbol.into();
        forged.functions[0].body[1] = call(wrong, Scalar::I64);
        assert!(forged.validate().is_err(), "{symbol}");
    }
    let mut forged = program.clone();
    forged.functions[0].body[1] = call(declaration.clone(), Scalar::I32);
    assert!(forged.validate().unwrap_err().contains("logical argument"));
    let mut forged = program;
    forged.functions[0].body[1] = assign(
        "invalid",
        Scalar::F64,
        Operation::ForeignCall {
            declaration,
            args: vec![(Scalar::I64, Operand::Int(1))],
        },
    );
    assert!(forged.validate().unwrap_err().contains("result type"));
}

#[test]
fn foreign_mixed_narrow_register_stack_pointer_and_void_abis() {
    use morrow_compiler::ffi::{AbiType as C, Declaration};
    let mut program = Program::default();
    let mut abi = Vec::new();
    let mut args = Vec::new();
    let mut parameters = Vec::new();
    let mut checks = Vec::new();
    for index in 0..12 {
        abi.extend([C::I8, C::U16, C::F32]);
        args.extend([
            (Scalar::I64, Operand::Int(-100 + index)),
            (Scalar::I64, Operand::Int(65500 + index)),
            (Scalar::F64, Operand::Float((index as f64 + 0.25).to_bits())),
        ]);
        parameters.push(format!("a{index}: i8, b{index}: u16, c{index}: f32"));
        checks.push(format!(
            "a{index} == {} && b{index} == {} && c{index} == {}f32",
            -100 + index,
            65500 + index,
            index as f64 + 0.25
        ));
    }
    program.functions.push(Function {
        name: "mixed".into(),
        export: true,
        params: vec![],
        result: Some(Scalar::I64),
        body: vec![
            Statement::Label("start".into()),
            assign(
                "result",
                Scalar::I64,
                Operation::ForeignCall {
                    declaration: Declaration {
                        symbol: "external_mixed".into(),
                        library: None,
                        params: abi,
                        result: C::I64,
                    },
                    args,
                },
            ),
            Statement::Return(Some(temp("result"))),
        ],
    });
    let pointer = C::Pointer(morrow_compiler::Type::Int);
    program.functions.push(Function {
        name: "pointer".into(),
        export: true,
        params: vec![(Scalar::I64, "address".into())],
        result: Some(Scalar::I64),
        body: vec![
            Statement::Label("start".into()),
            assign(
                "result",
                Scalar::I64,
                Operation::ForeignCall {
                    declaration: Declaration {
                        symbol: "external_pointer".into(),
                        library: None,
                        params: vec![pointer.clone()],
                        result: pointer,
                    },
                    args: vec![(Scalar::I64, temp("address"))],
                },
            ),
            Statement::Return(Some(temp("result"))),
        ],
    });
    program.functions.push(Function {
        name: "effect".into(),
        export: true,
        params: vec![],
        result: None,
        body: vec![
            Statement::Label("start".into()),
            Statement::Effect(Operation::ForeignCall {
                declaration: Declaration {
                    symbol: "external_effect".into(),
                    library: None,
                    params: vec![C::I32],
                    result: C::Void,
                },
                args: vec![(Scalar::I64, Operand::Int(-42))],
            }),
            Statement::Return(None),
        ],
    });
    for target in [
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        assert!(
            cranelift::emit_object_for_target(&program, target)
                .unwrap()
                .len()
                > 100
        );
    }
    let harness = format!(
        r#"
use std::sync::atomic::{{AtomicI32, Ordering}};
static OBSERVED: AtomicI32 = AtomicI32::new(0);
#[unsafe(no_mangle)] pub extern "C" fn external_mixed({}) -> i64 {{ if {} {{ 42 }} else {{ -1 }} }}
#[unsafe(no_mangle)] pub extern "C" fn external_pointer(value: *mut i64) -> *mut i64 {{ value }}
#[unsafe(no_mangle)] pub extern "C" fn external_effect(value: i32) {{ OBSERVED.store(value, Ordering::Relaxed); }}
unsafe extern "C" {{ fn mixed() -> i64; fn pointer(value: *mut i64) -> *mut i64; fn effect(); }}
fn main() {{
    assert_eq!(unsafe {{ mixed() }}, 42);
    let mut number = i64::MIN; let address = &mut number as *mut i64;
    assert_eq!(unsafe {{ pointer(address) }}, address);
    unsafe {{ effect() }};
    assert_eq!(OBSERVED.load(Ordering::Relaxed), -42);
    println!("foreign mixed stack pointer void preserved");
}}
"#,
        parameters.join(", "),
        checks.join(" && ")
    );
    assert_eq!(
        NativeFixture::new().execute(&program, &harness),
        b"foreign mixed stack pointer void preserved\n"
    );
}

#[test]
fn actor_collection_loop_yields_to_sibling_before_finishing() {
    unit_tail_poll(
        "    let first: Pid(()) = spawn(() ->\n        for i in 0..2048:\n            let held = [i, 9223372036854775807]\n            ()\n        match send(reply, \"finished\"):\n            Ok(()) -> ()\n            Err(_) -> ()\n    )\n    ()",
        1,
    );
}

#[test]
fn actor_non_tail_value_helper_yields_and_returns_to_its_caller() {
    use morrow_compiler::{check, lowering, parse};
    let source = r#"
fn sum(n: Int) -> Int:
    if n == 0: 0
    else: n + sum(n - 1)
fn worker():
    println(sum(32) + sum(8))
fn sibling(): println("sibling")
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(sibling)
    ()
"#;
    let program = lowering::lower(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap();
    let harness = r#"unsafe extern "C" { fn morrow_main() -> i32; }
fn main() { assert_eq!(unsafe { morrow_main() }, 0); }"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"sibling\n564\n"
    );
}

#[test]
fn actor_large_finite_helper_yields_and_preserves_its_return_value() {
    use morrow_compiler::{check, lowering, parse};
    let mut source = String::from("fn finite(value: Int) -> Int:\n");
    for index in 0..128 {
        let previous = if index == 0 {
            "value".to_string()
        } else {
            format!("v{}", index - 1)
        };
        source.push_str(&format!("    let v{index} = {previous} + 1\n"));
    }
    source.push_str(
        r#"    v127
fn worker():
    println(finite(9223372036854775679))
fn sibling(): println("sibling")
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(sibling)
    ()
"#,
    );
    let program = lowering::lower(&check::check(&parse::parse(&source).unwrap()).unwrap()).unwrap();
    let harness = r#"unsafe extern "C" { fn morrow_main() -> i32; }
fn main() { assert_eq!(unsafe { morrow_main() }, 0); }"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"sibling\n9223372036854775807\n"
    );
}

#[test]
fn actor_finite_helper_suspends_within_its_body() {
    let mut setup = String::from(
        "    let first: Pid(()) = spawn(() -> finite(reply))\n    ()\nfn finite(reply: Pid(String)):\n    let v0 = 0\n",
    );
    for index in 1..=256 {
        setup.push_str(&format!("    let v{index} = v{} + 1\n", index - 1));
    }
    setup.push_str(
        "    match send(reply, \"finished\"):\n        Ok(()) -> ()\n        Err(_) -> ()\n",
    );
    unit_tail_poll(&setup, 3);
}

#[test]
fn actor_finite_call_tree_charges_transitive_inline_work() {
    let mut setup = String::from(
        "    let first: Pid(()) = spawn(() -> finite(reply))\n    ()\nfn leaf(value: Int) -> Int:\n",
    );
    for index in 0..16 {
        let previous = if index == 0 {
            "value".to_string()
        } else {
            format!("v{}", index - 1)
        };
        setup.push_str(&format!("    let v{index} = {previous} + 1\n"));
    }
    setup.push_str("    v15\nfn pair(value: Int) -> Int: leaf(leaf(value))\nfn finite(reply: Pid(String)):\n    let v0 = 0\n");
    for index in 1..=32 {
        setup.push_str(&format!("    let v{index} = pair(v{})\n", index - 1));
    }
    setup.push_str(
        "    match send(reply, \"finished\"):\n        Ok(()) -> ()\n        Err(_) -> ()\n",
    );
    unit_tail_poll(&setup, 3);
}

#[test]
fn actor_large_strict_operands_suspend_inside_a_statement() {
    let mut setup = String::from(
        "    let first: Pid(()) = spawn(() -> finite(reply))\n    ()\nfn finite(reply: Pid(String)):\n    let v0 = 0\n",
    );
    for index in 1..=8 {
        setup.push_str(&format!(
            "    let v{index} = v{}{}\n",
            index - 1,
            " + 1".repeat(80)
        ));
    }
    setup.push_str(
        "    match send(reply, \"finished\"):\n        Ok(()) -> ()\n        Err(_) -> ()\n",
    );
    unit_tail_poll(&setup, 3);
}

#[test]
fn foreign_source_retains_borrowed_and_interior_strings_across_precise_collection() {
    let source = r#"
foreign "C" fn suffix(value: Ptr(CUInt8)) -> Ptr(CUInt8) as "test_suffix"
foreign "C" fn high() -> CUInt64 as "test_high"
foreign "C" fn check_high(value: CUInt64) -> Bool as "test_check_high"
fn borrow() -> Ptr(CUInt8):
    suffix(("hello " + "🌿 café").as_ptr())
fn main():
    let pointer = borrow()
    match Ptr.to_string(pointer, 64):
        Ok(text) -> println(text)
        Err(error) -> println(error)
    println(Result.is_err(Ptr.to_string(pointer, 2)))
    println(Result.is_err(Ptr.to_string(Ptr.null(), 64)))
    println(check_high(high()))
    println(Result.is_err(CUInt64.to_int(high())))
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let mut inserted = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in function.body.drain(..) {
            let collect = match &statement {
                Statement::Assign {
                    operation: Operation::ForeignCall { .. },
                    ..
                } => true,
                Statement::Assign {
                    operation:
                        Operation::Call {
                            callee: Operand::Symbol(name),
                            ..
                        },
                    ..
                } => machine::bare(name) == "morrow_ffi_read_string",
                _ => false,
            };
            if collect {
                body.push(Statement::Effect(Operation::Call {
                    callee: symbol("morrow_gc_collect_precise"),
                    args: vec![],
                    variadic: None,
                }));
                inserted += 1;
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert!(inserted >= 4);
    let harness = r#"
#[unsafe(no_mangle)] unsafe extern "C" fn test_suffix(value: *const u8) -> *const u8 { unsafe { value.add(6) } }
#[unsafe(no_mangle)] extern "C" fn test_high() -> u64 { u64::MAX - 7 }
#[unsafe(no_mangle)] extern "C" fn test_check_high(value: u64) -> bool { value == u64::MAX - 7 }
unsafe extern "C" { fn morrow_main() -> i32; }
fn main() { assert_eq!(unsafe { morrow_main() }, 0); }
"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        "🌿 café\ntrue\ntrue\ntrue\ntrue\n".as_bytes()
    );
}

#[test]
fn actor_cps_collections_nested_exits_and_receive_keep_lexical_values() {
    let source = r#"
fn loops():
    for (key, value) in %{"λ": [4294967297, -4294967295], "雪": [9223372036854775807]}:
        println(key)
        for item in value:
            if item < 0: continue
            println(item)
            break
    for n in 0..3:
        if n == 1: continue
        for m in [10, 20]:
            if m == 20: break
            println(n * 100 + m)
    for n in 9223372036854775806..=9223372036854775807: println(n)
    for n in 3..1: println("unreachable")
    println("done")
fn receiver():
    for n in [1, 2, 3]:
        let value = receive:
            text -> text
        println(value)
        if n == 2: return ()
    println("unreachable")
fn main():
    let loops: Pid(()) = spawn(loops)
    let receiver: Pid(String) = spawn(receiver)
    match send(receiver, "first"):
        Ok(()) -> ()
        Err(_) -> ()
    match send(receiver, "second"):
        Ok(()) -> ()
        Err(_) -> ()
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    let output = NativeFixture::new().execute_linked(
        &program,
        harness,
        &[core_runtime_archive().into_os_string()],
    );
    // Ignore only cross-actor ordering here; each independent actor's exact lexical sequence matters.
    let output = String::from_utf8(output).unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(
        lines
            .iter()
            .copied()
            .filter(|line| !["first", "second"].contains(line))
            .collect::<Vec<_>>(),
        [
            "λ",
            "4294967297",
            "雪",
            "9223372036854775807",
            "10",
            "210",
            "9223372036854775806",
            "9223372036854775807",
            "done"
        ]
    );
    assert_eq!(
        lines
            .iter()
            .copied()
            .filter(|line| ["first", "second"].contains(line))
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

fn actor_force_collection_before_suspension(program: &mut Program) {
    let mut points = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if ["morrow_managed_continue", "morrow_managed_receive", "morrow_managed_scope_enter", "morrow_managed_scope_defer", "morrow_managed_scope_leave"].contains(&machine::bare(name)))
            {
                body.push(Statement::Effect(Operation::Call {
                    callee: symbol("morrow_gc_collect_precise"),
                    args: vec![],
                    variadic: None,
                }));
                points += 1;
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert!(points > 0);
}

#[test]
fn compiled_parallel_actors_preserve_fifo_full_width_payloads_and_precise_roots() {
    let source = r#"
type Message:
    Data(Int, String, List(Int))
fn receiver():
    for index in 0..8:
        receive:
            Data(number, text, values) ->
                println(number)
                println(String.len(text))
                println(List.head(values))
fn sender(target: Pid(Message)):
    for index in 0..8:
        let text = String.repeat("🌿", 2048)
        match send(target, Data(index, text, [-9223372036854775808])):
            Ok(()) -> ()
            Err(code) -> println(code)
fn main():
    let first: Pid(()) = spawn(() -> ())
    let target: Pid(Message) = spawn(receiver)
    let source: Pid(()) = spawn(() -> sender(target))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = r#"unsafe extern "C" { fn morrow_main() -> i32; }
fn main() {
    // No other thread exists before the native runtime starts its workers.
    unsafe {
        std::env::set_var("MORROW_SCHEDULERS", "3");
        std::env::set_var("MORROW_WORK_STEALING", "@STEALING@");
        std::env::set_var("MORROW_REDUCTIONS", "@REDUCTIONS@");
    }
    assert_eq!(unsafe { morrow_main() }, 0);
}"#;
    let expected: String = (0..8)
        .map(|index| format!("{index}\n8192\n-9223372036854775808\n"))
        .collect();
    for (stealing, reductions) in [("0", "1"), ("1", "8")] {
        let harness = harness
            .replace("@STEALING@", stealing)
            .replace("@REDUCTIONS@", reductions);
        assert_eq!(
            NativeFixture::new().execute_linked(
                &program,
                &harness,
                &[core_runtime_archive().into_os_string()]
            ),
            expected.as_bytes(),
            "stealing={stealing}, reductions={reductions}"
        );
    }
}

#[test]
fn compiled_actor_main_keeps_exec_live_across_precise_heap_zero_collection() {
    let source = "fn worker(): println(\"worker\")\nfn main():\n    let pid: Pid(()) = spawn(worker)\n    ()\n";
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let main = program
        .functions
        .iter_mut()
        .find(|function| machine::bare(&function.name) == "morrow_main")
        .unwrap();
    let run = main
        .body
        .iter()
        .position(|statement| {
            matches!(statement,
                Statement::Effect(Operation::Call { callee: Operand::Symbol(name), .. })
                    if machine::bare(name) == "morrow_managed_run"
            )
        })
        .unwrap();
    let pressure = [
        assign(
            "exec_gc_before",
            Scalar::I64,
            Operation::Call {
                callee: symbol("morrow_gc_heap_size"),
                args: vec![],
                variadic: None,
            },
        ),
        Statement::Effect(Operation::Call {
            callee: symbol("morrow_gc_collect_precise"),
            args: vec![],
            variadic: None,
        }),
        assign(
            "exec_gc_after",
            Scalar::I64,
            Operation::Call {
                callee: symbol("morrow_gc_heap_size"),
                args: vec![],
                variadic: None,
            },
        ),
        assign(
            "exec_gc_reclaimed",
            Scalar::I64,
            Operation::Binary(BinaryOp::Sub, temp("exec_gc_before"), temp("exec_gc_after")),
        ),
        Statement::Effect(Operation::Call {
            callee: symbol("morrow_println_int"),
            args: vec![(Scalar::I64, temp("exec_gc_reclaimed"))],
            variadic: None,
        }),
    ];
    main.body.splice(run..run, pressure);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        // Only source main's zero-capture closure (one identity word) and its
        // unused PID (four ABI words) die. The three-word Exec must survive.
        b"40\nworker\n"
    );
}

#[test]
fn actor_cps_seeded_value_returns_preserve_full_width_and_unicode_under_precise_gc() {
    let mut source = String::from(
        r#"
fn sum(n: Int, held: (Int, String, List(Int))) -> (Int, String, List(Int)):
    if n == 0: held
    else:
        let returned = sum(n - 1, held)
        (returned.0 + n, returned.1, returned.2)
fn worker():
"#,
    );
    let mut expected = String::from("sibling\n");
    let mut seed = 0x4645524e_u64;
    for _ in 0..64 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let n = (seed % 17) as i64;
        source.push_str(&format!("    let result = sum({n}, (4294967297, String.repeat(\"λ雪\", 2), [-9223372036854775808]))\n    println(result.0)\n    println(result.1)\n    println(List.head(result.2))\n"));
        expected.push_str(&format!(
            "{}\nλ雪λ雪\n-9223372036854775808\n",
            4294967297_i64 + n * (n + 1) / 2
        ));
    }
    source.push_str("fn main():\n    let first: Pid(()) = spawn(worker)\n    let second: Pid(()) = spawn(() -> println(\"sibling\"))\n    ()\n");
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(&source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        expected.as_bytes()
    );
}

#[test]
fn actor_cps_strict_order_short_circuit_and_early_helper_return() {
    let source = r#"
fn sum(n: Int) -> Int:
    if n == 0: 0
    else: n + sum(n - 1)
fn mark() -> Int:
    println("left")
    100
fn early() -> Int:
    for n in 0..100:
        if n == 3: return n
    99
fn worker():
    println(mark() + sum(4))
    if false and (sum(-1) > 0): println("unreachable")
    if true or (sum(-1) > 0): println("short")
    println(early())
    println("done")
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"left\nsibling\n110\nshort\n3\ndone\n"
    );
}

#[test]
fn actor_cps_faults_and_return_frame_limits_are_supervised_without_starving_siblings() {
    use morrow_compiler::native_library::{self, Export};
    for body in [
        "if n == 0: 1 / n\n    else: 1 + work(n - 1)",
        "1 + work(n + 1)",
    ] {
        for supervised in [false, true] {
            let spawn = if supervised {
                "supervise(worker, 1)"
            } else {
                "spawn(worker)"
            };
            let source = format!(
                r#"
fn work(n: Int) -> Int:
    {body}
fn worker():
    println("attempt")
    println(work(8))
pub fn start() -> ():
    let first: Pid(()) = {spawn}
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#
            );
            let checked = morrow_compiler::check::check_library(
                &morrow_compiler::parse::parse(&source).unwrap(),
            )
            .unwrap();
            let mut program =
                native_library::lower(&checked, &[Export::new("start", "start")]).unwrap();
            actor_force_collection_before_suspension(&mut program);
            let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_close(exec: usize);
}
fn main() {
    let mut fault = Box::new(0);
    unsafe {
        let exec = morrow_library_open(&mut *fault);
        assert_ne!(exec, 0);
        morrow_export_start(&mut *fault, exec);
        let mut finished = false;
        let mut seed = 0x4645524e_u64;
        for _ in 0..4096 {
            seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
            let status = morrow_managed_poll(exec, (seed % 7 + 1) as i64);
            if status == TERMINAL { finished = true; break; }
            assert_eq!(status, 2);
        }
        assert!(finished, "bounded return-frame accounting or checked arithmetic must fail deterministically");
        assert_eq!(*fault == 0, HANDLED);
        morrow_managed_close(exec);
    }
}
"#.replace("TERMINAL", if supervised { "0" } else { "3" }).replace("HANDLED", if supervised { "true" } else { "false" });
            assert_eq!(
                NativeFixture::new().execute_linked(
                    &program,
                    &harness,
                    &[core_runtime_archive().into_os_string()]
                ),
                if supervised {
                    b"attempt\nsibling\nattempt\n".as_slice()
                } else {
                    b"attempt\nsibling\n".as_slice()
                }
            );
        }
    }
}

#[test]
fn actor_cps_let_else_suspends_before_destructuring_and_preserves_failure_return() {
    let source = r#"
fn value(n: Int) -> Option(Int):
    if n == 0: Some(42)
    else: value(n - 1)
fn worker():
    let Some(found) = value(32) else: return ()
    println(found)
    let missing: Option(Int) = None
    let Some(unused) = missing else: return ()
    println(unused)
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"sibling\n42\n"
    );
}

#[test]
fn actor_defer_survives_receive_and_tail_call_until_logical_return() {
    let source = r#"
fn child():
    defer println("child cleanup")
    receive:
        () -> println("body")
fn parent():
    defer println("parent cleanup")
    receive:
        () -> child()
fn main():
    let child: Pid(()) = spawn(parent)
    match send(child, ()):
        Ok(()) -> ()
        Err(_) -> ()
    match send(child, ()):
        Ok(()) -> ()
        Err(_) -> ()
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"body\nchild cleanup\nparent cleanup\n"
    );
}

#[path = "support/custom_json_backend.rs"]
mod custom_json_backend;

#[test]
fn actor_defer_loop_snapshots_and_tail_returns_preserve_value_roots() {
    let source = r#"
fn tail(n: Int) -> String:
    defer println(n)
    if n == 0: String.repeat("λ", 2)
    else: tail(n - 1)
fn worker():
    for i in 0..3:
        let held = String.repeat("雪", i + 1)
        defer println(held)
        defer println(i + 10)
    println(tail(3))
    println("body end")
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        "sibling\n0\n1\n2\n3\nλλ\nbody end\n12\n雪雪雪\n11\n雪雪\n10\n雪\n".as_bytes()
    );
}

fn actor_cleanup_host(source: &str, body: &str, precise: bool) -> Vec<u8> {
    use morrow_compiler::native_library::{self, Export};
    let checked =
        morrow_compiler::check::check_library(&morrow_compiler::parse::parse(source).unwrap())
            .unwrap();
    let mut program = native_library::lower(&checked, &[Export::new("start", "start")]).unwrap();
    if precise {
        actor_force_collection_before_suspension(&mut program);
    }
    let harness = format!(
        r#"
unsafe extern "C" {{
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_collect_precise();
}}
fn main() {{
    let mut fault = Box::new(0);
    unsafe {{
        let exec = morrow_library_open(&mut *fault);
        assert_ne!(exec, 0);
        morrow_export_start(&mut *fault, exec);
        assert_eq!(*fault, 0);
        {body}
        morrow_managed_close(exec);
    }}
}}
"#
    );
    NativeFixture::new().execute_linked(
        &program,
        &harness,
        &[core_runtime_archive().into_os_string()],
    )
}

#[test]
fn actor_defer_fault_unwinds_every_activation_and_preserves_the_body_fault() {
    let source = r#"
fn bad_cleanup():
    defer println("nested cleanup")
    println("cleanup fault")
    let empty: List(Int) = []
    println(List.head(empty))
fn nested(n: Int) -> Int:
    defer println(n)
    if n == 0:
        defer bad_cleanup()
        1 / n
    else: 1 + nested(n - 1)
pub fn start() -> ():
    let first: Pid(()) = spawn(() ->
        defer println("outer")
        println(nested(3))
    )
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let body = r#"
let mut ended = false;
let mut seed = 0x4645524e_u64;
for _ in 0..256 {
    seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
    let status = morrow_managed_poll(exec, (seed % 7 + 1) as i64);
    if status == 3 { ended = true; break; }
    assert_eq!(status, 2);
}
assert!(ended);
assert_eq!(*fault, 1, "division is primary; cleanup's empty-list fault is secondary");
"#;
    assert_eq!(
        actor_cleanup_host(source, body, true),
        b"sibling\ncleanup fault\nnested cleanup\n0\n1\n2\n3\nouter\n"
    );
}

#[test]
fn actor_defer_cancellation_drains_suspended_scopes_and_preserves_prior_faults() {
    let source = r#"
fn bad_cleanup():
    defer println("nested cleanup")
    println("cleanup fault")
    let empty: List(Int) = []
    println(List.head(empty))
fn worker():
    defer println("oldest")
    for i in 0..3: defer println(i)
    defer bad_cleanup()
    receive:
        () -> println("unreachable")
pub fn start() -> ():
    let first: Pid(()) = spawn(worker)
    ()
"#;
    for prior in [0, 1] {
        let body = format!(
            r#"
let mut idle = false;
for _ in 0..64 {{
    let status = morrow_managed_poll(exec, 1);
    if status == 1 {{ idle = true; break; }}
    assert_eq!(status, 2);
}}
assert!(idle);
morrow_gc_collect_precise();
*fault = {prior};
morrow_managed_close(exec);
assert_eq!(*fault, {}, "cancellation keeps the first failure and drains every cleanup");
"#,
            if prior == 0 { 4 } else { prior }
        );
        assert_eq!(
            actor_cleanup_host(source, &body, true),
            b"cleanup fault\nnested cleanup\n2\n1\n0\noldest\n"
        );
    }
}

#[test]
fn actor_defer_registration_limit_drains_all_admitted_callbacks_once() {
    let source = r#"
fn worker():
    for i in 0..5000: defer println(i)
pub fn start() -> ():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let body = r#"
let mut failed = false;
let mut seed = 0x4645524e_u64;
for _ in 0..4096 {
    seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
    let status = morrow_managed_poll(exec, (seed % 7 + 1) as i64);
    if status == 3 { failed = true; break; }
    assert_eq!(status, 2);
}
assert!(failed);
assert_eq!(*fault, 9);
"#;
    let mut expected = String::from("sibling\n");
    // One logical scope plus 4095 registered nodes exhausts the explicit 4096-entry budget.
    for i in (0..4095).rev() {
        expected.push_str(&format!("{i}\n"));
    }
    assert_eq!(actor_cleanup_host(source, body, false), expected.as_bytes());
}

#[test]
fn actor_with_result_steps_suspend_and_preserve_logical_cleanup() {
    let source = include_str!("actors/with_cps.mr");
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"await cleanup\nawait cleanup\n7\nworker cleanup\nbad\nworker cleanup\n"
    );
}

#[test]
fn actor_dynamic_captured_call_yields_through_recursive_helper_and_returns_value() {
    let source = r#"
fn sum(n: Int) -> Int:
    if n == 0: 0
    else: n + sum(n - 1)
fn apply(action: (Int) -> Int, value: Int) -> Int: action(value)
fn worker():
    let offset = 4294967297
    let action = (n: Int) -> sum(n) + offset
    println(apply(action, 32))
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"sibling\n4294967825\n"
    );
}

#[path = "support/language_tour_backend.rs"]
mod language_tour_backend;

#[test]
fn actor_higher_order_fold_yields_between_elements_and_recursive_callbacks() {
    let source = r#"
fn sum(n: Int) -> Int:
    if n == 0: 0
    else: n + sum(n - 1)
fn worker():
    println(List.fold([1, 2, 3, 4], 4294967297, (acc: Int, n: Int) -> acc + sum(n)))
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"sibling\n4294967317\n"
    );
}

#[test]
fn actor_result_sequencing_preserves_full_payloads_and_fairness_under_collection() {
    let mut source = include_str!("actors/result_cps.mr").to_owned();
    source.push_str("\nfn main():\n");
    let mut expected = vec!["sibling".to_owned()];
    for mode in 0..3 {
        for n in [-1, 0, 2] {
            let id = mode * 4 + n + 1;
            source.push_str(&format!(
                "    let pid{id}: Pid(Int) = spawn(() -> worker(mode: {mode}, n: {n}))\n"
            ));
            for _ in 0..2 {
                source.push_str(&format!(
                    "    match send(pid{id}, 1):\n        Ok(()) -> ()\n        Err(_) -> ()\n"
                ));
            }
            let kind = ["try", "with", "handled"][mode as usize];
            let result = if n < 0 {
                format!("bad {n}")
            } else if mode == 2 && n == 0 {
                "bad -9".to_owned()
            } else {
                format!("🌿 {}", n + i64::from(mode != 0))
            };
            expected.push(format!("{kind} {n}: {result}"));
            expected.push(format!("{kind} cleanup {n}"));
        }
    }
    source.push_str("    let sibling: Pid(()) = spawn(() -> println(\"sibling\"))\n    ()\n");
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(&source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    let bytes = NativeFixture::new().execute_linked(
        &program,
        harness,
        &[core_runtime_archive().into_os_string()],
    );
    let output = String::from_utf8(bytes).unwrap();
    assert!(output.starts_with("sibling\n"), "{output}");
    let mut actual = output.lines().map(str::to_owned).collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    for kind in ["try", "with"] {
        for n in [-1, 0, 2] {
            assert!(
                output.find(&format!("{kind} cleanup {n}\n")).unwrap()
                    < output.find(&format!("{kind} {n}:")).unwrap()
            );
        }
    }
}

#[test]
fn actor_sum_callbacks_preserve_eager_factories_and_lazy_calls_under_collection() {
    let source = include_str!("actors/sums_cps.mr");
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        include_bytes!("actors/sums_cps.stdout")
    );
}

#[test]
fn actor_higher_order_seeded_native_models_preserve_order_and_scalar_widths() {
    for mut seed in [0x4645524e_u64, 17, 991] {
        let mut values = Vec::new();
        for _ in 0..24 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            values.push((seed % 31) as i64 - 15);
        }
        let literal = values
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            r#"
fn depth(n: Int, value: Int) -> Int:
    if n == 0: value
    else: depth(n: n - 1, value: value)
fn worker():
    let values = [{literal}]
    let offset = 4294967297
    let mapped = List.map(values, (value: Int) -> depth(n: 3, value: value + offset))
    for value in mapped: println(value)
    let kept = List.filter(mapped, (value: Int) -> value % 2 == 0)
    for value in kept: println(value)
    println(List.fold(kept, 0, (acc: Int, value: Int) -> depth(n: 2, value: acc + value)))
    println(Option.unwrap_or(List.find(mapped, (value: Int) -> value > offset), -1))
    println(List.any(mapped, (value: Int) -> value > offset))
    println(List.all(mapped, (value: Int) -> value > offset))
    let empty: List(Int) = []
    println(List.len(List.map(empty, (value: Int) -> 1 / value)))
    println(List.any(empty, (value: Int) -> 1 / value == 0))
    println(List.all(empty, (value: Int) -> 1 / value == 0))
    println(Option.unwrap_or(List.find(empty, (value: Int) -> 1 / value == 0), -99))
    let floats = List.map([0.5, -2.25, 17.125], (value: Float) -> value * 2.0)
    for value in floats: println(value)
    let suffix = "🦀"
    let names = List.map(["é", "雪", "morrow"], (value: String) -> value + suffix)
    for value in names: println(value)
fn main():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#
        );
        let mut expected = String::from("sibling\n");
        let mapped: Vec<_> = values.iter().map(|value| value + 4294967297_i64).collect();
        for value in &mapped {
            expected.push_str(&format!("{value}\n"));
        }
        let kept: Vec<_> = mapped
            .iter()
            .copied()
            .filter(|value| value % 2 == 0)
            .collect();
        for value in &kept {
            expected.push_str(&format!("{value}\n"));
        }
        expected.push_str(&format!(
            "{}\n{}\n{}\n{}\n0\nfalse\ntrue\n-99\n1\n-4.5\n34.25\né🦀\n雪🦀\nmorrow🦀\n",
            kept.iter().sum::<i64>(),
            mapped
                .iter()
                .copied()
                .find(|value| *value > 4294967297)
                .unwrap_or(-1),
            mapped.iter().any(|value| *value > 4294967297),
            mapped.iter().all(|value| *value > 4294967297)
        ));
        let checked =
            morrow_compiler::check::check(&morrow_compiler::parse::parse(&source).unwrap())
                .unwrap();
        let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
        actor_force_collection_before_suspension(&mut program);
        let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
        assert_eq!(
            NativeFixture::new().execute_linked(
                &program,
                harness,
                &[core_runtime_archive().into_os_string()]
            ),
            expected.as_bytes()
        );
    }
}

#[test]
fn actor_dynamic_returned_aliases_and_higher_order_short_circuit_keep_original_abi() {
    let source = r#"
fn sum(n: Int) -> Int:
    if n == 0: 0
    else: n + sum(n - 1)
fn make(offset: Int) -> (Int) -> Int: (n: Int) -> sum(n) + offset
fn worker(action: (Int) -> Int):
    println(action(12))
    println(List.any([1, 0], (value: Int) -> 1 / value == 1))
    println(List.all([1, 0], (value: Int) -> 1 / value == 0))
    println(Option.unwrap_or(List.find([1, 0], (value: Int) -> 1 / value == 1), -1))
    println(List.len(List.map([1, 2, 3], (value: Int) -> ())))
fn main():
    let original = make(4294967297)
    println(original(2))
    let aliased = original
    let first: Pid(()) = spawn(() -> worker(aliased))
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    actor_force_collection_before_suspension(&mut program);
    let harness = "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"4294967300\nsibling\n4294967375\ntrue\nfalse\n1\n3\n"
    );
}

#[test]
fn actor_dynamic_callback_fault_drains_activations_without_invoking_later_elements() {
    let source = r#"
fn broken(n: Int) -> Int:
    defer println(n)
    if n == 0: 1 / n
    else: broken(n - 1)
fn worker():
    defer println("worker cleanup")
    let callback = broken
    let values = List.map([2, 3], callback)
    println(List.len(values))
pub fn start() -> ():
    let first: Pid(()) = spawn(worker)
    let second: Pid(()) = spawn(() -> println("sibling"))
    ()
"#;
    let body = r#"
let mut seed = 19_u64;
let mut failed = false;
for _ in 0..100 {
    seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
    let status = morrow_managed_poll(exec, (seed % 5 + 1) as i64);
    if status == 3 { failed = true; break; }
    assert_eq!(status, 2);
}
assert!(failed);
assert_eq!(*fault, 1);
"#;
    assert_eq!(
        actor_cleanup_host(source, body, true),
        b"sibling\n0\n1\n2\nworker cleanup\n"
    );
}
#[path = "support/immutable_gc_backend.rs"]
mod immutable_gc_backend;

#[path = "support/bounded_lists_backend.rs"]
mod bounded_lists_backend;
#[path = "support/callback_inlining_backend.rs"]
mod callback_inlining_backend;
#[path = "support/constant_arithmetic_backend.rs"]
mod constant_arithmetic_backend;

#[path = "actor_frame_reuse/native.rs"]
mod actor_frame_reuse;
