//! Mailbox effects are checked independently from ordinary function values and return types.
use super::*;

/// Infer mailbox schemes from owned receive patterns, never from an arbitrary scalar witness.
pub(super) fn attach(
    program: &ast::Program,
    registry: &nominal::Registry,
    signatures: &mut HashMap<String, Signature>,
    inference: &mut Inference,
) -> Checked<()> {
    for function in &program.functions {
        let Some(mailbox) = mailbox(&function.body, registry)? else {
            continue;
        };
        let signature = signatures
            .get_mut(&function.name)
            .ok_or_else(|| Diagnostic::new(function.span, "missing actor signature"))?;
        signature.mailbox = Some(mailbox.clone());
        for generic in nominal::generics([mailbox]) {
            if !signature.generics.contains(&generic) {
                signature.generics.push(generic);
            }
        }
    }
    if !signatures
        .values()
        .any(|signature| signature.mailbox.is_some())
    {
        return Ok(());
    }
    let graph = dependencies::owned_calls(program)?;
    inference
        .probe_work
        .set(inference.probe_work.get().saturating_add(graph.work));
    if inference.probe_work.get() > MAX_EXPR_COUNT * 4 {
        return Err(Diagnostic::new(
            Span::default(),
            "actor effect inference work limit exceeded",
        ));
    }
    // Propagate effect presence once along reverse lexical call edges. A reference to a
    // receiving function, or creation of a receiving closure, does not suspend its owner.
    let mut callers = vec![Vec::new(); graph.groups.len()];
    let mut pending = Vec::new();
    for (index, group) in graph.groups.iter().enumerate() {
        if signatures[&group.name].mailbox.is_some() {
            pending.push(index);
        }
        for &callee in &group.callees {
            callers[callee].push(index);
        }
    }
    while let Some(callee) = pending.pop() {
        for &caller in &callers[callee] {
            let signature = signatures
                .get_mut(&graph.groups[caller].name)
                .expect("registered caller");
            if signature.mailbox.is_none() {
                let generic = format!("$mailbox_call{caller}");
                signature.mailbox = Some(Type::Generic(generic.clone()));
                signature.generics.push(generic);
                pending.push(caller);
            }
        }
    }
    // Infer callees before callers. Recursive components share existential mailbox slots
    // until every body has contributed its constraints, then publish independent schemes.
    for component in &graph.components {
        let mut candidates = Vec::new();
        for &index in component {
            let group = &graph.groups[index];
            let signature = signatures.get_mut(&group.name).expect("registered actor");
            let Some(mailbox) = signature.mailbox.clone() else {
                continue;
            };
            let variables: HashMap<_, _> = nominal::generics([mailbox.clone()])
                .into_iter()
                .filter(|name| name.starts_with("$mailbox"))
                .map(|name| (name, inference.fresh()))
                .collect();
            if variables.is_empty() {
                continue;
            }
            let candidate = nominal::substitute(&mailbox, &variables)?;
            signature.mailbox = Some(candidate.clone());
            signature
                .generics
                .retain(|name| !variables.contains_key(name));
            candidates.push((index, candidate));
        }
        for &(index, _) in &candidates {
            for function in &program.functions[graph.groups[index].clauses.clone()] {
                let previous = (
                    inference.whole_signature,
                    inference.probing,
                    inference.template,
                );
                inference.whole_signature = true;
                let result = returns::probe(function, registry, signatures, inference);
                inference.whole_signature = previous.0;
                inference.probing = previous.1;
                inference.template = previous.2;
                if let Err(error) = result
                    && !error.message.contains(returns::WAITING)
                {
                    return Err(error);
                }
            }
        }
        for (index, candidate) in candidates {
            let group = &graph.groups[index];
            let mailbox = generalize(&inference.resolve(&candidate, group.span)?);
            let signature = signatures.get_mut(&group.name).expect("registered actor");
            signature.mailbox = Some(mailbox.clone());
            for generic in nominal::generics([mailbox]) {
                if !signature.generics.contains(&generic) {
                    signature.generics.push(generic);
                }
            }
        }
    }
    Ok(())
}

/// Constrain all selective patterns together while excluding independently lifted lambda bodies.
fn mailbox(body: &ast::Expr, registry: &nominal::Registry) -> Checked<Option<Type>> {
    let mut pending = vec![body];
    let mut patterns = Vec::new();
    let mut actor_context = false;
    let mut work = 0;
    while let Some(expr) = pending.pop() {
        work += 1;
        if work > MAX_EXPR_COUNT {
            return Err(Diagnostic::new(
                expr.span,
                "actor effect work limit exceeded",
            ));
        }
        if matches!(expr.kind, ast::ExprKind::Lambda { .. }) {
            continue;
        }
        if let ast::ExprKind::Receive { view, arms, .. } = &expr.kind {
            actor_context = true;
            patterns.extend(arms.iter().map(|a| (&a.pattern, *view)));
        }
        actor_context |= process_context(expr);
        pending.extend(crate::actors::source_children(expr));
    }
    if !actor_context {
        return Ok(None);
    }
    let signatures = HashMap::new();
    let mut checker = Checker {
        mailbox: None,
        editor: None,
        recovery: None,
        signatures: &signatures,
        registry,
        scopes: vec![HashMap::new()],
        local_count: 0,
        expr_count: 0,
        inference: Inference::default(),
        function_return: Type::Unit,
        deferred: false,
        loop_depth: 0,
    };
    let ty = checker.inference.fresh();
    parameters::constrain(
        &mut checker,
        patterns
            .into_iter()
            .map(|(p, view)| (p, view.item(&ty)))
            .collect(),
    )?;
    let ty = checker.inference.resolve(&ty, body.span)?;
    Ok(Some(generalize(&ty)))
}

/// Unconstrained mailbox components remain quantified identities until spawn supplies a type.
fn generalize(ty: &Type) -> Type {
    match ty {
        Type::Infer(id) => Type::Generic(format!("$mailbox{id}")),
        Type::Pid(t) => Type::Pid(Box::new(generalize(t))),
        Type::List(t) => Type::List(Box::new(generalize(t))),
        Type::Option(t) => Type::Option(Box::new(generalize(t))),
        Type::Result(a, b) => Type::Result(Box::new(generalize(a)), Box::new(generalize(b))),
        Type::Map(a, b) => Type::Map(Box::new(generalize(a)), Box::new(generalize(b))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(generalize).collect()),
        Type::Union(ts) => Type::Union(ts.iter().map(generalize).collect()),
        Type::Named(n, ts) => Type::Named(n.clone(), ts.iter().map(generalize).collect()),
        _ => ty.clone(),
    }
}

impl Checker<'_> {
    /// Instantiate one receiving function value with a fresh mailbox scheme and ordinary signature.
    pub(super) fn actor_name(&mut self, name: &str, span: Span) -> Checked<Option<TypedKind>> {
        let Some(signature) = self.signatures.get(name) else {
            return Ok(None);
        };
        let Some(mailbox) = &signature.mailbox else {
            return Ok(None);
        };
        let values: HashMap<_, _> = signature
            .generics
            .iter()
            .map(|n| (n.clone(), self.inference.fresh()))
            .collect();
        let params = signature
            .params
            .iter()
            .map(|t| nominal::substitute(t, &values))
            .collect::<Checked<Vec<_>>>()?;
        let result = returns::call_result(&mut self.inference, signature, &values, span)?;
        let mailbox = nominal::substitute(mailbox, &values)?;
        self.inference
            .call_names
            .insert(signature.id.0, name.into());
        Ok(Some((
            ir::ExprKind::FunctionValue {
                target: ir::CallTarget::Function(signature.id),
            },
            Type::ActorFunction(
                Box::new(mailbox),
                Box::new(Type::Function(params, Box::new(result))),
            ),
        )))
    }

    /// Resolve only the two managed source primitives; legacy actors.* remains a separate API.
    pub(super) fn actor_call(
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
                "actor operations are unsupported in deferred cleanup",
            ));
        }
        labels::positional(args)?;
        if name == "spawn" || name == "supervise" {
            return self.spawn(args, expected, span, depth, name == "supervise");
        }
        if name == "supervised_current" {
            if args.len() != 1 {
                return Err(Diagnostic::new(
                    span,
                    "supervised_current expects one typed Pid",
                ));
            }
            let pid = self.expression(&args[0], depth)?;
            let ty = self.inference.resolve(&pid.ty, span)?;
            if !matches!(ty, Type::Pid(_)) {
                return Err(Diagnostic::new(
                    span,
                    "supervised_current requires a typed Pid",
                ));
            }
            return Ok((
                ir::ExprKind::Actor(ir::ActorExpr::SupervisedCurrent { pid: Box::new(pid) }),
                Type::Result(Box::new(ty), Box::new(Type::Int)),
            ));
        }
        if args.len() != 2 {
            return Err(Diagnostic::new(span, "send expects pid and message"));
        }
        let pid = self.expression(&args[0], depth)?;
        let Type::Pid(mailbox) = self.inference.resolve(&pid.ty, span)? else {
            return Err(Diagnostic::new(span, "send requires a typed Pid"));
        };
        let message = self.expression_expected(&args[1], Some(&mailbox), depth)?;
        Ok((
            ir::ExprKind::Actor(ir::ActorExpr::Send {
                pid: Box::new(pid),
                message: Box::new(message),
            }),
            Type::Result(Box::new(Type::Unit), Box::new(Type::Int)),
        ))
    }

    /// Spawn context unifies mailbox evidence before a receiving lambda is checked.
    fn spawn(
        &mut self,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
        supervised: bool,
    ) -> Checked<TypedKind> {
        if args.len() != if supervised { 2 } else { 1 } {
            return Err(Diagnostic::new(
                span,
                if supervised {
                    "supervise expects a zero-argument Unit function and integer restart budget"
                } else {
                    "spawn expects one zero-argument Unit function"
                },
            ));
        }
        let mailbox = self.inference.fresh();
        let ty = Type::Pid(Box::new(mailbox.clone()));
        self.constrain_result(&ty, expected, span)?;
        let entry = self.spawn_entry(&args[0], &mailbox, span, depth)?;
        let max_restarts = if supervised {
            Some(Box::new(self.expression_expected(
                &args[1],
                Some(&Type::Int),
                depth,
            )?))
        } else {
            None
        };
        Ok((
            ir::ExprKind::Actor(ir::ActorExpr::Spawn {
                entry: Box::new(entry),
                mailbox,
                max_restarts,
            }),
            ty,
        ))
    }

    /// Share entry checking between legacy and isolated admission APIs.
    pub(super) fn spawn_entry(
        &mut self,
        argument: &ast::Argument,
        mailbox: &Type,
        span: Span,
        depth: usize,
    ) -> Checked<ir::Expr> {
        let function = Type::Function(Vec::new(), Box::new(Type::Unit));
        let context = Type::ActorFunction(Box::new(mailbox.clone()), Box::new(function));
        let entry = if let ast::ExprKind::Lambda { body, .. } = &argument.value.kind {
            let ordinary = Type::Function(Vec::new(), Box::new(Type::Unit));
            let expected = if self.actor_body(body) {
                &context
            } else {
                &ordinary
            };
            self.expression_expected(argument, Some(expected), depth)?
        } else {
            self.expression(argument, depth)?
        };
        let resolved = self.inference.resolve(&entry.ty, span)?;
        if resolved == Type::Never {
            return Ok(entry);
        }
        let Some((effect, params, result)) = crate::actors::function(&resolved) else {
            return Err(Diagnostic::new(span, "spawn requires a function value"));
        };
        if !params.is_empty() {
            return Err(Diagnostic::new(
                span,
                "spawn requires a zero-argument Unit function",
            ));
        }
        self.inference
            .unify(result, &Type::Unit, span, "spawn entry return")?;
        if let Some(effect) = effect {
            self.inference
                .unify(effect, mailbox, span, "spawn mailbox")?;
        }
        Ok(entry)
    }

    /// Type selective arms in isolated scopes without falsely requiring exhaustive message coverage.
    pub(super) fn receive(
        &mut self,
        view: crate::processes::ReceiveView,
        arms: &[ast::MatchArm],
        timeout: Option<&(Box<ast::Expr>, Box<ast::Expr>)>,
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let mailbox = self
            .mailbox
            .clone()
            .ok_or_else(|| Diagnostic::new(span, "receive requires an actor context"))?;
        if self.deferred {
            return Err(Diagnostic::new(span, "actor receive cannot occur in defer"));
        }
        let result = expected.cloned().unwrap_or_else(|| self.inference.fresh());
        let timeout = timeout
            .map(|(duration, body)| {
                let duration = self.expression_equal(duration, &Type::Int, depth)?;
                let body = self.expression_equal(body, &result, depth)?;
                Ok((Box::new(duration), Box::new(body)))
            })
            .transpose()?;
        let mut checked = Vec::new();
        for arm in arms {
            if let Some(guard) = &arm.guard {
                guard_source(guard)?;
            }
            self.scopes.push(HashMap::new());
            let pattern =
                self.pattern(&arm.pattern, &view.item(&mailbox), &mut HashSet::new(), 0)?;
            let guard = arm
                .guard
                .as_ref()
                .map(|g| self.expression_equal(g, &Type::Bool, depth))
                .transpose()?;
            let body = self.expression_equal(&arm.body, &result, depth)?;
            self.scopes.pop();
            checked.push(ir::MatchArm {
                pattern,
                guard,
                body,
                span: arm.span,
            });
        }
        Ok((
            ir::ExprKind::Actor(ir::ActorExpr::Receive {
                view,
                mailbox,
                arms: checked,
                timeout,
            }),
            result,
        ))
    }
}

/// Repeated mailbox selection may evaluate only scalar expressions with no hidden calls or effects.
fn guard_source(expr: &ast::Expr) -> Checked<()> {
    let mut pending = vec![expr];
    let mut work = 0;
    while let Some(expr) = pending.pop() {
        work += 1;
        if work > MAX_EXPR_COUNT {
            return Err(Diagnostic::new(
                expr.span,
                "receive guard work limit exceeded",
            ));
        }
        if matches!(
            expr.kind,
            ast::ExprKind::Binary {
                op: ast::BinaryOp::Divide | ast::BinaryOp::Remainder | ast::BinaryOp::Power,
                ..
            }
        ) {
            return Err(Diagnostic::new(
                expr.span,
                "receive guard must be non-failing",
            ));
        }
        if !matches!(
            expr.kind,
            ast::ExprKind::Name(_)
                | ast::ExprKind::Int(_)
                | ast::ExprKind::Float(_)
                | ast::ExprKind::Bool(_)
                | ast::ExprKind::String(_)
                | ast::ExprKind::Unit
                | ast::ExprKind::Unary { .. }
                | ast::ExprKind::Binary { .. }
                | ast::ExprKind::Field { .. }
        ) {
            return Err(Diagnostic::new(
                expr.span,
                "receive guard must be a pure scalar expression without calls",
            ));
        }
        pending.extend(crate::actors::source_children(expr));
    }
    Ok(())
}

impl Checker<'_> {
    /// Include the mailbox when proving generic receiving-function capabilities.
    pub(super) fn actor_requirements(
        &self,
        target: ir::CallTarget,
        params: &[Type],
        result: &Type,
        mailbox: &Type,
        span: Span,
    ) -> Checked<()> {
        self.named_requirements_with_mailbox(target, params, result, Some(mailbox), span)
    }

    /// Finalize each effect's children and reject unproved transfer/accountability boundaries.
    pub(super) fn finalize_actor(&self, actor: &mut ir::ActorExpr, span: Span) -> Checked<()> {
        match actor {
            ir::ActorExpr::Process(operation) => self.finalize_process(operation, span)?,
            ir::ActorExpr::Lowered(_) => {
                return Err(Diagnostic::new(
                    span,
                    "private actor continuation reached checker",
                ));
            }
            ir::ActorExpr::Spawn { mailbox, .. }
            | ir::ActorExpr::Receive { mailbox, .. }
            | ir::ActorExpr::Call { mailbox, .. } => {
                *mailbox = self.inference.concrete(mailbox, span).map_err(|e| {
                    Diagnostic::new(e.span, format!("cannot infer actor mailbox: {}", e.message))
                })?;
                self.actor_sendable(mailbox, span)?;
                if self.registry.contains_result(mailbox)? {
                    return Err(Diagnostic::new(
                        span,
                        "Result-bearing actor messages are unsupported until suspension accountability is proved",
                    ));
                }
            }
            ir::ActorExpr::Send { message, .. } => {
                let ty = self.inference.resolve(&message.ty, span)?;
                self.actor_sendable(&ty, span)?;
                if self.registry.contains_result(&ty)? {
                    return Err(Diagnostic::new(
                        span,
                        "Result-bearing actor messages are unsupported; send does not handle the sender's Result",
                    ));
                }
            }
            ir::ActorExpr::SupervisedCurrent { .. } => {}
        }
        for child in crate::actors::children_mut(actor) {
            self.finalize(child)?;
        }
        if let ir::ActorExpr::Receive {
            view,
            mailbox,
            arms,
            ..
        } = actor
        {
            for arm in arms.iter() {
                if let Some(guard) = &arm.guard {
                    crate::actors::contracts::guard(guard)?;
                }
            }
            let retained = coverage::selective(
                &view.item(mailbox),
                arms,
                self.registry,
                span,
                self.inference.specializing,
            )?;
            let mut index = 0;
            arms.retain(|_| {
                let keep = retained[index];
                index += 1;
                keep
            });
        }
        Ok(())
    }
}

impl Checker<'_> {
    /// Preserve source argument order while marking receiving calls for actor-tail conversion.
    pub(super) fn actor_named_call(
        &mut self,
        name: &str,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let owner = self.mailbox.clone().ok_or_else(|| {
            Diagnostic::new(span, "receiving function call requires an actor context")
        })?;
        let (kind, ty) = self
            .actor_name(name, span)?
            .ok_or_else(|| Diagnostic::new(span, "missing actor callable"))?;
        let Type::ActorFunction(mailbox, function) = ty else {
            unreachable!()
        };
        let Type::Function(params, result) = *function else {
            unreachable!()
        };
        self.inference
            .unify(&mailbox, &owner, span, "actor call mailbox")?;
        self.constrain_result(&result, expected, span)?;
        let ir::ExprKind::FunctionValue {
            target: ir::CallTarget::Function(id),
        } = kind
        else {
            unreachable!()
        };
        let signature = &self.signatures[name];
        let order = labels::order(args, &signature.labels, span)?;
        labels::required(args, signature, &order)?;
        let written: Vec<_> = order.iter().map(|i| params[*i].clone()).collect();
        let args = self.call_arguments(args, &written, span, depth)?;
        let (mut kind, result) =
            self.ordered_call(ir::CallTarget::Function(id), args, &order, *result, span);
        let call = match &mut kind {
            ir::ExprKind::Block(stmts) => match stmts.last_mut() {
                Some(ir::Stmt::Expr(expr)) => &mut expr.kind,
                _ => return Err(Diagnostic::new(span, "invalid ordered actor call")),
            },
            kind => kind,
        };
        if let ir::ExprKind::Call { args, .. } = call {
            *call = ir::ExprKind::Actor(ir::ActorExpr::Call {
                function: id,
                args: std::mem::take(args),
                mailbox: owner,
            });
        }
        Ok((kind, result))
    }
}

impl Checker<'_> {
    /// Detect owned suspension effects without attributing a nested closure's body to its creator.
    fn actor_body(&self, body: &ast::Expr) -> bool {
        let mut pending = vec![body];
        while let Some(expr) = pending.pop() {
            if matches!(expr.kind, ast::ExprKind::Lambda { .. }) {
                continue;
            }
            if matches!(expr.kind, ast::ExprKind::Receive { .. }) || process_context(expr) {
                return true;
            }
            let name = match &expr.kind {
                ast::ExprKind::Call { name, .. } | ast::ExprKind::Pipe { name, .. } => Some(name),
                ast::ExprKind::GlobalCall { resolved, .. }
                | ast::ExprKind::GlobalPipe { resolved, .. } => Some(resolved),
                _ => None,
            };
            if name.is_some_and(|name| {
                self.signatures
                    .get(name)
                    .is_some_and(|f| f.mailbox.is_some())
            }) {
                return true;
            }
            pending.extend(crate::actors::source_children(expr));
        }
        false
    }

    /// Require recursively immutable, accounted message layouts while keeping generic obligations open.
    pub(super) fn actor_sendable(&self, ty: &Type, span: Span) -> Checked<()> {
        let mut pending = vec![ty.clone()];
        let mut seen = HashSet::new();
        let mut work = 0usize;
        while let Some(ty) = pending.pop() {
            work = work.saturating_add(crate::unions::cost(&ty, span)?);
            if work > 400_000 || pending.len() > 4096 {
                return Err(Diagnostic::new(
                    span,
                    "actor message type work limit exceeded",
                ));
            }
            if !seen.insert(ty.clone()) {
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
                    | crate::runtime::NativeType::MonitorRef,
                )
                | Type::Pid(_)
                | Type::Generic(_)
                | Type::Infer(_) => {}
                Type::List(item) | Type::Option(item) => pending.push(*item),
                Type::Tuple(fields) | Type::Union(fields) => pending.extend(fields),
                Type::Map(key, value) => pending.extend([*key, *value]),
                Type::Named(ref name, _) if name == "Ptr" => {
                    return Err(Diagnostic::new(
                        span,
                        "foreign pointers cannot cross actor boundaries",
                    ));
                }
                Type::Named(_, _) => pending.extend(
                    self.registry
                        .layout(&ty, span)?
                        .variants
                        .into_iter()
                        .flatten(),
                ),
                Type::Result(_, _) => {
                    return Err(Diagnostic::new(
                        span,
                        "Result-bearing actor messages are unsupported; send does not handle the sender's Result",
                    ));
                }
                _ => {
                    return Err(Diagnostic::new(
                        span,
                        "function or native handle actor messages are unsupported",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Actor-only primitives establish context even without a receive in this body.
fn process_context(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Call { name, .. } | ast::ExprKind::Pipe { name, .. } => {
            crate::processes::requires_actor(name)
        }
        ast::ExprKind::GlobalCall { resolved, .. } | ast::ExprKind::GlobalPipe { resolved, .. } => {
            crate::processes::requires_actor(resolved)
        }
        _ => false,
    }
}
