//! Explicit actor context and typed Result admission for isolated process operations.
use super::*;
use crate::processes::{self as p, ProcessExpr, SpawnMode};

impl Checker<'_> {
    pub(super) fn process_call(
        &mut self,
        name: &str,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        if self.deferred {
            return Err(Diagnostic::new(
                span,
                "process operations are unsupported in deferred cleanup",
            ));
        }
        labels::positional(args)?;
        if p::requires_actor(name) && self.mailbox.is_none() {
            return Err(Diagnostic::new(
                span,
                "process operation requires an actor context",
            ));
        }
        let arity = match name {
            "Process.self" => 0,
            "Process.demonitor" | "Process.signal_exit" => 2,
            _ => 1,
        };
        if args.len() != arity {
            return Err(Diagnostic::new(
                span,
                format!("{name} expects {arity} arguments"),
            ));
        }
        let (operation, ty) = match name {
            "Process.spawn" | "Process.spawn_monitor" | "Process.spawn_link" => {
                let mailbox = self.inference.fresh();
                let pid = Type::Pid(Box::new(mailbox.clone()));
                let mode = match name {
                    "Process.spawn_monitor" => SpawnMode::Monitor,
                    "Process.spawn_link" => SpawnMode::Link,
                    _ => SpawnMode::Isolated,
                };
                let result = p::result(if mode == SpawnMode::Monitor {
                    Type::Tuple(vec![pid, p::monitor_ref()])
                } else {
                    pid
                });
                if expected.is_some_and(|ty| matches!(ty, Type::Result(..))) {
                    self.constrain_result(&result, expected, span)?;
                }
                let entry = self.spawn_entry(&args[0], &mailbox, span, depth)?;
                (
                    ProcessExpr::Spawn {
                        entry: Box::new(entry),
                        mailbox,
                        mode,
                    },
                    result,
                )
            }
            "Process.self" => {
                let mailbox = self
                    .mailbox
                    .clone()
                    .ok_or_else(|| Diagnostic::new(span, "self requires an actor context"))?;
                (
                    ProcessExpr::SelfPid {
                        mailbox: mailbox.clone(),
                    },
                    Type::Pid(Box::new(mailbox)),
                )
            }
            "Process.id" => {
                let pid = self.expression(&args[0], depth)?;
                if !matches!(
                    self.inference.resolve(&pid.ty, span)?,
                    Type::Pid(_) | Type::Never
                ) {
                    return Err(Diagnostic::new(span, "Process.id requires a typed Pid"));
                }
                (ProcessExpr::Id { pid: Box::new(pid) }, p::identity())
            }
            "Process.monitor" => {
                let target = self.expression_equal(&args[0], &p::identity(), depth)?;
                (
                    ProcessExpr::Monitor {
                        target: Box::new(target),
                    },
                    p::result(p::monitor_ref()),
                )
            }
            "Process.link" | "Process.unlink" => {
                let target = Box::new(self.expression_equal(&args[0], &p::identity(), depth)?);
                let operation = if name == "Process.link" {
                    ProcessExpr::Link { target }
                } else {
                    ProcessExpr::Unlink { target }
                };
                (operation, p::result(Type::Unit))
            }
            "Process.trap_exit" => {
                let enabled = Box::new(self.expression_equal(&args[0], &Type::Bool, depth)?);
                (ProcessExpr::TrapExit { enabled }, Type::Bool)
            }
            "Process.exit" => {
                let reason = Box::new(self.expression_equal(&args[0], &p::reason(), depth)?);
                (ProcessExpr::Exit { reason }, Type::Never)
            }
            "Process.signal_exit" => {
                let target = Box::new(self.expression_equal(&args[0], &p::identity(), depth)?);
                let reason = Box::new(self.expression_equal(&args[1], &p::reason(), depth)?);
                (
                    ProcessExpr::SignalExit { target, reason },
                    p::result(Type::Unit),
                )
            }
            "Process.demonitor" => {
                let reference = self.expression_equal(&args[0], &p::monitor_ref(), depth)?;
                let options = self.expression_equal(&args[1], &p::options(), depth)?;
                (
                    ProcessExpr::Demonitor {
                        reference: Box::new(reference),
                        options: Box::new(options),
                    },
                    p::result(Type::Bool),
                )
            }
            _ => return Err(Diagnostic::new(span, "unknown process operation")),
        };
        let (kind, ty) =
            control::strict_divergence(ir::ExprKind::Actor(ir::ActorExpr::Process(operation)), ty);
        self.constrain_result(&ty, expected, span)?;
        Ok((kind, ty))
    }

    pub(super) fn finalize_process(&self, operation: &mut ProcessExpr, span: Span) -> Checked<()> {
        match operation {
            ProcessExpr::Spawn { mailbox, .. } | ProcessExpr::SelfPid { mailbox } => {
                *mailbox = self.inference.concrete(mailbox, span)?;
                self.actor_sendable(mailbox, span)?;
                if self.registry.contains_result(mailbox)? {
                    return Err(Diagnostic::new(
                        span,
                        "Result-bearing process messages are unsupported",
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}
