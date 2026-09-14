//! Immutable sets are nominal map-backed values with explicit, deterministic set algebra.
//! Calls expand to ordinary typed IR so GC roots, callbacks and Result analysis stay shared.
use super::*;
use ir::{Builtin as B, Expr as E, ExprKind as K};

/// The complete public set API; arbitrary qualified names remain module errors.
pub(crate) const API_NAMES: &[&str] = &[
    "Set.new",
    "Set.insert",
    "Set.delete",
    "Set.contains",
    "Set.len",
    "Set.is_empty",
    "Set.to_list",
    "Set.from_list",
    "Set.union",
    "Set.intersection",
    "Set.difference",
    "Set.is_subset",
    "Set.equal",
];

/// Recognize only the public set API; arbitrary qualified names remain module errors.
pub(crate) fn is_api(name: &str) -> bool {
    API_NAMES.contains(&name)
}
fn set(item: Type) -> Type {
    Type::Named("Set".into(), vec![item])
}
fn map(item: Type) -> Type {
    Type::Map(Box::new(item), Box::new(Type::Unit))
}
fn expr(kind: K, ty: Type, span: Span) -> E {
    E { kind, ty, span }
}
fn call(builtin: B, args: Vec<E>, ty: Type, span: Span) -> E {
    expr(
        K::Call {
            target: ir::CallTarget::Builtin(builtin),
            args,
        },
        ty,
        span,
    )
}
fn unwrap(value: E, item: &Type) -> E {
    let span = value.span;
    expr(K::Unwrap(Box::new(value)), map(item.clone()), span)
}
fn wrap(value: E, item: &Type) -> E {
    let span = value.span;
    expr(K::Wrap(Box::new(value)), set(item.clone()), span)
}
impl Checker<'_> {
    /// Preserve left-before-right source effects when membership lowers to a map lookup.
    pub(super) fn set_membership(&mut self, key: E, value: E) -> Checked<TypedKind> {
        let span = value.span;
        let item = self.inference.fresh();
        self.inference
            .unify(&value.ty, &set(item.clone()), span, "set membership")?;
        self.inference
            .unify(&key.ty, &item, key.span, "set element")?;
        let key_local = self.set_param(key.ty.clone());
        let set_local = self.set_param(value.ty.clone());
        let lookup = call(
            B::MapContains,
            vec![
                unwrap(expr(K::Local(set_local.id), set_local.ty, span), &item),
                expr(K::Local(key_local.id), key_local.ty, span),
            ],
            Type::Bool,
            span,
        );
        Ok((
            K::Block(vec![
                ir::Stmt::Let {
                    id: key_local.id,
                    value: key,
                },
                ir::Stmt::Let {
                    id: set_local.id,
                    value,
                },
                ir::Stmt::Expr(lookup),
            ]),
            Type::Bool,
        ))
    }
    fn set_signature(&mut self, name: &str) -> (Type, Vec<Type>, Type) {
        let item = self.inference.fresh();
        let set = set(item.clone());
        let list = Type::List(Box::new(item.clone()));
        let (params, result) = match name {
            "Set.new" => (vec![], set),
            "Set.insert" | "Set.delete" => (vec![set.clone(), item.clone()], set),
            "Set.contains" => (vec![set, item.clone()], Type::Bool),
            "Set.len" => (vec![set], Type::Int),
            "Set.is_empty" => (vec![set], Type::Bool),
            "Set.to_list" => (vec![set], list),
            "Set.from_list" => (vec![list], set),
            "Set.is_subset" | "Set.equal" => (vec![set.clone(), set], Type::Bool),
            _ => (vec![set.clone(), set.clone()], set),
        };
        (item, params, result)
    }
    /// Apply result context before checking arguments, preserving empty/generic set inference.
    pub(super) fn set_call(
        &mut self,
        name: &str,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        labels::positional(args)?;
        let (item, params, result) = self.set_signature(name);
        self.constrain_result(&result, expected, span)?;
        let args = self.call_arguments(args, &params, span, depth)?;
        if let Some(index) = args.iter().position(|arg| arg.ty == Type::Never) {
            return Ok((
                K::Block(
                    args.into_iter()
                        .take(index + 1)
                        .map(ir::Stmt::Expr)
                        .collect(),
                ),
                Type::Never,
            ));
        }
        let value = self.set_body(name, args, &item, span);
        Ok((value.kind, value.ty))
    }
    /// First-class set APIs use the ordinary lifted closure ABI and contextual specialization.
    pub(super) fn set_function(&mut self, name: &str, span: Span) -> Checked<TypedKind> {
        let (item, types, result) = self.set_signature(name);
        let params: Vec<_> = types.iter().map(|ty| self.set_param(ty.clone())).collect();
        let args = params
            .iter()
            .map(|p| expr(K::Local(p.id), p.ty.clone(), span))
            .collect();
        let body = self.set_body(name, args, &item, span);
        Ok((
            K::Lambda {
                params,
                captures: vec![],
                body: Box::new(body),
                local_count: self.local_count,
            },
            Type::Function(types, Box::new(result)),
        ))
    }
    fn set_param(&mut self, ty: Type) -> ir::Param {
        let id = ir::LocalId(self.local_count);
        self.local_count += 1;
        ir::Param { id, ty }
    }
    /// Bind each argument once before algebra reuses either operand in captures or comparisons.
    fn set_body(&mut self, name: &str, args: Vec<E>, item: &Type, span: Span) -> E {
        let mut statements = Vec::new();
        let args: Vec<_> = args
            .into_iter()
            .map(|value| {
                let param = self.set_param(value.ty.clone());
                statements.push(ir::Stmt::Let {
                    id: param.id,
                    value,
                });
                expr(K::Local(param.id), param.ty, span)
            })
            .collect();
        let map_type = map(item.clone());
        let list_type = Type::List(Box::new(item.clone()));
        let value = match name {
            "Set.new" => wrap(call(B::MapNew, vec![], map_type, span), item),
            "Set.from_list" => {
                let empty = call(B::MapNew, vec![], map_type, span);
                wrap(self.set_collect(args[0].clone(), empty, item, span), item)
            }
            "Set.insert" | "Set.delete" => {
                let mut values = vec![unwrap(args[0].clone(), item), args[1].clone()];
                let builtin = if name == "Set.insert" {
                    values.push(expr(K::Unit, Type::Unit, span));
                    B::MapPut
                } else {
                    B::MapDelete
                };
                wrap(call(builtin, values, map_type, span), item)
            }
            "Set.contains" => call(
                B::MapContains,
                vec![unwrap(args[0].clone(), item), args[1].clone()],
                Type::Bool,
                span,
            ),
            "Set.len" => call(
                B::MapLen,
                vec![unwrap(args[0].clone(), item)],
                Type::Int,
                span,
            ),
            "Set.is_empty" => call(
                B::MapIsEmpty,
                vec![unwrap(args[0].clone(), item)],
                Type::Bool,
                span,
            ),
            "Set.to_list" => call(
                B::MapKeys,
                vec![unwrap(args[0].clone(), item)],
                list_type,
                span,
            ),
            "Set.union" => {
                let keys = call(
                    B::MapKeys,
                    vec![unwrap(args[1].clone(), item)],
                    list_type,
                    span,
                );
                wrap(
                    self.set_collect(keys, unwrap(args[0].clone(), item), item, span),
                    item,
                )
            }
            _ => self.set_algebra(name, &args, item, span),
        };
        let ty = value.ty.clone();
        statements.push(ir::Stmt::Expr(value));
        expr(K::Block(statements), ty, span)
    }
    /// Fold a finite list into immutable map storage; duplicate insertions retain first position.
    fn set_collect(&mut self, keys: E, initial: E, item: &Type, span: Span) -> E {
        let map_type = map(item.clone());
        let acc = self.set_param(map_type.clone());
        let key = self.set_param(item.clone());
        let body = call(
            B::MapPut,
            vec![
                expr(K::Local(acc.id), acc.ty.clone(), span),
                expr(K::Local(key.id), key.ty.clone(), span),
                expr(K::Unit, Type::Unit, span),
            ],
            map_type.clone(),
            span,
        );
        let callback_type = Type::Function(
            vec![acc.ty.clone(), key.ty.clone()],
            Box::new(map_type.clone()),
        );
        let callback = expr(
            K::Lambda {
                params: vec![acc, key],
                captures: vec![],
                body: Box::new(body),
                local_count: self.local_count,
            },
            callback_type,
            span,
        );
        call(B::ListFold, vec![keys, initial, callback], map_type, span)
    }
    /// Capture the already evaluated right operand without exposing nominal storage to source.
    fn set_algebra(&mut self, name: &str, args: &[E], item: &Type, span: Span) -> E {
        let list_type = Type::List(Box::new(item.clone()));
        let left = unwrap(args[0].clone(), item);
        let right = unwrap(args[1].clone(), item);
        let keys = call(B::MapKeys, vec![left.clone()], list_type.clone(), span);
        let key = self.set_param(item.clone());
        let captured = self.set_param(right.ty.clone());
        let mut body = call(
            B::MapContains,
            vec![
                expr(K::Local(captured.id), captured.ty.clone(), span),
                expr(K::Local(key.id), item.clone(), span),
            ],
            Type::Bool,
            span,
        );
        if name == "Set.difference" {
            body = expr(
                K::Unary {
                    op: ast::UnaryOp::Not,
                    value: Box::new(body),
                },
                Type::Bool,
                span,
            );
        }
        let callback = expr(
            K::Lambda {
                params: vec![key],
                captures: vec![ir::Capture {
                    param: captured,
                    value: right.clone(),
                }],
                body: Box::new(body),
                local_count: self.local_count,
            },
            Type::Function(vec![item.clone()], Box::new(Type::Bool)),
            span,
        );
        if matches!(name, "Set.is_subset" | "Set.equal") {
            let subset = call(B::ListAll, vec![keys, callback], Type::Bool, span);
            if name == "Set.is_subset" {
                return subset;
            }
            let same_len = expr(
                K::Binary {
                    op: ast::BinaryOp::Eq,
                    left: Box::new(call(B::MapLen, vec![left], Type::Int, span)),
                    right: Box::new(call(B::MapLen, vec![right], Type::Int, span)),
                },
                Type::Bool,
                span,
            );
            return expr(
                K::Binary {
                    op: ast::BinaryOp::And,
                    left: Box::new(same_len),
                    right: Box::new(subset),
                },
                Type::Bool,
                span,
            );
        }
        let selected = call(B::ListFilter, vec![keys, callback], list_type, span);
        let empty = call(B::MapNew, vec![], map(item.clone()), span);
        wrap(self.set_collect(selected, empty, item, span), item)
    }
}
