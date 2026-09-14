//! One collection element and one callback return per logical actor transition.
use super::loops::binary;
use super::*;
use crate::{Constructor, ast::BinaryOp};

impl Builder<'_> {
    pub(super) fn higher_list(
        &mut self,
        builtin: Builtin,
        args: &[Expr],
        source: &Expr,
        next: Option<&Continuation>,
    ) -> Lowering<Expr> {
        let result = crate::lowering::higher_order::signature(builtin, args, source.span)?;
        expect_type(source.ty.clone(), result.clone(), source.span)?;
        let Type::List(item) = &args[0].ty else {
            return Err(invalid(source.span, "actor list callback requires List"));
        };
        let item = *item.clone();
        let Type::Function(_, mapped) = &args.last().unwrap().ty else {
            unreachable!("validated callback")
        };
        let mapped = *mapped.clone();
        let span = source.span;
        let mut prefix = Vec::new();
        let mut parameters = Vec::new();
        for argument in args {
            atomic(argument)?;
            let param = ir::Param {
                id: self.local(span)?,
                ty: argument.ty.clone(),
            };
            prefix.push(Stmt::Let {
                id: param.id,
                value: argument.clone(),
            });
            parameters.push(param);
        }
        let collection = parameters[0].clone();
        let callback = parameters.last().unwrap().clone();
        let index = ir::Param {
            id: self.local(span)?,
            ty: Type::Int,
        };
        let accumulator = ir::Param {
            id: self.local(span)?,
            ty: result.clone(),
        };
        let current = ir::Param {
            id: self.local(span)?,
            ty: item.clone(),
        };
        let returned = ir::Param {
            id: self.local(span)?,
            ty: mapped.clone(),
        };
        let local = |p: &ir::Param| node(ExprKind::Local(p.id), p.ty.clone(), span);
        let length = operation(
            Operation::IterateField {
                value: Box::new(local(&collection)),
                field: 1,
            },
            span,
        );
        let initial = match builtin {
            Builtin::ListMap | Builtin::ListFilter => {
                let Type::List(output) = &result else {
                    unreachable!("validated result")
                };
                let mut value = operation(
                    Operation::ListBuilder {
                        capacity: Box::new(binary(
                            BinaryOp::Add,
                            length.clone(),
                            integer(1, span),
                            Type::Int,
                        )),
                        item: *output.clone(),
                    },
                    span,
                );
                value.ty = result.clone();
                value
            }
            Builtin::ListFold => local(&parameters[1]),
            Builtin::ListFind => construct(Constructor::None, None, result.clone(), span),
            Builtin::ListAny | Builtin::ListAll => node(
                ExprKind::Bool(builtin == Builtin::ListAll),
                Type::Bool,
                span,
            ),
            _ => return Err(invalid(span, "unsupported actor list callback")),
        };
        let done = self.finish(local(&accumulator), next)?;
        let evidence = node(
            ExprKind::Block(vec![
                Stmt::Expr(done.clone()),
                Stmt::Expr(local(&collection)),
                Stmt::Expr(local(&callback)),
                Stmt::Expr(local(&index)),
                Stmt::Expr(local(&accumulator)),
            ]),
            Type::Int,
            span,
        );
        let captures = free(&evidence, &[], &mut self.work)?;
        let id = ir::FunctionId(self.identity(span)?);
        let frame = |advance: bool, value: Expr| {
            node(
                ExprKind::Closure {
                    function: id,
                    captures: captures
                        .iter()
                        .map(|p| {
                            if p.id == index.id && advance {
                                binary(BinaryOp::Add, local(p), integer(1, span), Type::Int)
                            } else if p.id == accumulator.id {
                                value.clone()
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
        let advance =
            |value: Expr| operation(Operation::Continue(Box::new(frame(true, value))), span);
        let append = |value: Expr| {
            let mut expr = operation(
                Operation::ListAppend {
                    list: Box::new(local(&accumulator)),
                    value: Box::new(value),
                },
                span,
            );
            expr.ty = result.clone();
            expr
        };
        let after = match builtin {
            Builtin::ListMap => advance(append(local(&returned))),
            Builtin::ListFold => advance(local(&returned)),
            Builtin::ListFilter => branch(
                local(&returned),
                advance(append(local(&current))),
                advance(local(&accumulator)),
                span,
            ),
            Builtin::ListFind => {
                let found = self.finish(
                    construct(
                        Constructor::Some,
                        Some(local(&current)),
                        result.clone(),
                        span,
                    ),
                    next,
                )?;
                branch(local(&returned), found, advance(local(&accumulator)), span)
            }
            Builtin::ListAny => {
                let found = self.finish(node(ExprKind::Bool(true), Type::Bool, span), next)?;
                branch(local(&returned), found, advance(local(&accumulator)), span)
            }
            Builtin::ListAll => {
                let found = self.finish(node(ExprKind::Bool(false), Type::Bool, span), next)?;
                branch(local(&returned), advance(local(&accumulator)), found, span)
            }
            _ => unreachable!("validated builtin"),
        };
        let continuation = Continuation {
            value: returned,
            entry: self.closure(after, vec![], false)?,
        };
        let mut call_args = Vec::new();
        if builtin == Builtin::ListFold {
            call_args.push(local(&accumulator));
        }
        call_args.push(local(&current));
        let invoke = node(
            ExprKind::Invoke {
                callee: Box::new(local(&callback)),
                args: call_args.clone(),
            },
            mapped,
            span,
        );
        let invoking =
            self.dynamic_call(&invoke, &local(&callback), &call_args, Some(&continuation))?;
        let mut indexed = operation(
            Operation::IterateItem {
                value: Box::new(local(&collection)),
                index: Box::new(local(&index)),
                item: item.clone(),
            },
            span,
        );
        indexed.ty = item;
        let selected = node(
            ExprKind::Block(vec![
                Stmt::Let {
                    id: current.id,
                    value: indexed,
                },
                Stmt::Expr(invoking),
            ]),
            Type::Int,
            span,
        );
        let body = branch(
            binary(BinaryOp::Lt, local(&index), length, Type::Bool),
            selected,
            done,
            span,
        );
        let mut step = self.function(body, vec![], false)?;
        self.plan.steps.remove(&step.id.0);
        self.plan.generic_steps.remove(&step.id.0);
        step.id = id;
        for actual in &step.captures {
            if !captures
                .iter()
                .any(|p| p.id == actual.id && p.ty == actual.ty)
            {
                return Err(invalid(
                    span,
                    "actor collection frame misses a lexical capture",
                ));
            }
        }
        let initial_frame = frame(false, local(&accumulator));
        step.captures = captures;
        self.plan.steps.insert(id.0, self.mailbox.clone());
        if self.generic {
            self.plan.generic_steps.insert(id.0);
        }
        self.plan.functions.push(step);
        prefix.push(Stmt::Let {
            id: index.id,
            value: integer(0, span),
        });
        prefix.push(Stmt::Let {
            id: accumulator.id,
            value: initial,
        });
        prefix.push(Stmt::Expr(operation(
            Operation::Continue(Box::new(initial_frame)),
            span,
        )));
        Ok(node(ExprKind::Block(prefix), Type::Int, span))
    }
}
fn construct(constructor: Constructor, value: Option<Expr>, ty: Type, span: Span) -> Expr {
    node(
        ExprKind::Construct {
            constructor,
            value: value.map(Box::new),
        },
        ty,
        span,
    )
}
fn branch(condition: Expr, yes: Expr, no: Expr, span: Span) -> Expr {
    node(
        ExprKind::If {
            condition: Box::new(condition),
            then_branch: Box::new(yes),
            else_branch: Some(Box::new(no)),
        },
        Type::Int,
        span,
    )
}
