//! A-normalization preserves left-to-right strict operand evaluation around suspension.
use super::*;
use crate::ast::BinaryOp;

impl Builder<'_> {
    pub(super) fn normalize(&mut self, source: &Expr, depth: usize) -> Lowering<Expr> {
        self.work += 1;
        if depth >= MAX_DEPTH || self.work > MAX_NODES {
            return Err(invalid(
                source.span,
                "actor operand normalization limit exceeded",
            ));
        }
        if let ExprKind::Defer(value) = &source.kind {
            return self.register_defer(value, source.span);
        }
        if matches!(source.kind, ExprKind::With { .. }) {
            return self.normalize_with(source, depth);
        }
        if matches!(source.kind, ExprKind::Try(_)) {
            return self.normalize_try(source, depth);
        }
        if let ExprKind::Match { arms, .. } = &source.kind {
            for arm in arms {
                if arm.guard.as_ref().is_some_and(|guard| self.needs(guard)) {
                    return Err(invalid(
                        arm.span,
                        "actor match guard must be finite and cannot suspend; bind the computed condition before matching",
                    ));
                }
            }
        }
        if let Some(sum) = self.normalize_sum(source, depth)? {
            return Ok(sum);
        }
        let mut expr = source.clone();
        if let ExprKind::Binary {
            op: BinaryOp::And | BinaryOp::Or,
            left,
            right,
        } = &expr.kind
        {
            let and = matches!(
                expr.kind,
                ExprKind::Binary {
                    op: BinaryOp::And,
                    ..
                }
            );
            expr.kind = ExprKind::If {
                condition: left.clone(),
                then_branch: Box::new(if and {
                    *right.clone()
                } else {
                    node(ExprKind::Bool(true), Type::Bool, source.span)
                }),
                else_branch: Some(Box::new(if and {
                    node(ExprKind::Bool(false), Type::Bool, source.span)
                } else {
                    *right.clone()
                })),
            };
        }
        let mut operands = match &mut expr.kind {
            ExprKind::Block(stmts) => {
                let mut prefix = Vec::new();
                for (index, stmt) in stmts.iter().enumerate() {
                    match stmt {
                        Stmt::Let { id, value } => prefix.push(Stmt::Let {
                            id: *id,
                            value: self.normalize(value, depth + 1)?,
                        }),
                        Stmt::Expr(value) => {
                            prefix.push(Stmt::Expr(self.normalize(value, depth + 1)?))
                        }
                        Stmt::LetElse {
                            pattern,
                            value,
                            else_branch,
                        } => {
                            // The successful pattern owns the remaining lexical block.
                            // Match lowering can suspend the initializer before destructuring.
                            let rest = node(
                                ExprKind::Block(stmts[index + 1..].to_vec()),
                                expr.ty.clone(),
                                expr.span,
                            );
                            let matching = node(
                                ExprKind::Match {
                                    value: Box::new(value.clone()),
                                    arms: vec![
                                        MatchArm {
                                            pattern: pattern.clone(),
                                            guard: None,
                                            body: rest,
                                            span: value.span,
                                        },
                                        MatchArm {
                                            pattern: Pattern::Wildcard,
                                            guard: None,
                                            body: else_branch.clone(),
                                            span: else_branch.span,
                                        },
                                    ],
                                },
                                expr.ty.clone(),
                                expr.span,
                            );
                            prefix.push(Stmt::Expr(self.normalize(&matching, depth + 1)?));
                            break;
                        }
                    }
                }
                return Ok(node(ExprKind::Block(prefix), expr.ty, expr.span));
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                **then_branch = self.normalize(then_branch, depth + 1)?;
                if let Some(branch) = else_branch {
                    **branch = self.normalize(branch, depth + 1)?;
                }
                vec![condition.as_mut()]
            }
            ExprKind::Match { value, arms } => {
                for arm in arms {
                    arm.body = self.normalize(&arm.body, depth + 1)?;
                }
                vec![value.as_mut()]
            }
            ExprKind::For { iterable, body, .. } => {
                **body = self.normalize(body, depth + 1)?;
                vec![iterable.as_mut()]
            }
            ExprKind::Return(value) => {
                **value = self.normalize(value, depth + 1)?;
                return Ok(expr);
            }
            ExprKind::Actor(ir::ActorExpr::Receive { arms, timeout, .. }) => {
                for arm in arms {
                    arm.body = self.normalize(&arm.body, depth + 1)?;
                }
                if let Some((_, body)) = timeout {
                    **body = self.normalize(body, depth + 1)?;
                }
                return Ok(expr);
            }
            ExprKind::Actor(actor) => crate::actors::children_mut(actor),
            ExprKind::Binary { left, right, .. }
            | ExprKind::Range {
                start: left,
                end: right,
                ..
            } => vec![left.as_mut(), right.as_mut()],
            ExprKind::Call { args, .. }
            | ExprKind::ForeignCall { args, .. }
            | ExprKind::List(args)
            | ExprKind::Tuple(args)
            | ExprKind::Interpolate(args)
            | ExprKind::CustomConstruct { fields: args, .. }
            | ExprKind::Closure { captures: args, .. } => args.iter_mut().collect(),
            ExprKind::Invoke { callee, args } => std::iter::once(callee.as_mut())
                .chain(args.iter_mut())
                .collect(),
            ExprKind::Map(entries) => entries.iter_mut().flat_map(|(k, v)| [k, v]).collect(),
            ExprKind::Unary { value, .. }
            | ExprKind::Field { value, .. }
            | ExprKind::UnionInject { value }
            | ExprKind::UnionWiden { value }
            | ExprKind::Wrap(value)
            | ExprKind::Unwrap(value)
            | ExprKind::Try(value)
            | ExprKind::JsonCodec { input: value, .. } => vec![value.as_mut()],
            ExprKind::Construct { value, .. } => value.iter_mut().map(|v| v.as_mut()).collect(),
            _ => return Ok(expr),
        };
        for operand in &mut operands {
            **operand = self.normalize(operand, depth + 1)?;
        }
        if !operands.iter().any(|operand| self.needs(operand))
            && tail_helpers::expression_work(source, &self.inline_work) <= tail_helpers::STEP_WORK
        {
            return Ok(expr);
        }
        let mut prefix = Vec::new();
        // Bind every preceding operand too: effects cannot slide past a yielded later operand.
        for operand in operands {
            let id = self.local(operand.span)?;
            let value = std::mem::replace(
                operand,
                node(ExprKind::Local(id), operand.ty.clone(), operand.span),
            );
            prefix.push(Stmt::Let { id, value });
        }
        let ty = expr.ty.clone();
        prefix.push(Stmt::Expr(expr));
        Ok(node(ExprKind::Block(prefix), ty, source.span))
    }
}
