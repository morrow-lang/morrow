//! Fuse small pure self-tail paths into a bounded actor callback. Ordinary ABIs stay intact.
use super::*;

impl Builder<'_> {
    pub(super) fn batch_body(&mut self, function: &Function) -> Lowering<Expr> {
        let cost = tail_helpers::expression_work(&function.body, &self.inline_work).max(1);
        if function.mailbox.is_some()
            || !function.captures.is_empty()
            || cost > tail_helpers::STEP_WORK / 2
        {
            return Ok(function.body.clone());
        }
        // Counting both branches and the replaced call overestimates each executed
        // path. Also cap zero-cost syntax growth and leave room for normalization
        // and CPS depth: an optimization must not exhaust their recursion budget.
        let copies = copies(&function.body, cost);
        if copies < 2 || eligible(&function.body, function.id, true, 0) != Some((1, false)) {
            return Ok(function.body.clone());
        }
        let mut body = function.body.clone();
        self.expand_tail(&mut body, function, copies - 1, 0)?;
        Ok(body)
    }

    fn expand_tail(
        &mut self,
        expr: &mut Expr,
        function: &Function,
        remaining: usize,
        depth: usize,
    ) -> Lowering<()> {
        self.work += 1;
        if depth >= MAX_DEPTH || self.work > MAX_NODES {
            return Err(invalid(expr.span, "actor tail batch work limit exceeded"));
        }
        if remaining == 0 {
            return Ok(());
        }
        if let ExprKind::Call {
            target: CallTarget::Function(id),
            args,
        } = &expr.kind
            && *id == function.id
        {
            let offset = self.next_local;
            self.next_local = offset
                .checked_add(function.local_count)
                .ok_or_else(|| invalid(expr.span, "actor tail batch local limit exceeded"))?;
            let mut body = function.body.clone();
            self.rename_batch(&mut body, offset, depth + 1)?;
            self.expand_tail(&mut body, function, remaining - 1, depth + 1)?;
            // The checked IR already stages labeled arguments in written order.
            // Bind every parameter before the next body so swaps and faults retain
            // simultaneous full-width assignment and exactly-once evaluation.
            let mut statements: Vec<_> = args
                .iter()
                .zip(&function.params)
                .map(|(value, param)| Stmt::Let {
                    id: ir::LocalId(param.id.0 + offset),
                    value: value.clone(),
                })
                .collect();
            statements.push(Stmt::Expr(body));
            expr.kind = ExprKind::Block(statements);
        } else {
            for child in children_mut(expr) {
                self.expand_tail(child, function, remaining, depth + 1)?;
            }
        }
        Ok(())
    }

    fn rename_batch(&mut self, expr: &mut Expr, offset: usize, depth: usize) -> Lowering<()> {
        self.work += 1;
        if depth >= MAX_DEPTH || self.work > MAX_NODES {
            return Err(invalid(expr.span, "actor tail batch rename limit exceeded"));
        }
        match &mut expr.kind {
            ExprKind::Local(id) => id.0 += offset,
            ExprKind::Block(stmts) => {
                for stmt in stmts {
                    if let Stmt::Let { id, .. } = stmt {
                        id.0 += offset;
                    }
                }
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    rename_pattern(&mut arm.pattern, offset);
                }
            }
            _ => {}
        }
        for child in children_mut(expr) {
            self.rename_batch(child, offset, depth + 1)?;
        }
        Ok(())
    }
}

/// A small, fixed source-growth bound also limits eligibility and cloning work.
fn copies(body: &Expr, cost: usize) -> usize {
    let mut pending = vec![(body, 1)];
    let mut nodes = 0;
    let mut depth = 0;
    while let Some((expr, level)) = pending.pop() {
        nodes += 1;
        depth = depth.max(level);
        if nodes > 1024 || depth >= MAX_DEPTH / 4 {
            return 1;
        }
        pending.extend(
            ir::children(expr)
                .into_iter()
                .map(|child| (child, level + 1)),
        );
    }
    (tail_helpers::STEP_WORK / cost)
        .min(8)
        .min(1024 / nodes)
        .min(MAX_DEPTH / (4 * (depth + 1)))
}

/// Return (self calls, pure). An effect may occur only on a terminating path;
/// every computation preceding the unique self-tail call is scalar and finite.
fn eligible(
    expr: &Expr,
    self_id: ir::FunctionId,
    tail: bool,
    depth: usize,
) -> Option<(usize, bool)> {
    if depth >= MAX_DEPTH {
        return None;
    }
    match &expr.kind {
        ExprKind::Local(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Unit => Some((0, true)),
        ExprKind::Binary { left, right, .. } => {
            if !matches!(left.ty, Type::Int | Type::Float | Type::Bool)
                || !matches!(right.ty, Type::Int | Type::Float | Type::Bool)
            {
                return None;
            }
            let a = eligible(left, self_id, false, depth + 1)?;
            let b = eligible(right, self_id, false, depth + 1)?;
            Some((0, a.1 && b.1))
        }
        ExprKind::Unary { value, .. } => eligible(value, self_id, false, depth + 1),
        ExprKind::Call {
            target: CallTarget::Function(id),
            ..
        } if *id == self_id && tail => pure_children(expr, self_id, depth).then_some((1, false)),
        ExprKind::Block(stmts) => {
            let mut calls = 0;
            let mut pure = true;
            for (index, stmt) in stmts.iter().enumerate() {
                let (value, tail) = match stmt {
                    Stmt::Expr(value) => (value, tail && index + 1 == stmts.len()),
                    Stmt::Let { value, .. } => (value, false),
                    _ => return None,
                };
                let (found, child_pure) = eligible(value, self_id, tail, depth + 1)?;
                if found != 0 && !pure {
                    return None;
                }
                calls += found;
                pure &= child_pure;
            }
            (calls <= 1).then_some((calls, pure))
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            if eligible(condition, self_id, false, depth + 1)? != (0, true) {
                return None;
            }
            let (a, ap) = eligible(then_branch, self_id, tail, depth + 1)?;
            let (b, bp) = if let Some(branch) = else_branch {
                eligible(branch, self_id, tail, depth + 1)?
            } else {
                (0, true)
            };
            (a + b <= 1).then_some((a + b, ap && bp))
        }
        // Terminal effects retain their normal normalization and CPS boundaries.
        ExprKind::String(_)
        | ExprKind::Interpolate(_)
        | ExprKind::Tuple(_)
        | ExprKind::Construct { .. }
        | ExprKind::CustomConstruct { .. }
        | ExprKind::Match { .. }
        | ExprKind::Actor(ir::ActorExpr::Send { .. })
        | ExprKind::Call {
            target: CallTarget::Builtin(_) | CallTarget::Runtime(_),
            ..
        } => {
            if matches!(expr.kind, ExprKind::Call { target: CallTarget::Builtin(b), .. }
                if crate::lowering::higher_order::is_higher_order(b))
            {
                return None;
            }
            for child in ir::children(expr) {
                if eligible(child, self_id, false, depth + 1)?.0 != 0 {
                    return None;
                }
            }
            Some((0, false))
        }
        _ => None,
    }
}

fn pure_children(expr: &Expr, self_id: ir::FunctionId, depth: usize) -> bool {
    ir::children(expr)
        .into_iter()
        .all(|child| eligible(child, self_id, false, depth + 1) == Some((0, true)))
}

/// Deliberately matches the eligibility whitelist; unsupported forms never reach expansion.
fn children_mut(expr: &mut Expr) -> Vec<&mut Expr> {
    match &mut expr.kind {
        ExprKind::Binary { left, right, .. } => vec![left, right],
        ExprKind::Unary { value, .. } => vec![value],
        ExprKind::Block(stmts) => stmts
            .iter_mut()
            .filter_map(|stmt| match stmt {
                Stmt::Let { value, .. } | Stmt::Expr(value) => Some(value),
                _ => None,
            })
            .collect(),
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let mut children = vec![condition.as_mut(), then_branch.as_mut()];
            children.extend(else_branch.as_deref_mut());
            children
        }
        ExprKind::Match { value, arms } => {
            let mut children = vec![value.as_mut()];
            for arm in arms {
                children.extend(arm.guard.as_mut());
                children.push(&mut arm.body);
            }
            children
        }
        ExprKind::Call { args, .. }
        | ExprKind::Interpolate(args)
        | ExprKind::Tuple(args)
        | ExprKind::CustomConstruct { fields: args, .. } => args.iter_mut().collect(),
        ExprKind::Construct { value, .. } => value.iter_mut().map(|value| value.as_mut()).collect(),
        ExprKind::Actor(ir::ActorExpr::Send { pid, message }) => vec![pid, message],
        _ => vec![],
    }
}

fn rename_pattern(pattern: &mut Pattern, offset: usize) {
    match pattern {
        Pattern::Bind(id)
        | Pattern::Constructor {
            binding: Some(id), ..
        } => id.0 += offset,
        Pattern::UnionSelect {
            binding: Some(param),
            ..
        } => param.id.0 += offset,
        Pattern::Newtype(inner) => rename_pattern(inner, offset),
        Pattern::Tuple(fields) | Pattern::Variant { fields, .. } => {
            for field in fields {
                rename_pattern(field, offset);
            }
        }
        Pattern::List { prefix, rest } => {
            for field in prefix {
                rename_pattern(field, offset);
            }
            if let Some(rest) = rest {
                rename_pattern(rest, offset);
            }
        }
        Pattern::TupleRest { prefix, rest } => {
            for field in prefix {
                rename_pattern(field, offset);
            }
            rename_pattern(rest, offset);
        }
        _ => {}
    }
}
