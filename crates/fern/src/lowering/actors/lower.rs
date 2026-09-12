//! Typed continuations retain only lexical values and never the execution or fault context.
use super::*;

struct Builder {
    plan: Plan,
    next_function: usize,
    source_count: usize,
    next_local: usize,
    mailbox: Type,
    work: usize,
    targets: BTreeMap<usize, Function>,
}
#[derive(Clone)]
struct Continuation {
    value: ir::Param,
    entry: Expr,
}

/// Convert receiving bodies once, retaining ordinary functions for their existing native ABI.
pub(super) fn program(program: &ir::Program) -> Lowering<Plan> {
    let next = program
        .functions
        .iter()
        .map(|f| f.id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid(Span::default(), "actor function identity limit exceeded"))?;
    let mut builder = Builder {
        plan: Plan::default(),
        next_function: next,
        source_count: program.functions.len(),
        next_local: 0,
        mailbox: Type::Unit,
        work: 0,
        targets: BTreeMap::new(),
    };
    for function in &program.functions {
        if function.mailbox.is_some() {
            let identity = builder.identity(function.body.span)?;
            builder.plan.entries.insert(function.id.0, identity);
            builder.targets.insert(function.id.0, function.clone());
        }
    }
    for function in &program.functions {
        let Some(mailbox) = &function.mailbox else {
            continue;
        };
        builder.mailbox = mailbox.clone();
        builder.next_local = function.local_count;
        let body = builder.expression(&function.body, None, 0)?;
        let mut step = builder.function(body, vec![], false)?;
        builder.plan.steps.remove(&step.id.0);
        step.id = ir::FunctionId(builder.plan.entries[&function.id.0]);
        builder.plan.steps.insert(step.id.0, mailbox.clone());
        step.captures = function
            .captures
            .iter()
            .chain(&function.params)
            .cloned()
            .collect();
        builder.plan.functions.push(step);
    }
    Ok(builder.plan)
}

impl Builder {
    /// Preserve each branch's completion separately; joining continuation bodies are shared.
    fn expression(
        &mut self,
        expr: &Expr,
        next: Option<&Continuation>,
        depth: usize,
    ) -> Lowering<Expr> {
        self.work += 1;
        if depth >= MAX_DEPTH || self.work > MAX_NODES {
            return Err(invalid(expr.span, "actor continuation work limit exceeded"));
        }
        match &expr.kind {
            ExprKind::Block(stmts) => self.block(stmts, next, expr.span, depth + 1),
            ExprKind::Return(value) => self.expression(value, None, depth + 1),
            ExprKind::Actor(ir::ActorExpr::Call {
                function,
                args,
                mailbox,
            }) => self.tail_call(*function, args, mailbox, next, expr.span),
            ExprKind::Actor(ir::ActorExpr::Receive {
                arms,
                timeout,
                mailbox,
            }) => {
                expect_type(mailbox.clone(), self.mailbox.clone(), expr.span)?;
                self.receive(arms, timeout, next, expr.span, depth + 1)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                atomic(condition)?;
                let then_branch = Box::new(self.expression(then_branch, next, depth + 1)?);
                let otherwise = else_branch
                    .as_deref()
                    .cloned()
                    .unwrap_or_else(|| unit(expr.span));
                let else_branch = Some(Box::new(self.expression(&otherwise, next, depth + 1)?));
                Ok(node(
                    ExprKind::If {
                        condition: condition.clone(),
                        then_branch,
                        else_branch,
                    },
                    Type::Int,
                    expr.span,
                ))
            }
            ExprKind::Match { value, arms } => {
                atomic(value)?;
                let mut arms = arms.clone();
                for arm in &mut arms {
                    arm.body = self.expression(&arm.body, next, depth + 1)?;
                }
                Ok(node(
                    ExprKind::Match {
                        value: value.clone(),
                        arms,
                    },
                    Type::Int,
                    expr.span,
                ))
            }
            _ => {
                atomic(expr)?;
                Ok(finish(expr.clone(), next))
            }
        }
    }

    /// Evaluate tail-call arguments before publishing the exact receiving entry's parameter frame.
    fn tail_call(
        &mut self,
        function: ir::FunctionId,
        args: &[Expr],
        mailbox: &Type,
        next: Option<&Continuation>,
        span: Span,
    ) -> Lowering<Expr> {
        if next.is_some() {
            return Err(invalid(
                span,
                "receiving calls must be in actor tail position",
            ));
        }
        expect_type(mailbox.clone(), self.mailbox.clone(), span)?;
        let target = self
            .targets
            .get(&function.0)
            .ok_or_else(|| invalid(span, "unknown receiving call identity"))?;
        if !target.captures.is_empty() || args.len() != target.params.len() {
            return Err(invalid(
                span,
                "invalid receiving call capture/parameter signature",
            ));
        }
        for (arg, param) in args.iter().zip(&target.params) {
            atomic(arg)?;
            expect_type(arg.ty.clone(), param.ty.clone(), arg.span)?;
        }
        let entry = node(
            ExprKind::Closure {
                function: ir::FunctionId(self.plan.entries[&function.0]),
                captures: args.to_vec(),
            },
            Type::Function(vec![], Box::new(Type::Int)),
            span,
        );
        Ok(operation(Operation::Continue(Box::new(entry)), span))
    }

    /// Split only at suspension/control boundaries; a straight-line prefix remains one step.
    fn block(
        &mut self,
        stmts: &[Stmt],
        next: Option<&Continuation>,
        span: Span,
        depth: usize,
    ) -> Lowering<Expr> {
        let mut prefix = Vec::new();
        for (index, stmt) in stmts.iter().enumerate() {
            if index + 1 == stmts.len()
                && let Stmt::Expr(value) = stmt
            {
                prefix.push(Stmt::Expr(self.expression(value, next, depth)?));
                return Ok(node(ExprKind::Block(prefix), Type::Int, span));
            }
            let (value, binding) = match stmt {
                Stmt::Let { id, value } => (value, Some(*id)),
                Stmt::Expr(value) => (value, None),
                Stmt::LetElse { .. } => {
                    for child in stmt_children(stmt) {
                        atomic(child)?;
                    }
                    prefix.push(stmt.clone());
                    continue;
                }
            };
            if !needs(value) {
                prefix.push(stmt.clone());
                continue;
            }
            let id = match binding {
                Some(id) => id,
                None => self.local(span)?,
            };
            let rest = self.block(&stmts[index + 1..], next, span, depth + 1)?;
            let entry = self.closure(rest, vec![], false)?;
            let continuation = Continuation {
                value: ir::Param {
                    id,
                    ty: value.ty.clone(),
                },
                entry,
            };
            prefix.push(Stmt::Expr(self.expression(
                value,
                Some(&continuation),
                depth + 1,
            )?));
            return Ok(node(ExprKind::Block(prefix), Type::Int, span));
        }
        prefix.push(Stmt::Expr(finish(unit(span), next)));
        Ok(node(ExprKind::Block(prefix), Type::Int, span))
    }

    /// Selectors allocate a selected frame only after its complete pattern and guard succeed.
    fn receive(
        &mut self,
        arms: &[MatchArm],
        timeout: &Option<(Box<Expr>, Box<Expr>)>,
        next: Option<&Continuation>,
        span: Span,
        depth: usize,
    ) -> Lowering<Expr> {
        let candidate = ir::Param {
            id: self.local(span)?,
            ty: self.mailbox.clone(),
        };
        let value = Box::new(node(
            ExprKind::Local(candidate.id),
            candidate.ty.clone(),
            span,
        ));
        let mut selected = Vec::new();
        for arm in arms {
            let body = self.expression(&arm.body, next, depth)?;
            let entry = self.closure(body, vec![], false)?;
            selected.push(MatchArm {
                pattern: arm.pattern.clone(),
                guard: arm.guard.clone(),
                body: operation(Operation::Pointer(Box::new(entry)), span),
                span: arm.span,
            });
        }
        let selector = operation(
            Operation::Select {
                value,
                arms: selected,
            },
            span,
        );
        let selector = Box::new(self.closure(selector, vec![candidate], true)?);
        let (duration, timeout) = if let Some((duration, body)) = timeout {
            atomic(duration)?;
            let body = self.expression(body, next, depth)?;
            (
                duration.clone(),
                Some(Box::new(self.closure(body, vec![], false)?)),
            )
        } else {
            (Box::new(integer(-1, span)), None)
        };
        Ok(operation(
            Operation::Register {
                selector,
                timeout,
                duration,
            },
            span,
        ))
    }

    /// Construct one immutable function identity and its concrete lexical capture signature.
    fn function(
        &mut self,
        body: Expr,
        params: Vec<ir::Param>,
        selector: bool,
    ) -> Lowering<Function> {
        if self.source_count.saturating_add(self.plan.functions.len()) >= 4096 {
            return Err(invalid(
                body.span,
                "actor continuation count limit exceeded",
            ));
        }
        let id = ir::FunctionId(self.identity(body.span)?);
        let captures = free(&body, &params, &mut self.work)?;
        let local_count = self.next_local;
        if selector {
            self.plan.selectors.insert(id.0, self.mailbox.clone());
        } else {
            self.plan.steps.insert(id.0, self.mailbox.clone());
        }
        Ok(Function {
            mailbox: None,
            id,
            name: format!("$actor{}", id.0),
            params,
            captures,
            return_type: Type::Int,
            body,
            local_count,
        })
    }

    /// Materialize only a typed closure recipe here; allocation happens at its source execution point.
    fn closure(&mut self, body: Expr, params: Vec<ir::Param>, selector: bool) -> Lowering<Expr> {
        let function = self.function(body, params, selector)?;
        let span = function.body.span;
        let captures = function
            .captures
            .iter()
            .map(|p| node(ExprKind::Local(p.id), p.ty.clone(), span))
            .collect();
        let ty = Type::Function(
            function.params.iter().map(|p| p.ty.clone()).collect(),
            Box::new(Type::Int),
        );
        let entry = node(
            ExprKind::Closure {
                function: function.id,
                captures,
            },
            ty,
            span,
        );
        self.plan.functions.push(function);
        Ok(entry)
    }

    /// Fresh compiler locals share the source function's bounded identity space.
    fn local(&mut self, span: Span) -> Lowering<ir::LocalId> {
        if self.next_local >= MAX_NODES {
            return Err(invalid(span, "actor generated local limit exceeded"));
        }
        let id = ir::LocalId(self.next_local);
        self.next_local = self
            .next_local
            .checked_add(1)
            .ok_or_else(|| invalid(span, "actor generated local limit exceeded"))?;
        Ok(id)
    }

    /// Allocate a compiler identity only within the original bounded public-IR identity domain.
    fn identity(&mut self, span: Span) -> Lowering<usize> {
        if self.next_function >= MAX_NODES {
            return Err(invalid(
                span,
                "actor generated function identity limit exceeded",
            ));
        }
        let id = self.next_function;
        self.next_function = self
            .next_function
            .checked_add(1)
            .ok_or_else(|| invalid(span, "actor generated function identity limit exceeded"))?;
        Ok(id)
    }
}

/// Evaluate a return value before scheduling its continuation; completed Unit values are discarded.
fn finish(value: Expr, next: Option<&Continuation>) -> Expr {
    let span = value.span;
    let stmts = if let Some(next) = next {
        vec![
            Stmt::Let {
                id: next.value.id,
                value,
            },
            Stmt::Expr(operation(
                Operation::Continue(Box::new(next.entry.clone())),
                span,
            )),
        ]
    } else {
        vec![Stmt::Expr(value), Stmt::Expr(integer(2, span))]
    };
    node(ExprKind::Block(stmts), Type::Int, span)
}
/// Emit compiler-private operations with a single explicit status/pointer representation.
fn operation(operation: Operation, span: Span) -> Expr {
    node(
        ExprKind::Actor(ir::ActorExpr::Lowered(Lowered { operation })),
        Type::Int,
        span,
    )
}
/// Construct a private typed expression without changing the original diagnostic position.
fn node(kind: ExprKind, ty: Type, span: Span) -> Expr {
    Expr { kind, ty, span }
}
/// Represent one scheduler status as the full-width native callback result.
fn integer(value: i64, span: Span) -> Expr {
    node(ExprKind::Int(value), Type::Int, span)
}
/// Supply the ordinary Unit value for an absent branch before continuation conversion.
fn unit(span: Span) -> Expr {
    node(ExprKind::Unit, Type::Unit, span)
}

/// Detect only execution-owned control; lifted callable bodies are separate functions.
fn needs(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Return(_)
            | ExprKind::Actor(ir::ActorExpr::Receive { .. } | ir::ActorExpr::Call { .. })
    ) || ir::children(expr).into_iter().any(needs)
}
/// Reject suspended strict operands until a later checkpoint provides their continuation forms.
fn atomic(expr: &Expr) -> Lowering<()> {
    if needs(expr) {
        Err(invalid(
            expr.span,
            "receive is unsupported in this strict operand or loop; bind it before the operation",
        ))
    } else {
        Ok(())
    }
}
fn stmt_children(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::LetElse {
            value, else_branch, ..
        } => vec![value, else_branch],
        Stmt::Let { value, .. } | Stmt::Expr(value) => vec![value],
    }
}

/// Discover free identities from actual local reads; source local IDs are unique within each function.
fn free(body: &Expr, params: &[ir::Param], work: &mut usize) -> Lowering<Vec<ir::Param>> {
    let mut reads = BTreeMap::new();
    let mut bound: BTreeSet<_> = params.iter().map(|p| p.id.0).collect();
    let mut pending = vec![body];
    while let Some(expr) = pending.pop() {
        *work += 1;
        if *work > MAX_NODES {
            return Err(invalid(expr.span, "actor capture work limit exceeded"));
        }
        if let ExprKind::Local(id) = expr.kind
            && let Some(previous) = reads.insert(id.0, expr.ty.clone())
        {
            expect_type(previous, expr.ty.clone(), expr.span)?;
        }
        if let ExprKind::Block(stmts) = &expr.kind {
            for stmt in stmts {
                match stmt {
                    Stmt::Let { id, .. } => {
                        bound.insert(id.0);
                    }
                    Stmt::LetElse { pattern, .. } => bind(pattern, &mut bound),
                    _ => {}
                }
            }
        }
        match &expr.kind {
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    bind(&arm.pattern, &mut bound);
                }
            }
            ExprKind::Actor(ir::ActorExpr::Lowered(Lowered {
                operation: Operation::Select { arms, .. },
            })) => {
                for arm in arms {
                    bind(&arm.pattern, &mut bound);
                }
            }
            ExprKind::For { pattern, .. } => bind(pattern, &mut bound),
            ExprKind::With {
                steps, handlers, ..
            } => {
                for step in steps {
                    bind(&step.pattern, &mut bound);
                }
                for handler in handlers {
                    bound.insert(handler.error.id.0);
                }
            }
            _ => {}
        }
        pending.extend(ir::children(expr));
    }
    Ok(reads
        .into_iter()
        .filter(|(id, _)| !bound.contains(id))
        .map(|(id, ty)| ir::Param {
            id: ir::LocalId(id),
            ty,
        })
        .collect())
}
/// Pattern bindings remain local to selected arms and never become outer frame captures.
fn bind(pattern: &Pattern, bound: &mut BTreeSet<usize>) {
    match pattern {
        Pattern::Bind(id)
        | Pattern::Constructor {
            binding: Some(id), ..
        } => {
            bound.insert(id.0);
        }
        Pattern::UnionSelect {
            binding: Some(param),
            ..
        } => {
            bound.insert(param.id.0);
        }
        Pattern::Newtype(inner) => bind(inner, bound),
        Pattern::Tuple(fields) | Pattern::Variant { fields, .. } => {
            for field in fields {
                bind(field, bound);
            }
        }
        Pattern::List { prefix, rest } => {
            for field in prefix {
                bind(field, bound);
            }
            if let Some(rest) = rest {
                bind(rest, bound);
            }
        }
        Pattern::TupleRest { prefix, rest } => {
            for field in prefix {
                bind(field, bound);
            }
            bind(rest, bound);
        }
        _ => {}
    }
}
