//! Supervisor admission is typed here; root/actor execution is selected during specialization.
use super::*;
use crate::supervisors::{self as s, RequestKind, SupervisorExpr as S};
impl Checker<'_> {
    pub(super) fn supervisor_call(&mut self, name: &str, args: &[ast::Argument], expected: Option<&Type>, span: Span, depth: usize) -> Checked<TypedKind> {
        if self.deferred { return Err(Diagnostic::new(span, "supervisor operations are unsupported in deferred cleanup")); }
        labels::positional(args)?;
        if s::requires_actor(name) && self.mailbox.is_none() { return Err(Diagnostic::new(span, "supervisor startup operation requires an actor context")); }
        let arity = match name {
            "Process.init_ack" | "Process.init_ignore" => 0,
            "Supervisor.start" | "Supervisor.start_link" | "Supervisor.current" => 2,
            "Supervisor.worker" => 3,
            "Supervisor.branch" => 4,
            _ => 1,
        };
        if args.len() != arity { return Err(Diagnostic::new(span, format!("{name} expects {arity} arguments"))); }
        let (operation, ty) = match name {
            "Supervisor.child_key" => {
                let mailbox = self.inference.fresh();
                let ty = Type::ChildKey(Box::new(mailbox.clone()));
                self.constrain_result(&ty, expected, span)?;
                let name = Box::new(self.expression_equal(&args[0], &Type::String, depth)?);
                (S::ChildKey { name, mailbox }, ty)
            }
            "Supervisor.worker" => {
                let mailbox = self.inference.fresh();
                let key = Box::new(self.expression_equal(&args[0], &Type::ChildKey(Box::new(mailbox.clone())), depth)?);
                let entry = Box::new(self.spawn_entry(&args[1], &mailbox, span, depth)?);
                let policy = Box::new(self.expression_equal(&args[2], &s::named("ChildPolicy"), depth)?);
                (S::Worker { key, entry, policy, mailbox }, s::result(s::spec()))
            }
            "Supervisor.branch" => {
                let name = Box::new(self.expression_equal(&args[0], &Type::String, depth)?);
                let flags = Box::new(self.expression_equal(&args[1], &s::named("Flags"), depth)?);
                let children = Box::new(self.expression_equal(&args[2], &Type::List(Box::new(s::spec())), depth)?);
                let policy = Box::new(self.expression_equal(&args[3], &s::named("ChildPolicy"), depth)?);
                (S::Branch { name, flags, children, policy }, s::result(s::spec()))
            }
            "Supervisor.id" => {
                let handle = Box::new(self.expression_equal(&args[0], &s::handle(), depth)?);
                (S::Id { handle }, crate::processes::identity())
            }
            "Supervisor.start" | "Supervisor.start_link" | "Supervisor.current" | "Supervisor.stop" => {
                let (kind, arguments, ty) = match name {
                    "Supervisor.start" | "Supervisor.start_link" => {
                        let flags = self.expression_equal(&args[0], &s::named("Flags"), depth)?;
                        let children = self.expression_equal(&args[1], &Type::List(Box::new(s::spec())), depth)?;
                        (if name == "Supervisor.start" { RequestKind::Start } else { RequestKind::StartLink }, vec![flags, children], s::result(s::handle()))
                    }
                    "Supervisor.current" => {
                        let mailbox = self.inference.fresh();
                        let result = s::result(Type::Pid(Box::new(mailbox.clone())));
                        self.constrain_result(&result, expected, span)?;
                        let handle = self.expression_equal(&args[0], &s::handle(), depth)?;
                        let key = self.expression_equal(&args[1], &Type::ChildKey(Box::new(mailbox)), depth)?;
                        (RequestKind::Current, vec![handle, key], result)
                    }
                    _ => (RequestKind::Stop, vec![self.expression_equal(&args[0], &s::handle(), depth)?], s::result(Type::Unit)),
                };
                (S::Request { kind, args: arguments, mailbox: self.mailbox.clone() }, ty)
            }
            "Process.init_ack" => (S::InitAck, crate::processes::result(Type::Unit)),
            "Process.init_ignore" => (S::InitIgnore, Type::Never),
            "Process.init_fail" => (S::InitFail { reason: Box::new(self.expression_equal(&args[0], &crate::processes::reason(), depth)?) }, Type::Never),
            _ => return Err(Diagnostic::new(span, "unknown supervisor operation")),
        };
        let (kind, ty) = control::strict_divergence(ir::ExprKind::Actor(ir::ActorExpr::Supervisor(operation)), ty);
        self.constrain_result(&ty, expected, span)?;
        Ok((kind, ty))
    }
    pub(super) fn finalize_supervisor(&self, operation: &mut S, span: Span) -> Checked<()> {
        let mailbox = match operation {
            S::ChildKey { mailbox, .. } | S::Worker { mailbox, .. } => Some(mailbox),
            S::Request { mailbox, .. } => mailbox.as_mut(),
            _ => None,
        };
        if let Some(mailbox) = mailbox {
            *mailbox = self.inference.concrete(mailbox, span)?;
            self.actor_sendable(mailbox, span)?;
            if self.registry.contains_result(mailbox)? { return Err(Diagnostic::new(span, "Result-bearing supervisor mailboxes are unsupported")); }
        }
        Ok(())
    }
}
