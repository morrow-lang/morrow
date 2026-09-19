//! Managed actor lowering keeps logical functions separate from resumable continuation entries.
use super::*;
use crate::actors::{Lowered, Operation};
#[path = "actors/control_types.rs"]
mod control_types;
#[path = "actors/descriptors.rs"]
mod descriptors;
#[path = "actors/lower.rs"]
mod lower;
#[path = "actors/tail_helpers.rs"]
mod tail_helpers;
#[path = "actors/validate.rs"]
mod validate;

#[derive(Default)]
pub(super) struct Plan {
    pub(super) active: bool,
    pub(super) managed: BTreeSet<usize>,
    types: BTreeMap<Type, usize>,
    functions: Vec<Function>,
    steps: BTreeMap<usize, Type>,
    selectors: BTreeMap<usize, Type>,
    pub(super) entries: BTreeMap<usize, usize>,
    // Ordinary callable identities remain executable; only their actor callback
    // selects these separate resumable copies.
    pub(super) helpers: BTreeMap<usize, usize>,
    generic_steps: BTreeSet<usize>,
}

/// Prepared original layouts remain unchanged when lowering appends private functions.
pub(super) struct Prepared<'a> {
    pub(super) program: std::borrow::Cow<'a, ir::Program>,
    pub(super) plan: Plan,
    pub(super) layouts: HashMap<Type, &'a ir::TypeLayout>,
}

/// Prepare private continuations only after the original public tree passes bounded type preflight.
pub(super) fn prepare(program: &ir::Program) -> Lowering<Prepared<'_>> {
    validate::shape(program)?;
    union_validation::preflight(program)?;
    let layouts = nominal::layouts(&program.types)?;
    union_validation::references(program, &layouts)?;
    validate::program(program, &layouts)?;
    let managed = crate::actors::contracts::validate(program)?;
    let mut plan = lower::program(program, &layouts)?;
    if plan.functions.is_empty() {
        plan.active = !managed.is_empty();
        plan.managed = managed;
        return Ok(Prepared {
            program: std::borrow::Cow::Borrowed(program),
            plan,
            layouts,
        });
    }
    let mut program = program.clone();
    program.functions.append(&mut plan.functions);
    if !plan.entries.is_empty() && program.functions.len() > 4096 {
        return Err(invalid(
            Span::default(),
            "actor continuation descriptor count limit exceeded",
        ));
    }
    plan.managed = crate::actors::contracts::effects(&program)?;
    plan.active = !plan.managed.is_empty();
    // Original layouts are immutable; validate only the new combined expression tree again.
    union_validation::preflight(&program)?;
    union_validation::references(&program, &layouts)?;
    Ok(Prepared {
        program: std::borrow::Cow::Owned(program),
        plan,
        layouts,
    })
}

impl Emitter<'_> {
    /// Export only descriptor-backed host constructors for managed libraries.
    pub(super) fn actor_library(&mut self) -> Lowering<()> {
        if !self.actors.active {
            return Ok(());
        }
        let string = self.actor_type(&Type::String)?;
        for (name, parameter, callee, args) in [
            (
                "$morrow_library_open",
                "%fault",
                "$morrow_managed_open",
                vec![
                    (Scalar::I64, native_operand("%fault")),
                    (Scalar::I64, native_operand("$actor_functions")),
                    (
                        Scalar::I64,
                        native_operand(&self.functions.len().to_string()),
                    ),
                ],
            ),
            (
                "$morrow_library_string_port",
                "%exec",
                "$morrow_managed_port",
                vec![
                    (Scalar::I64, native_operand("%exec")),
                    (Scalar::I64, native_operand(&string)),
                ],
            ),
        ] {
            self.output.begin(
                name,
                Some(Scalar::I64),
                vec![(Scalar::I64, parameter.into())],
                true,
            );
            self.output.statement(Statement::Label("@start".into()));
            self.output.statement(Statement::Assign {
                destination: "%result".into(),
                ty: Scalar::I64,
                operation: NativeOperation::Call {
                    callee: native_operand(callee),
                    args,
                    variadic: None,
                },
            });
            self.output
                .statement(Statement::Return(Some(native_operand("%result"))));
            self.output.end();
        }
        Ok(())
    }

    /// Lower validated actor operations with the caller's separate execution context.
    pub(super) fn actor_expression(
        &mut self,
        actor: &ir::ActorExpr,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        match actor {
            ir::ActorExpr::Spawn {
                entry,
                mailbox,
                max_restarts,
            } => {
                let (_, params, result) = crate::actors::function(&entry.ty)
                    .ok_or_else(|| invalid(span, "spawn entry must be callable"))?;
                if !params.is_empty() || *result != Type::Unit {
                    return Err(invalid(
                        span,
                        "spawn entry must take no arguments and return Unit",
                    ));
                }
                let entry = self.expr(entry, locals, depth)?;
                let descriptor = self.actor_type(mailbox)?;
                let mut arguments = vec![
                    (Scalar::I64, native_operand("%exec")),
                    (Scalar::I64, native_operand(&entry)),
                    (Scalar::I64, native_operand(&descriptor)),
                ];
                if let Some(budget) = max_restarts {
                    let budget = self.expr(budget, locals, depth)?;
                    arguments.push((Scalar::I64, native_operand(&budget)));
                }
                let ty = Type::Pid(Box::new(mailbox.clone()));
                let value = self.assign(
                    locals,
                    ty.clone(),
                    NativeOperation::Call {
                        callee: native_operand(if max_restarts.is_some() {
                            "$morrow_managed_supervise"
                        } else {
                            "$morrow_managed_spawn"
                        }),
                        args: arguments,
                        variadic: None,
                    },
                );
                self.guard_fault(locals);
                Ok((ty, value))
            }
            ir::ActorExpr::SupervisedCurrent { pid } => {
                let ty = Type::Result(Box::new(pid.ty.clone()), Box::new(Type::Int));
                let pid = self.expr(pid, locals, depth)?;
                let value = self.assign(
                    locals,
                    ty.clone(),
                    NativeOperation::Call {
                        callee: native_operand("$morrow_managed_supervised_current"),
                        args: vec![
                            (Scalar::I64, native_operand("%exec")),
                            (Scalar::I64, native_operand(&pid)),
                        ],
                        variadic: None,
                    },
                );
                self.guard_fault(locals);
                Ok((ty, value))
            }
            ir::ActorExpr::Send { pid, message } => {
                expect_type(
                    pid.ty.clone(),
                    Type::Pid(Box::new(message.ty.clone())),
                    span,
                )?;
                let pid = self.expr(pid, locals, depth)?;
                let value = self.expr(message, locals, depth)?;
                let value = self.payload(locals, &message.ty, value);
                let descriptor = self.actor_type(&message.ty)?;
                let ty = Type::Result(Box::new(Type::Unit), Box::new(Type::Int));
                let result = self.assign(
                    locals,
                    ty.clone(),
                    NativeOperation::Call {
                        callee: native_operand("$morrow_managed_send"),
                        args: vec![
                            (Scalar::I64, native_operand("%exec")),
                            (Scalar::I64, native_operand(&(pid))),
                            (Scalar::I64, native_operand(&(value))),
                            (Scalar::I64, native_operand(&(descriptor))),
                        ],
                        variadic: None,
                    },
                );
                self.guard_fault(locals);
                Ok((ty, result))
            }
            ir::ActorExpr::Lowered(value) => self.actor_operation(&value.operation, locals, depth),
            _ => Err(invalid(span, "unconverted actor suspension")),
        }
    }

    /// Private continuation operations publish only known closure identities after selection.
    fn actor_operation(
        &mut self,
        op: &Operation,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let value = match op {
            Operation::ListBuilder { capacity, item } => {
                let capacity = self.expr(capacity, locals, depth)?;
                let ty = Type::List(Box::new(item.clone()));
                let value = self.assign(
                    locals,
                    ty.clone(),
                    NativeOperation::Call {
                        callee: native_operand("$morrow_list_with_capacity"),
                        args: vec![(Scalar::I64, native_operand(&capacity))],
                        variadic: None,
                    },
                );
                return Ok((ty, value));
            }
            Operation::ListAppend { list, value } => {
                let output = self.expr(list, locals, depth)?;
                let raw = self.expr(value, locals, depth)?;
                let raw = self.payload(locals, &value.ty, raw);
                self.output
                    .statement(Statement::Effect(NativeOperation::Call {
                        callee: native_operand("$morrow_list_push_mut"),
                        args: vec![
                            (Scalar::I64, native_operand(&output)),
                            (Scalar::I64, native_operand(&raw)),
                        ],
                        variadic: None,
                    }));
                return Ok((list.ty.clone(), output));
            }
            Operation::ClosureIdentity { value, function } => {
                let closure = self.expr(value, locals, depth)?;
                let code = self.raw_field(&closure, 0, locals);
                let identity = if self.actors.entries.contains_key(&function.0) {
                    format!("$actor_identity{}", function.0)
                } else {
                    format!("$f{}", function.0)
                };
                let result = self.assign(
                    locals,
                    Type::Bool,
                    NativeOperation::Binary(
                        MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                        native_operand(&code),
                        native_operand(&identity),
                    ),
                );
                return Ok((Type::Bool, result));
            }
            Operation::ClosureCapture { value, index, ty } => {
                let closure = self.expr(value, locals, depth)?;
                let raw = self.raw_field(&closure, 8 * (index + 1), locals);
                let result = self.unpack(locals, ty, raw);
                return Ok((ty.clone(), result));
            }
            Operation::ScopeEnter | Operation::ScopeLeave => self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand(if matches!(op, Operation::ScopeEnter) {
                        "$morrow_managed_scope_enter"
                    } else {
                        "$morrow_managed_scope_leave"
                    }),
                    args: vec![(Scalar::I64, native_operand("%exec"))],
                    variadic: None,
                },
            ),
            Operation::ScopeDefer(closure) => {
                let closure = self.expr(closure, locals, depth)?;
                self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$morrow_managed_scope_defer"),
                        args: vec![
                            (Scalar::I64, native_operand("%exec")),
                            (Scalar::I64, native_operand(&closure)),
                        ],
                        variadic: None,
                    },
                )
            }
            Operation::CleanupInvoke { function, closure } => {
                let target = self
                    .functions
                    .get(&function.0)
                    .ok_or_else(|| invalid(closure.span, "unknown actor cleanup function"))?;
                if !target.params.is_empty()
                    || target.return_type != Type::Unit
                    || target.mailbox.is_some()
                {
                    return Err(invalid(
                        closure.span,
                        "actor cleanup requires ordinary zero-argument Unit function",
                    ));
                }
                let closure = self.expr(closure, locals, depth)?;
                let mut args = vec![
                    (Scalar::I64, native_operand(&closure)),
                    (Scalar::I64, native_operand("%fault")),
                ];
                if self.actors.managed.contains(&function.0) {
                    args.push((Scalar::I64, native_operand("%exec")));
                }
                self.output
                    .statement(Statement::Effect(NativeOperation::Call {
                        callee: native_operand(&format!("$f{}", function.0)),
                        args,
                        variadic: None,
                    }));
                self.guard_fault(locals);
                "2".into()
            }
            Operation::IterateField { value, field } => {
                let range = value.ty == Type::Range;
                let value = self.expr(value, locals, depth)?;
                if range {
                    self.raw_field(&value, field * 8, locals)
                } else if *field == 1 {
                    self.assign(
                        locals,
                        Type::Int,
                        NativeOperation::Call {
                            callee: native_operand("$morrow_list_len"),
                            args: vec![(Scalar::I64, native_operand(&value))],
                            variadic: None,
                        },
                    )
                } else {
                    "0".into()
                }
            }
            Operation::IterateItem { value, index, item } => {
                let ty = value.ty.clone();
                let value = self.expr(value, locals, depth)?;
                let index = self.expr(index, locals, depth)?;
                let value = self.indexed_iteration_item(&index, &value, &ty, item, locals);
                return Ok((item.clone(), value));
            }
            Operation::Pointer(entry) => self.expr(entry, locals, depth)?,
            Operation::Continue(entry) => {
                let entry = self.expr(entry, locals, depth)?;
                self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$morrow_managed_continue"),
                        args: vec![
                            (Scalar::I64, native_operand("%exec")),
                            (Scalar::I64, native_operand(&(entry))),
                        ],
                        variadic: None,
                    },
                )
            }
            Operation::Register {
                selector,
                timeout,
                duration,
            } => {
                let duration = self.expr(duration, locals, depth)?;
                let selector = self.expr(selector, locals, depth)?;
                let timeout = timeout
                    .as_ref()
                    .map(|e| self.expr(e, locals, depth))
                    .transpose()?
                    .unwrap_or_else(|| "0".into());
                self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$morrow_managed_receive"),
                        args: vec![
                            (Scalar::I64, native_operand("%exec")),
                            (Scalar::I64, native_operand(&(selector))),
                            (Scalar::I64, native_operand(&(timeout))),
                            (Scalar::I64, native_operand(&(duration))),
                        ],
                        variadic: None,
                    },
                )
            }
            Operation::Select { value, arms } => {
                return self.matching_mode(value, arms, locals, depth, false, true);
            }
        };
        self.guard_fault(locals);
        Ok((Type::Int, value))
    }

    /// Preserve main's result/fault precedence before the scheduler can execute queued work.
    pub(super) fn actor_main(&mut self, main: &Function) {
        self.output
            .begin("$morrow_main", Some(Scalar::I32), vec![], true);
        self.output.statement(Statement::Label("@start".to_owned()));
        self.output.statement(Statement::Assign {
            destination: "%fault".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc { bytes: 8, align: 8 },
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand("%fault"),
        });
        self.output.statement(Statement::Assign {
            destination: "%exec".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Call {
                callee: native_operand("$morrow_managed_new"),
                args: vec![
                    (Scalar::I64, native_operand("%fault")),
                    (Scalar::I64, native_operand("$actor_functions")),
                    (
                        Scalar::I64,
                        native_operand(&(self.functions.len()).to_string()),
                    ),
                ],
                variadic: None,
            },
        });
        self.output.statement(Statement::Assign {
            destination: "%initialized".to_owned(),
            ty: Scalar::I32,
            operation: NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                native_operand("%exec"),
                native_operand("0"),
            ),
        });
        self.output.statement(Statement::Branch {
            condition: native_operand("%initialized"),
            then_label: "@entry".to_owned(),
            else_label: "@initial_failed".to_owned(),
        });
        self.output
            .statement(Statement::Label("@initial_failed".to_owned()));
        self.output.statement(Statement::Assign {
            destination: "%initial_code".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
        });
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_rs_report_fault"),
                args: vec![(Scalar::I64, native_operand("%initial_code"))],
                variadic: None,
            }));
        self.output
            .statement(Statement::Return(Some(native_operand("1"))));
        self.output.statement(Statement::Label("@entry".to_owned()));
        // Exec is a collected wrapper around external session ownership. Keep
        // it explicit while source main and the scheduler may collect heap 0.
        self.output.statement(Statement::Assign {
            destination: "%exec_root".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc { bytes: 8, align: 8 },
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("%exec"),
            address: native_operand("%exec_root"),
        });
        self.output.statement(Statement::Assign {
            destination: "%exec_frame".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Call {
                callee: native_operand("$morrow_gc_frame_enter"),
                args: vec![
                    (Scalar::I64, native_operand("%exec_root")),
                    (Scalar::I64, native_operand("1")),
                ],
                variadic: None,
            },
        });
        let mut arguments = vec![
            (Scalar::I64, Operand::Int(0)),
            (Scalar::I64, native_operand("%fault")),
        ];
        if self.actors.managed.contains(&main.id.0) {
            arguments.push((Scalar::I64, native_operand("%exec")));
        }
        self.output.statement(Statement::Assign {
            destination: "%exit".to_owned(),
            ty: machine_width(self.width(main.return_type.clone())),
            operation: NativeOperation::Call {
                callee: native_operand(&format!("$f{}", main.id.0)),
                args: arguments,
                variadic: None,
            },
        });
        self.output.statement(Statement::Assign {
            destination: "%before".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
        });
        self.output.statement(Statement::Assign {
            destination: "%bad".to_owned(),
            ty: Scalar::I32,
            operation: NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                native_operand("%before"),
                native_operand("0"),
            ),
        });
        self.output.statement(Statement::Branch {
            condition: native_operand("%bad"),
            then_label: "@stopped".to_owned(),
            else_label: "@main_ok".to_owned(),
        });
        self.output
            .statement(Statement::Label("@main_ok".to_owned()));
        if matches!(main.return_type, Type::Result(_, _)) {
            self.output.statement(Statement::Assign {
                destination: "%actor_ok".to_owned(),
                ty: Scalar::I64,
                operation: NativeOperation::Call {
                    callee: native_operand("$morrow_result_is_ok"),
                    args: vec![(Scalar::I64, native_operand("%exit"))],
                    variadic: None,
                },
            });
            self.output.statement(Statement::Assign {
                destination: "%actor_is_ok".to_owned(),
                ty: Scalar::I32,
                operation: NativeOperation::Binary(
                    MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                    native_operand("%actor_ok"),
                    native_operand("0"),
                ),
            });
            self.output.statement(Statement::Branch {
                condition: native_operand("%actor_is_ok"),
                then_label: "@drain".to_owned(),
                else_label: "@stopped".to_owned(),
            });
        } else {
            self.output.statement(Statement::Jump("@drain".to_owned()));
        }
        self.output.statement(Statement::Label("@drain".to_owned()));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_managed_run"),
                args: vec![(Scalar::I64, native_operand("%exec"))],
                variadic: None,
            }));
        self.output
            .statement(Statement::Jump("@stopped".to_owned()));
        self.output
            .statement(Statement::Label("@stopped".to_owned()));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_managed_stop"),
                args: vec![(Scalar::I64, native_operand("%exec"))],
                variadic: None,
            }));
        self.output.statement(Statement::Assign {
            destination: "%code".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
        });
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_gc_frame_leave"),
                args: vec![(Scalar::I64, native_operand("%exec_frame"))],
                variadic: None,
            }));
        self.output.statement(Statement::Assign {
            destination: "%failed".to_owned(),
            ty: Scalar::I32,
            operation: NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                native_operand("%code"),
                native_operand("0"),
            ),
        });
        self.output.statement(Statement::Branch {
            condition: native_operand("%failed"),
            then_label: "@failed".to_owned(),
            else_label: "@success".to_owned(),
        });
        self.output
            .statement(Statement::Label("@failed".to_owned()));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_rs_report_fault"),
                args: vec![(Scalar::I64, native_operand("%code"))],
                variadic: None,
            }));
        self.output
            .statement(Statement::Return(Some(native_operand("1"))));
        self.output
            .statement(Statement::Label("@success".to_owned()));
        if main.return_type == Type::Int {
            self.output.statement(Statement::Assign {
                destination: "%status".to_owned(),
                ty: Scalar::I32,
                operation: NativeOperation::Unary(MachineUnary::Copy, native_operand("%exit")),
            });
            self.output
                .statement(Statement::Return(Some(native_operand("%status"))));
            self.output.end();
        } else if matches!(main.return_type, Type::Result(_, _)) {
            self.result_main_exit();
        } else {
            self.output
                .statement(Statement::Return(Some(native_operand("0"))));
            self.output.end();
        }
    }
}

#[cfg(test)]
mod preparation_tests {
    use super::*;

    #[test]
    fn unchanged_program_is_borrowed_after_original_validation() {
        let ast = crate::parse::parse("fn main(): ()").unwrap();
        let program = crate::check::check(&ast).unwrap();
        let prepared = prepare(&program).unwrap();
        assert!(matches!(prepared.program, std::borrow::Cow::Borrowed(_)));
        assert!(!prepared.plan.active);
        assert!(std::ptr::eq(prepared.program.as_ref(), &program));
    }
    #[test]
    fn managed_program_without_continuations_borrows_with_its_effects() {
        let source = "fn worker(): ()\nfn main():\n    let pid:Pid(Int)=spawn(worker)\n    ()\n";
        let program = crate::check::check(&crate::parse::parse(source).unwrap()).unwrap();
        let prepared = prepare(&program).unwrap();
        assert!(matches!(prepared.program, std::borrow::Cow::Borrowed(_)));
        assert!(prepared.plan.active);
        assert!(!prepared.plan.managed.is_empty());
    }
}
