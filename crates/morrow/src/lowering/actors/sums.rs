//! Sum callbacks use the same explicit return frames as ordinary actor calls.
use super::*;
use crate::Constructor;

impl Builder<'_> {
    /// Both arguments are eager; only the selected payload invokes the callback.
    pub(super) fn normalize_sum(&mut self, source: &Expr, depth: usize) -> Lowering<Option<Expr>> {
        let ExprKind::Call {
            target: ir::CallTarget::Builtin(builtin),
            args,
        } = &source.kind
        else {
            return Ok(None);
        };
        if !matches!(
            builtin,
            Builtin::OptionMap
                | Builtin::ResultMap
                | Builtin::ResultAndThen
                | Builtin::ResultUnwrapOrElse
        ) {
            return Ok(None);
        }
        let result = crate::lowering::higher_order::signature(*builtin, args, source.span)?;
        expect_type(result.clone(), source.ty.clone(), source.span)?;
        let Type::Function(_, returned) = &args[1].ty else {
            unreachable!("validated callback");
        };
        let collection = self.local(source.span)?;
        let callback = self.local(source.span)?;
        let success = self.local(source.span)?;
        let failure = self.local(source.span)?;
        let (ok_type, error_type, ok_constructor, error_constructor) = match &args[0].ty {
            Type::Option(item) => (
                *item.clone(),
                Type::Unit,
                Constructor::Some,
                Constructor::None,
            ),
            Type::Result(item, error) => (
                *item.clone(),
                *error.clone(),
                Constructor::Ok,
                Constructor::Err,
            ),
            _ => unreachable!("validated sum"),
        };
        let invoke = |id, ty| {
            node(
                ExprKind::Invoke {
                    callee: Box::new(node(
                        ExprKind::Local(callback),
                        args[1].ty.clone(),
                        source.span,
                    )),
                    args: vec![node(ExprKind::Local(id), ty, source.span)],
                },
                *returned.clone(),
                source.span,
            )
        };
        let wrap = |constructor, value: Option<Expr>| {
            node(
                ExprKind::Construct {
                    constructor,
                    value: value.map(Box::new),
                },
                result.clone(),
                source.span,
            )
        };
        let ok = match builtin {
            Builtin::ResultUnwrapOrElse => node(ExprKind::Local(success), ok_type, source.span),
            Builtin::ResultAndThen => invoke(success, ok_type),
            _ => wrap(ok_constructor, Some(invoke(success, ok_type))),
        };
        let err = match builtin {
            Builtin::ResultUnwrapOrElse => invoke(failure, error_type),
            Builtin::OptionMap => wrap(Constructor::None, None),
            _ => wrap(
                Constructor::Err,
                Some(node(ExprKind::Local(failure), error_type, source.span)),
            ),
        };
        let matching = node(
            ExprKind::Match {
                value: Box::new(node(
                    ExprKind::Local(collection),
                    args[0].ty.clone(),
                    source.span,
                )),
                arms: vec![
                    MatchArm {
                        pattern: Pattern::Constructor {
                            constructor: ok_constructor,
                            binding: Some(success),
                        },
                        guard: None,
                        body: ok,
                        span: source.span,
                    },
                    MatchArm {
                        pattern: Pattern::Constructor {
                            constructor: error_constructor,
                            binding: (error_constructor != Constructor::None).then_some(failure),
                        },
                        guard: None,
                        body: err,
                        span: source.span,
                    },
                ],
            },
            result.clone(),
            source.span,
        );
        let expanded = node(
            ExprKind::Block(vec![
                Stmt::Let {
                    id: collection,
                    value: args[0].clone(),
                },
                Stmt::Let {
                    id: callback,
                    value: args[1].clone(),
                },
                Stmt::Expr(matching),
            ]),
            result,
            source.span,
        );
        self.normalize(&expanded, depth + 1).map(Some)
    }
}
