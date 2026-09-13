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

fn core_runtime_archive() -> std::path::PathBuf {
    let archive = std::env::var_os("FERN_RUNTIME_CORE_LIB")
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
                .join("libfern_runtime.a")
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = fern_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    let position = function.body.iter().position(|statement| matches!(statement,
        Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
        if machine::bare(name) == "fern_str_len"
    )).unwrap();
    // The missing-root branch avoids dereferencing freed memory in the negative
    // control. Only generated code holds the dynamic allocation across this GC.
    function.body.splice(
        position..position,
        [
            Statement::Effect(Operation::Call {
                callee: symbol("fern_gc_collect_precise"),
                args: vec![],
                variadic: None,
            }),
            assign(
                "gc_bytes",
                Scalar::I64,
                Operation::Call {
                    callee: symbol("fern_gc_heap_size"),
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
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { println!(\"{}\", unsafe { fern_main() }); }";
    assert_eq!(
        NativeFixture::new().execute_linked(&program, harness, &[archive.clone().into_os_string()]),
        b"11\n"
    );

    // Independently prove this oracle fails safely when explicit registration
    // is absent, even though live pointer copies remain in native stack slots.
    for function in &mut program.functions {
        for statement in &mut function.body {
            if let Statement::Assign { operation, .. } = statement
                && matches!(operation, Operation::Call { callee: Operand::Symbol(name), .. } if machine::bare(name) == "fern_gc_frame_enter")
            {
                *operation = Operation::Unary(UnaryOp::Copy, Operand::Int(0));
            }
        }
        function.body.retain(|statement| !matches!(statement,
            Statement::Effect(Operation::Call { callee: Operand::Symbol(name), .. }) if machine::bare(name) == "fern_gc_frame_leave"
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = fern_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    function.export = true;
    function.body.insert(
        1,
        Statement::Effect(Operation::Call {
            callee: symbol("fern_gc_collect_precise"),
            args: vec![],
            variadic: None,
        }),
    );
    let harness = "unsafe extern \"C\" { fn f0(env:usize,fault:*mut i64,value:i64)->i64; fn fern_alloc(size:usize)->usize; fn fern_gc_heap_size()->usize; } fn main() { let mut fault=0; let address=unsafe{fern_alloc(32)}; let value=unsafe{f0(0,&mut fault,address as i64)}; assert_eq!(value,address as i64); assert_eq!(unsafe{fern_gc_heap_size()},0); for n in [i64::MIN,i64::MAX] { assert_eq!(unsafe{f0(0,&mut fault,n)},n); } println!(\"precise integers preserved\"); }";
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = fern_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    let position = function.body.iter().position(|statement| matches!(statement,
        Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
        if machine::bare(name) == "fern_str_len"
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
                    callee: symbol("fern_gc_heap_size"),
                    args: vec![],
                    variadic: None,
                },
            ),
            Statement::Effect(Operation::Call {
                callee: symbol("fern_gc_collect_precise"),
                args: vec![],
                variadic: None,
            }),
            assign(
                "gc_after",
                Scalar::I64,
                Operation::Call {
                    callee: symbol("fern_gc_heap_size"),
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
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { println!(\"code={}\", unsafe { fern_main() }); }";
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
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
    use fern_compiler::native_library::{self, Export};
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
        fern_compiler::check::check_library(&fern_compiler::parse::parse(&source).unwrap())
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
    fn fern_library_open(fault: *mut i64) -> usize;
    fn fern_library_string_port(exec: usize) -> usize;
    fn fern_export_start(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn fern_export_queue_sibling(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn fern_managed_poll(exec: usize, steps: i64) -> i64;
    fn fern_managed_port_peek_len(exec: usize, port: usize) -> i64;
    fn fern_managed_port_read(exec: usize, port: usize, output: *mut u8, capacity: usize) -> i64;
    fn fern_managed_close(exec: usize);
    fn fern_gc_frame_enter(slots: *const usize, words: usize) -> usize;
    fn fern_gc_frame_leave(token: usize);
}
fn main() {
    let mut fault = Box::new(0);
    unsafe {
        let exec = fern_library_open(&mut *fault);
        assert_ne!(exec, 0);
        let port = fern_library_string_port(exec);
        let root = fern_gc_frame_enter(&port, 1);
        fern_export_start(&mut *fault, exec, port);
        assert_eq!(*fault, 0);
        for _ in 0..WARMUP {
            assert_eq!(fern_managed_poll(exec, 1), 2);
            assert_eq!(fern_managed_port_peek_len(exec, port), -1, "the busy helper must suspend before it finishes");
        }
        fern_export_queue_sibling(&mut *fault, exec, port);
        for _ in 0..3 {
            assert_eq!(fern_managed_poll(exec, 1), 2);
            if fern_managed_port_peek_len(exec, port) >= 0 { break; }
        }
        let mut bytes = [0u8; 16];
        let count = fern_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
        assert_eq!(count, 7, "sibling must make progress within four callbacks");
        assert_eq!(&bytes[..count as usize], b"sibling");
        assert_eq!(*fault, 0);
        fern_managed_close(exec);
        fern_gc_frame_leave(root);
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
    use fern_compiler::native_library::{self, Export};
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
        fern_compiler::check::check_library(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = native_library::lower(&checked, &[Export::new("start", "start")]).unwrap();
    let harness = r#"
unsafe extern "C" {
    fn fern_library_open(fault: *mut i64) -> usize;
    fn fern_export_start(fault: *mut i64, exec: usize) -> i32;
    fn fern_managed_poll(exec: usize, steps: i64) -> i64;
    fn fern_managed_close(exec: usize);
}
fn main() {
    let mut fault = Box::new(0);
    unsafe {
        let exec = fern_library_open(&mut *fault);
        assert_ne!(exec, 0);
        fern_export_start(&mut *fault, exec);
        assert_eq!(fern_managed_poll(exec, 1), 2);
        println!("yielded");
        assert_eq!(fern_managed_poll(exec, 1), 2);
        assert_eq!(*fault, 0);
        fern_managed_close(exec);
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
    let text = String.repeat("fern", 2)
    let busy: Pid(Int) = spawn(() -> left(remaining: 100000, text: text, boundary: -9223372036854775808))
    let sibling: Pid(String) = spawn(() -> println("sibling"))
    ()
"#;
    let checked =
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = fern_compiler::lowering::lower(&checked).unwrap();
    let mut safepoints = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "fern_managed_continue")
            {
                body.push(Statement::Effect(Operation::Call {
                    callee: Operand::Symbol("fern_gc_collect_precise".into()),
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
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"ordinary\n9223372036854775807\nsibling\nfernfern\n-9223372036854775808\n"
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
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
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
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
    use fern_compiler::{
        check,
        native_library::{self, Export},
        parse,
    };
    let source = format!(
        "{}\n{}",
        include_str!("../../../examples/web/checklist.fn"),
        include_str!("fixtures/web_feedback.fn")
    );
    let checked = check::check_library(&parse::parse(&source).unwrap()).unwrap();
    let program =
        native_library::lower(&checked, &[Export::new("feedback_trace", "feedback_trace")])
            .unwrap();
    let harness = r#"
unsafe extern "C" {
    fn fern_export_feedback_trace(fault: *mut i64, exec: usize, input: i64) -> i64;
}
fn main() {
    let mut fault = 0;
    for (index, expected) in [1, 1, 0, 1, 1, 1, 1, 256, 256, 1, 16, 11].into_iter().enumerate() {
        assert_eq!(unsafe { fern_export_feedback_trace(&mut fault, 0, index as i64) }, expected, "feedback trace {index}");
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
    let source = include_str!("fixtures/actor_map.fn");
    let checked =
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let program = fern_compiler::lowering::lower(&checked).unwrap();
    let harness = "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() },0); }";
    assert_eq!(NativeFixture::new().execute_linked(&program, harness, &[core_runtime_archive().into_os_string()]), b"2\n-9223372036854775808\n9223372036854775807\n0\n2\n-9223372036854775808\n9223372036854775807\n");
}
