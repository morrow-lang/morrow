//! Iteration continuations own immutable collection/index frames, never native stacks.
use super::*;
use crate::ast::BinaryOp;

impl Builder<'_> {
    /// Evaluate the collection once, then publish at most one iteration per callback.
    pub(super) fn iteration(
        &mut self,
        pattern: &Pattern,
        iterable: &Expr,
        body: &Expr,
        next: Option<&Continuation>,
        span: Span,
        depth: usize,
    ) -> Lowering<Expr> {
        atomic(iterable)?;
        let item = match &iterable.ty {
            Type::Range => Type::Int,
            Type::List(item) => *item.clone(),
            Type::Map(key, value) => Type::Tuple(vec![*key.clone(), *value.clone()]),
            _ => return Err(invalid(span, "for requires List, Map, or Range")),
        };
        let collection = ir::Param {
            id: self.local(span)?,
            ty: iterable.ty.clone(),
        };
        let index = ir::Param {
            id: self.local(span)?,
            ty: Type::Int,
        };
        let local = |p: &ir::Param| node(ExprKind::Local(p.id), p.ty.clone(), span);
        let field = |field| {
            operation(
                Operation::IterateField {
                    value: Box::new(local(&collection)),
                    field,
                },
                span,
            )
        };
        let completed = self.finish(unit(span), next)?;
        let done = operation(
            Operation::Continue(Box::new(self.closure(completed, vec![], false)?)),
            span,
        );
        // Include lexical loop exits when discovering the frame; an inner break/continue
        // can schedule an outer frame whose captures are absent from source local reads.
        let mut evidence = vec![
            Stmt::Expr(node(
                ExprKind::For {
                    pattern: pattern.clone(),
                    iterable: Box::new(local(&collection)),
                    body: Box::new(body.clone()),
                },
                Type::Unit,
                span,
            )),
            Stmt::Expr(done.clone()),
            Stmt::Expr(local(&index)),
        ];
        if let Some(target) = &self.return_to {
            evidence.push(Stmt::Expr(local(target)));
        }
        for (advance, exit) in &self.loops {
            evidence.extend([Stmt::Expr(advance.clone()), Stmt::Expr(exit.clone())]);
        }
        let captures = free(
            &node(ExprKind::Block(evidence), Type::Unit, span),
            &[],
            &mut self.work,
        )?;
        let id = ir::FunctionId(self.identity(span)?);
        let entry = |increment: bool| {
            node(
                ExprKind::Closure {
                    function: id,
                    captures: captures
                        .iter()
                        .map(|p| {
                            if p.id == index.id && increment {
                                binary(BinaryOp::Add, local(p), integer(1, span), Type::Int)
                            } else {
                                local(p)
                            }
                        })
                        .collect(),
                },
                Type::Function(vec![], Box::new(Type::Int)),
                span,
            )
        };
        // Test before increment: inclusive i64::MAX completes without wrapping.
        let advance = node(
            ExprKind::If {
                condition: Box::new(binary(BinaryOp::Eq, local(&index), field(1), Type::Bool)),
                then_branch: Box::new(done.clone()),
                else_branch: Some(Box::new(operation(
                    Operation::Continue(Box::new(entry(true))),
                    span,
                ))),
            },
            Type::Int,
            span,
        );
        let continuation = Continuation {
            value: ir::Param {
                id: self.local(span)?,
                ty: Type::Unit,
            },
            entry: self.closure(advance.clone(), vec![], false)?,
        };
        self.loops.push((advance, done.clone()));
        let converted = self.expression(body, Some(&continuation), depth);
        self.loops.pop();
        let converted = converted?;
        let value = node(
            ExprKind::Actor(ir::ActorExpr::Lowered(Lowered {
                operation: Operation::IterateItem {
                    value: Box::new(local(&collection)),
                    index: Box::new(local(&index)),
                    item: item.clone(),
                },
            })),
            item,
            span,
        );
        let selected = node(
            ExprKind::Match {
                value: Box::new(value),
                arms: vec![MatchArm {
                    pattern: pattern.clone(),
                    guard: None,
                    body: converted,
                    span,
                }],
            },
            Type::Int,
            span,
        );
        let before = binary(BinaryOp::Lt, local(&index), field(1), Type::Bool);
        let last = binary(
            BinaryOp::And,
            binary(BinaryOp::Eq, local(&index), field(1), Type::Bool),
            binary(BinaryOp::Ne, field(2), integer(0, span), Type::Bool),
            Type::Bool,
        );
        let step_body = node(
            ExprKind::If {
                condition: Box::new(binary(BinaryOp::Or, before, last, Type::Bool)),
                then_branch: Box::new(selected),
                else_branch: Some(Box::new(done)),
            },
            Type::Int,
            span,
        );
        let mut step = self.function(step_body, vec![], false)?;
        self.plan.steps.remove(&step.id.0);
        self.plan.generic_steps.remove(&step.id.0);
        step.id = id;
        // Extra captures are permitted; every actual read must have a typed frame slot.
        for actual in &step.captures {
            if !captures
                .iter()
                .any(|p| p.id == actual.id && p.ty == actual.ty)
            {
                return Err(invalid(
                    span,
                    "actor loop frame is missing a lexical capture",
                ));
            }
        }
        step.captures = captures.clone();
        self.plan.steps.insert(id.0, self.mailbox.clone());
        if self.generic {
            self.plan.generic_steps.insert(id.0);
        }
        self.plan.functions.push(step);
        Ok(node(
            ExprKind::Block(vec![
                Stmt::Let {
                    id: collection.id,
                    value: iterable.clone(),
                },
                Stmt::Let {
                    id: index.id,
                    value: field(0),
                },
                Stmt::Expr(operation(Operation::Continue(Box::new(entry(false))), span)),
            ]),
            Type::Int,
            span,
        ))
    }
}

/// Preserve ordinary checked operator semantics in generated induction and guards.
pub(super) fn binary(op: BinaryOp, left: Expr, right: Expr, ty: Type) -> Expr {
    let span = left.span;
    node(
        ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        ty,
        span,
    )
}
