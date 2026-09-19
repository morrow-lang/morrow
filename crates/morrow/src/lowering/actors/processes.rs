//! Explicit-context process calls use additive symbols and the existing rooted value ABI.
use super::*;
use crate::processes::{self as p, ProcessExpr};

pub(super) fn validate(
    operation: &ProcessExpr,
    expr: &Expr,
    owner: Option<&Type>,
    layouts: &HashMap<Type, &ir::TypeLayout>,
) -> Lowering<()> {
    let expected = match operation {
        ProcessExpr::Spawn {
            entry,
            mailbox,
            monitor,
        } => {
            if *monitor && owner.is_none() {
                return Err(invalid(
                    expr.span,
                    "spawn_monitor requires an actor context",
                ));
            }
            validate::sendable(mailbox, layouts, expr.span)?;
            let (effect, args, result) = crate::actors::function(&entry.ty)
                .ok_or_else(|| invalid(expr.span, "process entry must be callable"))?;
            if !args.is_empty() || *result != Type::Unit {
                return Err(invalid(
                    expr.span,
                    "process entry must take no arguments and return Unit",
                ));
            }
            if let Some(effect) = effect {
                expect_type(effect.clone(), mailbox.clone(), expr.span)?;
            }
            let pid = Type::Pid(Box::new(mailbox.clone()));
            p::result(if *monitor {
                Type::Tuple(vec![pid, p::monitor_ref()])
            } else {
                pid
            })
        }
        ProcessExpr::SelfPid { mailbox } => {
            let owner =
                owner.ok_or_else(|| invalid(expr.span, "self requires an actor context"))?;
            expect_type(mailbox.clone(), owner.clone(), expr.span)?;
            Type::Pid(Box::new(mailbox.clone()))
        }
        ProcessExpr::Id { pid } => {
            if !matches!(pid.ty, Type::Pid(_)) {
                return Err(invalid(expr.span, "Process.id requires a typed Pid"));
            }
            p::identity()
        }
        ProcessExpr::Monitor { target } => {
            if owner.is_none() {
                return Err(invalid(expr.span, "monitor requires an actor context"));
            }
            expect_type(target.ty.clone(), p::identity(), expr.span)?;
            p::result(p::monitor_ref())
        }
        ProcessExpr::Demonitor { reference, options } => {
            if owner.is_none() {
                return Err(invalid(expr.span, "demonitor requires an actor context"));
            }
            expect_type(reference.ty.clone(), p::monitor_ref(), expr.span)?;
            expect_type(options.ty.clone(), p::options(), expr.span)?;
            p::result(Type::Bool)
        }
    };
    expect_type(expr.ty.clone(), expected, expr.span)
}

impl Emitter<'_> {
    pub(super) fn process_expression(
        &mut self,
        operation: &ProcessExpr,
        _span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let mut arguments = vec![(Scalar::I64, native_operand("%exec"))];
        let (symbol, ty) = match operation {
            ProcessExpr::Spawn {
                entry,
                mailbox,
                monitor,
            } => {
                let entry = self.expr(entry, locals, depth)?;
                let descriptor = self.actor_type(mailbox)?;
                arguments.extend([
                    (Scalar::I64, native_operand(&entry)),
                    (Scalar::I64, native_operand(&descriptor)),
                ]);
                let pid = Type::Pid(Box::new(mailbox.clone()));
                if *monitor {
                    (
                        "$morrow_process_spawn_monitor",
                        p::result(Type::Tuple(vec![pid, p::monitor_ref()])),
                    )
                } else {
                    ("$morrow_process_spawn", p::result(pid))
                }
            }
            ProcessExpr::SelfPid { mailbox } => {
                arguments.push((Scalar::I64, native_operand(&self.actor_type(mailbox)?)));
                ("$morrow_process_self", Type::Pid(Box::new(mailbox.clone())))
            }
            ProcessExpr::Id { pid } => {
                arguments.push((Scalar::I64, native_operand(&self.expr(pid, locals, depth)?)));
                ("$morrow_process_id", p::identity())
            }
            ProcessExpr::Monitor { target } => {
                arguments.push((
                    Scalar::I64,
                    native_operand(&self.expr(target, locals, depth)?),
                ));
                ("$morrow_process_monitor", p::result(p::monitor_ref()))
            }
            ProcessExpr::Demonitor { reference, options } => {
                let reference = self.expr(reference, locals, depth)?;
                let options = self.expr(options, locals, depth)?;
                let flush = self.custom_field(&options, 0, &Type::Bool, locals);
                let flush = self.payload(locals, &Type::Bool, flush);
                let info = self.custom_field(&options, 1, &Type::Bool, locals);
                let info = self.payload(locals, &Type::Bool, info);
                let info = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Binary(
                        MachineBinary::Mul,
                        native_operand(&info),
                        native_operand("2"),
                    ),
                );
                let flags = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Binary(
                        MachineBinary::Or,
                        native_operand(&flush),
                        native_operand(&info),
                    ),
                );
                arguments.extend([
                    (Scalar::I64, native_operand(&reference)),
                    (Scalar::I64, native_operand(&flags)),
                ]);
                ("$morrow_process_demonitor", p::result(Type::Bool))
            }
        };
        let value = self.assign(
            locals,
            ty.clone(),
            NativeOperation::Call {
                callee: native_operand(symbol),
                args: arguments,
                variadic: None,
            },
        );
        self.guard_fault(locals);
        Ok((ty, value))
    }
}
