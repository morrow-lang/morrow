//! Actor metadata is validated even in unused functions and unreachable expression tails.
use super::*;

/// Bound original identity/count metadata before type walks, cloning, or generated-local arithmetic.
pub(super) fn shape(program: &ir::Program) -> Lowering<()> {
    let mut work = program.functions.len();
    if work > MAX_NODES {
        return Err(invalid(Span::default(), "function count limit exceeded"));
    }
    for function in &program.functions {
        if function.local_count > MAX_NODES || function.id.0 >= MAX_NODES {
            return Err(invalid(
                function.body.span,
                "function local or identity limit exceeded",
            ));
        }
        work = work
            .saturating_add(function.params.len())
            .saturating_add(function.captures.len());
        if work > MAX_NODES {
            return Err(invalid(
                function.body.span,
                "function signature limit exceeded",
            ));
        }
        let mut defined = BTreeSet::new();
        for param in function.params.iter().chain(&function.captures) {
            if param.id.0 >= function.local_count || !defined.insert(param.id.0) {
                return Err(invalid(
                    function.body.span,
                    "invalid parameter or capture local identity",
                ));
            }
        }
    }
    Ok(())
}

/// Reject forged contexts, identities and transfer signatures before private continuation cloning.
pub(super) fn program(
    program: &ir::Program,
    layouts: &HashMap<Type, &ir::TypeLayout>,
) -> Lowering<()> {
    let mut functions = BTreeMap::new();
    for function in &program.functions {
        if function.id.0 > MAX_NODES || functions.insert(function.id.0, function).is_some() {
            return Err(invalid(
                function.body.span,
                "invalid actor function identity",
            ));
        }
        if function.root_context && function.mailbox.is_some() { return Err(invalid(function.body.span, "root function cannot own an actor context")); }
        if let Some(mailbox) = &function.mailbox {
            if function.name == "main" {
                return Err(invalid(
                    function.body.span,
                    "main cannot own an actor context",
                ));
            }
            frame_owned(&function.return_type, layouts, function.body.span)?;
            sendable(mailbox, layouts, function.body.span)?;
            if function.body.ty != Type::Never {
                expect_type(
                    function.body.ty.clone(),
                    function.return_type.clone(),
                    function.body.span,
                )?;
            }
        }
    }
    for function in &program.functions {
        let mut pending = vec![&function.body];
        while let Some(expr) = pending.pop() {
            if function.mailbox.is_some() {
                control_types::expression(expr, function)?;
            }
            if let ExprKind::Closure {
                function: id,
                captures,
            } = &expr.kind
            {
                closure(expr, *id, captures, &functions)?;
            }
            if let ExprKind::Invoke { callee, .. } = &expr.kind {
                match &callee.ty {
                    Type::RootFunction(_) if function.mailbox.is_some() => return Err(invalid(expr.span, "root callable cannot run in an actor context")),
                    Type::ActorFunction(mailbox, _) if function.mailbox.as_ref() != Some(mailbox.as_ref()) => return Err(invalid(expr.span, "actor callable mailbox differs from owner")),
                    _ => {},
                }
            }
            if let ExprKind::Actor(actor) = &expr.kind {
                actor_expr(actor, expr, function.mailbox.as_ref(), &functions, layouts)?;
            }
            if let ExprKind::Call {
                target: CallTarget::Function(id),
                ..
            } = expr.kind
                && functions.get(&id.0).is_some_and(|f| f.mailbox.is_some() || f.root_context && function.mailbox.is_some())
            {
                return Err(invalid(
                    expr.span,
                    "receiving call requires an explicit actor context transition",
                ));
            }
            pending.extend(ir::children(expr));
        }
    }
    Ok(())
}

/// Check each actor operation's semantic signature independently of source inference.
fn actor_expr(
    actor: &ir::ActorExpr,
    expr: &Expr,
    owner: Option<&Type>,
    functions: &BTreeMap<usize, &Function>,
    layouts: &HashMap<Type, &ir::TypeLayout>,
) -> Lowering<()> {
    match actor {
        ir::ActorExpr::Process(operation) => processes::validate(operation, expr, owner, layouts)?,
        ir::ActorExpr::Supervisor(operation) => supervisors::validate(operation, expr, owner, layouts)?,
        ir::ActorExpr::Lowered(_) => {
            return Err(invalid(
                expr.span,
                "private actor continuation in public IR",
            ));
        }
        ir::ActorExpr::Spawn {
            entry,
            mailbox,
            max_restarts,
        } => {
            spawn(entry, mailbox, expr, layouts)?;
            if let Some(budget) = max_restarts {
                expect_type(budget.ty.clone(), Type::Int, expr.span)?;
            }
        }
        ir::ActorExpr::SupervisedCurrent { pid } => {
            if !matches!(pid.ty, Type::Pid(_)) {
                return Err(invalid(
                    expr.span,
                    "supervised_current requires a typed Pid",
                ));
            }
            expect_type(
                expr.ty.clone(),
                Type::Result(Box::new(pid.ty.clone()), Box::new(Type::Int)),
                expr.span,
            )?;
        }
        ir::ActorExpr::Send { pid, message } => {
            expect_type(
                pid.ty.clone(),
                Type::Pid(Box::new(message.ty.clone())),
                expr.span,
            )?;
            expect_type(
                expr.ty.clone(),
                Type::Result(Box::new(Type::Unit), Box::new(Type::Int)),
                expr.span,
            )?;
            sendable(&message.ty, layouts, expr.span)?;
        }
        ir::ActorExpr::Receive {
            mailbox,
            arms,
            timeout,
            ..
        } => receive(mailbox, arms, timeout, expr, owner, layouts)?,
        ir::ActorExpr::Call {
            function,
            args,
            mailbox,
        } => {
            let owner = owner
                .ok_or_else(|| invalid(expr.span, "receiving call requires an actor context"))?;
            expect_type(owner.clone(), mailbox.clone(), expr.span)?;
            let target = functions
                .get(&function.0)
                .ok_or_else(|| invalid(expr.span, "unknown receiving call identity"))?;
            if target.mailbox.as_ref() != Some(mailbox)
                || !target.captures.is_empty()
                || target.params.len() != args.len()
            {
                return Err(invalid(expr.span, "receiving call signature mismatch"));
            }
            expect_type(expr.ty.clone(), target.return_type.clone(), expr.span)?;
            for (arg, param) in args.iter().zip(&target.params) {
                expect_type(arg.ty.clone(), param.ty.clone(), arg.span)?;
            }
        }
    }
    Ok(())
}

/// Follow concrete immutable layouts once, refusing unproved Result/function/native transfers.
pub(super) fn sendable(
    ty: &Type,
    layouts: &HashMap<Type, &ir::TypeLayout>,
    span: Span,
) -> Lowering<()> {
    ownership(ty, layouts, span, false)
}

/// Internal continuation frames may retain Result values and callable graphs;
/// runtime descriptors validate each actual callable capture before publication.
pub(super) fn frame_owned(
    ty: &Type,
    layouts: &HashMap<Type, &ir::TypeLayout>,
    span: Span,
) -> Lowering<()> {
    ownership(ty, layouts, span, true)
}

fn ownership(
    ty: &Type,
    layouts: &HashMap<Type, &ir::TypeLayout>,
    span: Span,
    frame: bool,
) -> Lowering<()> {
    let mut pending = vec![ty];
    let mut seen = BTreeSet::new();
    let mut work = 0;
    while let Some(ty) = pending.pop() {
        work += 1;
        if work > 400_000 || pending.len() > 4096 {
            return Err(invalid(span, "actor sendability work limit exceeded"));
        }
        if !seen.insert(ty) {
            continue;
        }
        match ty {
            Type::Int
            | Type::Bool
            | Type::Unit
            | Type::Float
            | Type::String
            | Type::Range
            | Type::Native(
                crate::runtime::NativeType::JsonValue
                | crate::runtime::NativeType::ProcessId
                | crate::runtime::NativeType::MonitorRef
                | crate::runtime::NativeType::SupervisorHandle
                | crate::runtime::NativeType::ChildSpec,
            )
            | Type::Pid(_) | Type::ChildKey(_) => {}
            Type::List(item) | Type::Option(item) => pending.push(item),
            Type::Tuple(fields) | Type::Union(fields) => pending.extend(fields),
            Type::Map(key, value) => pending.extend([key.as_ref(), value.as_ref()]),
            Type::Named(name, _) if name == "Ptr" => {
                return Err(invalid(
                    span,
                    "foreign pointers cannot cross actor boundaries",
                ));
            }
            Type::Named(_, _) => {
                let layout = layouts
                    .get(ty)
                    .ok_or_else(|| invalid(span, "unknown actor message layout"))?;
                pending.extend(layout.variants.iter().flatten());
            }
            Type::Result(ok, err) if frame => pending.extend([ok.as_ref(), err.as_ref()]),
            Type::Function(_, _) | Type::ActorFunction(_, _) if frame => {}
            Type::Result(_, _) => {
                return Err(invalid(
                    span,
                    "Result-bearing actor messages are unsupported; send preserves sender duties",
                ));
            }
            _ => {
                return Err(invalid(
                    span,
                    "function or native handle actor messages are unsupported",
                ));
            }
        }
    }
    Ok(())
}

/// Validate each callable identity and capture shape even when ordinary control flow is inactive.
fn closure(
    expr: &Expr,
    id: ir::FunctionId,
    captures: &[Expr],
    functions: &BTreeMap<usize, &Function>,
) -> Lowering<()> {
    let target = functions
        .get(&id.0)
        .ok_or_else(|| invalid(expr.span, "unknown closure function identity"))?;
    if captures.len() != target.captures.len() {
        return Err(invalid(
            expr.span,
            "closure capture count differs from lifted function",
        ));
    }
    for (value, param) in captures.iter().zip(&target.captures) {
        expect_type(value.ty.clone(), param.ty.clone(), value.span)?;
    }
    let mut ty = Type::Function(
        target.params.iter().map(|p| p.ty.clone()).collect(),
        Box::new(target.return_type.clone()),
    );
    if let Some(mailbox) = &target.mailbox {
        ty = Type::ActorFunction(Box::new(mailbox.clone()), Box::new(ty));
    } else if target.root_context {
        ty = Type::RootFunction(Box::new(ty));
    }
    expect_type(expr.ty.clone(), ty, expr.span)
}

/// Prove the source entry's zero-argument Unit and mailbox signature before native dispatch.
fn spawn(
    entry: &Expr,
    mailbox: &Type,
    expr: &Expr,
    layouts: &HashMap<Type, &ir::TypeLayout>,
) -> Lowering<()> {
    sendable(mailbox, layouts, expr.span)?;
    expect_type(
        expr.ty.clone(),
        Type::Pid(Box::new(mailbox.clone())),
        expr.span,
    )?;
    let (effect, args, result) = crate::actors::function(&entry.ty)
        .ok_or_else(|| invalid(expr.span, "spawn entry must be callable"))?;
    if !args.is_empty() || *result != Type::Unit {
        return Err(invalid(
            expr.span,
            "spawn requires a zero-argument Unit entry",
        ));
    }
    if let Some(effect) = effect {
        expect_type(effect.clone(), mailbox.clone(), expr.span)?;
    }
    Ok(())
}

/// Prove selector and timeout signatures before the compiler creates private capture frames.
fn receive(
    mailbox: &Type,
    arms: &[ir::MatchArm],
    timeout: &Option<(Box<Expr>, Box<Expr>)>,
    expr: &Expr,
    owner: Option<&Type>,
    layouts: &HashMap<Type, &ir::TypeLayout>,
) -> Lowering<()> {
    let owner = owner.ok_or_else(|| invalid(expr.span, "receive requires an actor context"))?;
    expect_type(mailbox.clone(), owner.clone(), expr.span)?;
    sendable(mailbox, layouts, expr.span)?;
    if arms.is_empty() || arms.len() > 128 {
        return Err(invalid(expr.span, "receive arm limit exceeded"));
    }
    for arm in arms {
        if let Some(guard) = &arm.guard {
            expect_type(guard.ty.clone(), Type::Bool, guard.span)?;
            crate::actors::contracts::guard(guard)?;
        }
    }
    if let Some((duration, body)) = timeout {
        expect_type(duration.ty.clone(), Type::Int, duration.span)?;
        if body.ty != Type::Never {
            expect_type(body.ty.clone(), expr.ty.clone(), body.span)?;
        }
    }
    Ok(())
}
