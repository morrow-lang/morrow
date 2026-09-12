//! Managed actor lowering keeps logical functions separate from resumable continuation entries.
use super::*;
use crate::actors::{Lowered, Operation};
#[path = "actors/control_types.rs"]
mod control_types;
#[path = "actors/descriptors.rs"]
mod descriptors;
#[path = "actors/lower.rs"]
mod lower;
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
    let mut plan = lower::program(program)?;
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
    /// Lower validated actor operations with the caller's separate execution context.
    pub(super) fn actor_expression(
        &mut self,
        actor: &ir::ActorExpr,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        match actor {
            ir::ActorExpr::Spawn { entry, mailbox } => {
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
                let ty = Type::Pid(Box::new(mailbox.clone()));
                let value = self.assign(
                    locals,
                    ty.clone(),
                    NativeOperation::Call {
                        callee: native_operand("$fern_managed_spawn"),
                        args: vec![
                            (Scalar::I64, native_operand("%exec")),
                            (Scalar::I64, native_operand(&(entry))),
                            (Scalar::I64, native_operand(&(descriptor))),
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
                        callee: native_operand("$fern_managed_send"),
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
            Operation::Pointer(entry) => self.expr(entry, locals, depth)?,
            Operation::Continue(entry) => {
                let entry = self.expr(entry, locals, depth)?;
                self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$fern_managed_continue"),
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
                        callee: native_operand("$fern_managed_receive"),
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
            .begin("$fern_main", Some(Scalar::I32), vec![], true);
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
                callee: native_operand("$fern_managed_new"),
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
                callee: native_operand("$fern_rs_report_fault"),
                args: vec![(Scalar::I64, native_operand("%initial_code"))],
                variadic: None,
            }));
        self.output
            .statement(Statement::Return(Some(native_operand("1"))));
        self.output.statement(Statement::Label("@entry".to_owned()));
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
                    callee: native_operand("$fern_result_is_ok"),
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
                callee: native_operand("$fern_managed_run"),
                args: vec![(Scalar::I64, native_operand("%exec"))],
                variadic: None,
            }));
        self.output
            .statement(Statement::Jump("@stopped".to_owned()));
        self.output
            .statement(Statement::Label("@stopped".to_owned()));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$fern_managed_stop"),
                args: vec![(Scalar::I64, native_operand("%exec"))],
                variadic: None,
            }));
        self.output.statement(Statement::Assign {
            destination: "%code".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
        });
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
                callee: native_operand("$fern_rs_report_fault"),
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
