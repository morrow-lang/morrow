//! Immutable request registrations and rooted supervisor call boundaries.
use super::*;
use crate::supervisors::{self as s, RequestKind, SupervisorExpr as S};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Registration {
    pub kind: RequestKind,
    pub request: Type,
    pub result: Type,
    pub resume: Option<usize>,
}

pub(super) fn validate(op: &S, expr: &Expr, owner: Option<&Type>, layouts: &HashMap<Type, &ir::TypeLayout>) -> Lowering<()> {
    let ty = match op {
        S::ChildKey { name, mailbox } => {
            expect_type(name.ty.clone(), Type::String, expr.span)?;
            validate::sendable(mailbox, layouts, expr.span)?;
            Type::ChildKey(Box::new(mailbox.clone()))
        }
        S::Worker { key, entry, policy, mailbox } => {
            expect_type(key.ty.clone(), Type::ChildKey(Box::new(mailbox.clone())), expr.span)?;
            expect_type(policy.ty.clone(), s::named("ChildPolicy"), expr.span)?;
            validate::sendable(mailbox, layouts, expr.span)?;
            let (effect, params, result) = crate::actors::function(&entry.ty).ok_or_else(|| invalid(expr.span, "worker requires a callable"))?;
            if matches!(entry.ty, Type::RootFunction(_)) || !params.is_empty() || *result != Type::Unit { return Err(invalid(expr.span, "worker requires a fresh actor entry")); }
            if let Some(effect) = effect { expect_type(effect.clone(), mailbox.clone(), expr.span)?; }
            s::result(s::spec())
        }
        S::Branch { name, flags, children, policy } => {
            for (value, ty) in [(name, Type::String), (flags, s::named("Flags")), (children, Type::List(Box::new(s::spec()))), (policy, s::named("ChildPolicy"))] { expect_type(value.ty.clone(), ty, expr.span)?; }
            s::result(s::spec())
        }
        S::Id { handle } => { expect_type(handle.ty.clone(), s::handle(), expr.span)?; crate::processes::identity() }
        S::Request { kind, args, mailbox } => {
            if mailbox.as_ref() != owner { return Err(invalid(expr.span, "supervisor request context differs from owner")); }
            if *kind == RequestKind::StartLink && owner.is_none() { return Err(invalid(expr.span, "start_link requires an actor context")); }
            let fields = s::request_fields(&s::request_type(*kind, args)).ok_or_else(|| invalid(expr.span, "invalid supervisor request schema"))?;
            if fields.len() != args.len() { return Err(invalid(expr.span, "supervisor request arity mismatch")); }
            for (arg, ty) in args.iter().zip(fields) { expect_type(arg.ty.clone(), ty, expr.span)?; }
            s::result(match kind {
                RequestKind::Start | RequestKind::StartLink => s::handle(),
                RequestKind::Stop => Type::Unit,
                RequestKind::Current => { let Type::ChildKey(mailbox) = &args[1].ty else { return Err(invalid(expr.span, "current requires a typed child key")); }; Type::Pid(mailbox.clone()) },
            })
        }
        S::InitAck | S::InitIgnore | S::InitFail { .. } => {
            if owner.is_none() { return Err(invalid(expr.span, "startup operation requires an actor context")); }
            if let S::InitFail { reason } = op { expect_type(reason.ty.clone(), crate::processes::reason(), expr.span)?; }
            if op.terminal() { Type::Never } else { crate::processes::result(Type::Unit) }
        }
    };
    expect_type(expr.ty.clone(), ty, expr.span)
}

impl Emitter<'_> {
    pub(super) fn supervisor_expression(&mut self, operation: &S, span: Span, locals: &mut Locals, depth: usize) -> Lowering<(Type, String)> {
        let mut args = vec![(Scalar::I64, native_operand("%exec"))];
        let (symbol, ty) = match operation {
            S::ChildKey { name, mailbox } => {
                args.push((Scalar::I64, native_operand(&self.expr(name, locals, depth)?)));
                args.push((Scalar::I64, native_operand(&self.actor_type(mailbox)?)));
                ("$morrow_managed_supervisor_child_key", Type::ChildKey(Box::new(mailbox.clone())))
            }
            S::Worker { key, entry, policy, .. } => {
                for value in [key, entry, policy] { args.push((Scalar::I64, native_operand(&self.expr(value, locals, depth)?))); }
                ("$morrow_managed_supervisor_worker", s::result(s::spec()))
            }
            S::Branch { name, flags, children, policy } => {
                for value in [name, flags, children, policy] { args.push((Scalar::I64, native_operand(&self.expr(value, locals, depth)?))); }
                ("$morrow_managed_supervisor_branch", s::result(s::spec()))
            }
            S::Id { handle } => {
                args.push((Scalar::I64, native_operand(&self.expr(handle, locals, depth)?)));
                ("$morrow_managed_supervisor_id", crate::processes::identity())
            }
            S::InitAck => ("$morrow_process_init_ack", crate::processes::result(Type::Unit)),
            S::Request { kind, args: operands, mailbox: None } => {
                let request = s::request_type(*kind, operands);
                let index = self.actors.requests.iter().position(|r| r.kind == *kind && r.request == request && r.resume.is_none()).ok_or_else(|| invalid(span, "missing root supervisor registration"))?;
                let ty = self.actors.requests[index].result.clone();
                let request = self.supervisor_payload(operands, span, locals, depth)?;
                args.extend([(Scalar::I64, native_operand(&format!("$supervisor_request{index}"))), (Scalar::I64, native_operand(&request))]);
                ("$morrow_supervisor_root_request", ty)
            }
            _ => return Err(invalid(span, "suspending or terminal supervisor operation was not continuation-lowered")),
        };
        let value = self.assign(locals, ty.clone(), NativeOperation::Call { callee: native_operand(symbol), args, variadic: None });
        self.guard_fault(locals);
        Ok((ty, value))
    }
    fn supervisor_payload(&mut self, args: &[Expr], span: Span, locals: &mut Locals, depth: usize) -> Lowering<String> {
        // Kind3 and the private kind4 record share initialized [tag0, fields...] storage.
        // Registration publishes kind4 only; all operands retain their ordinary precise roots.
        let ty = Type::Tuple(args.iter().map(|a| a.ty.clone()).collect());
        self.custom_construct(0, args, &ty, span, locals, depth).map(|(_, value)| value)
    }
    pub(super) fn supervisor_suspend(&mut self, registration: usize, args: &[Expr], resume: &Expr, locals: &mut Locals, depth: usize) -> Lowering<String> {
        let payload = self.supervisor_payload(args, resume.span, locals, depth)?;
        let frame = self.expr(resume, locals, depth)?;
        Ok(self.assign(locals, Type::Int, NativeOperation::Call {
            callee: native_operand("$morrow_supervisor_request"),
            args: vec![(Scalar::I64, native_operand("%exec")), (Scalar::I64, native_operand(&format!("$supervisor_request{registration}"))), (Scalar::I64, native_operand(&payload)), (Scalar::I64, native_operand(&frame))], variadic: None,
        }))
    }
    pub(super) fn supervisor_reply(&mut self, registration: usize, ty: &Type, locals: &mut Locals) -> Lowering<(Type, String)> {
        let slot = self.reply_root(locals, Span::default())?;
        let status = self.assign(locals, Type::Int, NativeOperation::Call {
            callee: native_operand("$morrow_supervisor_take_reply"),
            args: vec![(Scalar::I64, native_operand("%exec")), (Scalar::I64, native_operand(&format!("$supervisor_request{registration}"))), (Scalar::I64, native_operand(&slot))], variadic: None,
        });
        self.guard_fault(locals);
        // A nonzero status without the required fault is an invalid native implementation.
        let good = self.assign(locals, Type::Bool, NativeOperation::Binary(MachineBinary::Compare(Comparison::Eq, Scalar::I64), native_operand(&status), Operand::Int(0)));
        let ready = locals.label(); let bad = locals.label();
        self.output.statement(Statement::Branch { condition: native_operand(&good), then_label: ready.clone(), else_label: bad.clone() });
        self.start_block(locals, &bad);
        self.output.statement(Statement::Store { kind: LoadKind::I64, value: Operand::Int(11), address: native_operand("%fault") });
        self.guard_fault(locals);
        self.output.statement(Statement::Jump(ready.clone()));
        self.start_block(locals, &ready);
        let value = self.assign(locals, ty.clone(), NativeOperation::Load(LoadKind::I64, native_operand(&slot)));
        Ok((ty.clone(), value))
    }
    pub(super) fn supervisor_descriptors(&mut self) -> Lowering<()> {
        let records = self.actors.requests.clone();
        let mut table = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let request = self.actor_type(&record.request)?;
            let result = self.actor_type(&record.result)?;
            let resume = record.resume.map(|id| format!("$actor_descriptor{id}")).unwrap_or_else(|| "0".into());
            let name = format!("$supervisor_request{index}");
            self.data.data(&name, vec![DataValue::Word(Operand::Int(record.kind.opcode())), DataValue::Word(native_operand(&request)), DataValue::Word(native_operand(&result)), DataValue::Word(native_operand(&resume))]);
            table.push(name);
        }
        if !table.is_empty() {
            self.data.data("$supervisor_requests", table.iter().map(|name| DataValue::Word(native_operand(name))).collect());
        }
        Ok(())
    }
}

impl Emitter<'_> {
    fn registration_call(&mut self, destination: &str, exec: &str) {
        self.output.statement(Statement::Assign { destination: destination.into(), ty: Scalar::I64,
            operation: NativeOperation::Call { callee: native_operand("$morrow_supervisor_register"), args: vec![(Scalar::I64, native_operand(exec)), (Scalar::I64, native_operand("$supervisor_requests")), (Scalar::I64, Operand::Int(self.actors.requests.len() as i64))], variadic: None } });
    }
    pub(super) fn supervisor_library(&mut self) {
        self.output.begin("$morrow_library_register_requests", Some(Scalar::I64), vec![(Scalar::I64, "%exec".into())], true);
        self.output.statement(Statement::Label("@start".into()));
        self.registration_call("%status", "%exec");
        self.output.statement(Statement::Return(Some(native_operand("%status"))));
        self.output.end();
        self.output.begin("$morrow_library_open", Some(Scalar::I64), vec![(Scalar::I64, "%fault".into())], true);
        self.output.statement(Statement::Label("@start".into()));
        self.output.statement(Statement::Assign { destination: "%exec".into(), ty: Scalar::I64, operation: NativeOperation::Call {
            callee: native_operand("$morrow_managed_open"), args: vec![(Scalar::I64, native_operand("%fault")), (Scalar::I64, native_operand("$actor_functions")), (Scalar::I64, Operand::Int(self.functions.len() as i64))], variadic: None,
        }});
        self.output.statement(Statement::Assign { destination: "%opened".into(), ty: Scalar::I32, operation: NativeOperation::Binary(MachineBinary::Compare(Comparison::Ne, Scalar::I64), native_operand("%exec"), Operand::Int(0)) });
        self.output.statement(Statement::Branch { condition: native_operand("%opened"), then_label: "@register".into(), else_label: "@failed".into() });
        self.output.statement(Statement::Label("@register".into()));
        self.registration_call("%status", "%exec");
        self.output.statement(Statement::Assign { destination: "%registered".into(), ty: Scalar::I32, operation: NativeOperation::Binary(MachineBinary::Compare(Comparison::Eq, Scalar::I64), native_operand("%status"), Operand::Int(0)) });
        self.output.statement(Statement::Branch { condition: native_operand("%registered"), then_label: "@ready".into(), else_label: "@close".into() });
        self.output.statement(Statement::Label("@close".into()));
        self.output.statement(Statement::Effect(NativeOperation::Call { callee: native_operand("$morrow_managed_close"), args: vec![(Scalar::I64, native_operand("%exec"))], variadic: None }));
        self.output.statement(Statement::Jump("@failed".into()));
        self.output.statement(Statement::Label("@failed".into()));
        self.output.statement(Statement::Return(Some(Operand::Int(0))));
        self.output.statement(Statement::Label("@ready".into()));
        self.output.statement(Statement::Return(Some(native_operand("%exec"))));
        self.output.end();
    }
    pub(super) fn supervisor_main_registration(&mut self) {
        if self.actors.requests.is_empty() { return; }
        self.registration_call("%registered_status", "%exec");
        self.output.statement(Statement::Assign { destination: "%registered_ok".into(), ty: Scalar::I32, operation: NativeOperation::Binary(MachineBinary::Compare(Comparison::Eq, Scalar::I64), native_operand("%registered_status"), Operand::Int(0)) });
        self.output.statement(Statement::Branch { condition: native_operand("%registered_ok"), then_label: "@registered".into(), else_label: "@registration_failed".into() });
        self.output.statement(Statement::Label("@registration_failed".into()));
        for (symbol, arg) in [("$morrow_managed_stop", "%exec"), ("$morrow_gc_frame_leave", "%exec_frame")] {
            self.output.statement(Statement::Effect(NativeOperation::Call { callee: native_operand(symbol), args: vec![(Scalar::I64, native_operand(arg))], variadic: None }));
        }
        self.output.statement(Statement::Jump("@initial_failed".into()));
        self.output.statement(Statement::Label("@registered".into()));
    }
}
