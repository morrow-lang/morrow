//! Typed list and tuple suffix patterns without sentinel element values.
use super::*;

impl Checker<'_> {
    /// Check sequence patterns against a concrete arity or a homogeneous element type.
    pub(super) fn sequence_pattern(
        &mut self,
        pattern: &ast::Pattern,
        ty: &Type,
        names: &mut HashSet<String>,
        depth: usize,
    ) -> Checked<ir::Pattern> {
        match &pattern.kind {
            ast::PatternKind::List { prefix, rest } => {
                let element = self.inference.fresh();
                let list = Type::List(Box::new(element.clone()));
                self.inference
                    .unify(ty, &list, pattern.span, "list pattern")?;
                let prefix = prefix
                    .iter()
                    .map(|p| self.pattern(p, &element, names, depth + 1))
                    .collect::<Checked<Vec<_>>>()?;
                let rest = rest
                    .as_ref()
                    .map(|p| self.sequence_rest(p, &list, names, depth + 1))
                    .transpose()?
                    .map(Box::new);
                Ok(ir::Pattern::List { prefix, rest })
            }
            ast::PatternKind::Tuple(fields) => {
                let types: Vec<_> = fields.iter().map(|_| self.inference.fresh()).collect();
                let expected = tuple_type(types.clone());
                self.inference
                    .unify(ty, &expected, pattern.span, "tuple pattern")?;
                let fields = fields
                    .iter()
                    .zip(types)
                    .map(|(p, t)| self.pattern(p, &t, names, depth + 1))
                    .collect::<Checked<Vec<_>>>()?;
                Ok(ir::Pattern::Tuple(fields))
            }
            ast::PatternKind::TupleRest { prefix, rest } => {
                let (types, tail) = if self.unknown_shape(ty, pattern.span)? {
                    self.defer_tuple(ty, prefix.len(), pattern.span)?
                } else {
                    returns::shape_ready(&self.inference, ty, pattern.span)?;
                    let ty = self.inference.resolve(ty, pattern.span)?;
                    let fields = tuple_fields(&ty, pattern.span)?;
                    if prefix.len() > fields.len() {
                        return Err(Diagnostic::new(
                            pattern.span,
                            "tuple pattern prefix exceeds tuple arity",
                        ));
                    }
                    (
                        fields[..prefix.len()].to_vec(),
                        tuple_type(fields[prefix.len()..].to_vec()),
                    )
                };
                let fields = prefix
                    .iter()
                    .zip(&types)
                    .map(|(p, t)| self.pattern(p, t, names, depth + 1))
                    .collect::<Checked<Vec<_>>>()?;
                let rest = self.sequence_rest(rest, &tail, names, depth + 1)?;
                Ok(ir::Pattern::TupleRest {
                    prefix: fields,
                    rest: Box::new(rest),
                })
            }
            _ => Err(Diagnostic::new(pattern.span, "invalid sequence pattern")),
        }
    }

    /// A suffix binds the complete remainder and cannot recursively destructure it.
    fn sequence_rest(
        &mut self,
        pattern: &ast::Pattern,
        ty: &Type,
        names: &mut HashSet<String>,
        depth: usize,
    ) -> Checked<ir::Pattern> {
        if !matches!(
            pattern.kind,
            ast::PatternKind::Bind(_) | ast::PatternKind::Wildcard
        ) {
            return Err(Diagnostic::new(
                pattern.span,
                "sequence rest must be a binding or wildcard",
            ));
        }
        self.pattern(pattern, ty, names, depth)
    }
}

/// Empty tuple suffixes are Unit; a one-element suffix retains its Tuple identity.
pub(super) fn tuple_type(fields: Vec<Type>) -> Type {
    if fields.is_empty() {
        Type::Unit
    } else {
        Type::Tuple(fields)
    }
}

/// Expose a fixed tuple arity while treating the empty tuple as Unit.
pub(super) fn tuple_fields(ty: &Type, span: Span) -> Checked<&[Type]> {
    match ty {
        Type::Unit => Ok(&[]),
        Type::Tuple(fields) => Ok(fields),
        _ => Err(Diagnostic::new(
            span,
            "tuple rest pattern requires a tuple type",
        )),
    }
}

/// Associate each sequence subpattern with its semantic payload type.
pub(super) fn parts<'a>(
    pattern: &'a ir::Pattern,
    ty: &Type,
    span: Span,
) -> Checked<Vec<(&'a ir::Pattern, Type)>> {
    match pattern {
        ir::Pattern::List { prefix, rest } => {
            let Type::List(element) = ty else {
                return Err(Diagnostic::new(span, "list pattern requires List type"));
            };
            let mut fields: Vec<_> = prefix.iter().map(|p| (p, *element.clone())).collect();
            if let Some(rest) = rest {
                fields.push((rest, ty.clone()));
            }
            Ok(fields)
        }
        ir::Pattern::TupleRest { prefix, rest } => {
            let types = tuple_fields(ty, span)?;
            if prefix.len() > types.len() {
                return Err(Diagnostic::new(
                    span,
                    "tuple pattern prefix exceeds tuple arity",
                ));
            }
            let mut fields: Vec<_> = prefix
                .iter()
                .zip(types)
                .map(|(p, t)| (p, t.clone()))
                .collect();
            fields.push((rest, tuple_type(types[prefix.len()..].to_vec())));
            Ok(fields)
        }
        _ => Err(Diagnostic::new(span, "invalid sequence pattern")),
    }
}

/// Lower irrefutable suffix bindings using an already evaluated subject local.
pub(super) fn destructure(
    pattern: &ir::Pattern,
    value: ir::Expr,
    statements: &mut Vec<ir::Stmt>,
    registry: &nominal::Registry,
    depth: usize,
) -> Checked<()> {
    match pattern {
        ir::Pattern::List {
            prefix,
            rest: Some(rest),
        } if prefix.is_empty() => {
            Checker::destructure_fields(rest, value, statements, registry, depth + 1)
        }
        ir::Pattern::TupleRest { prefix, rest } => {
            let types = tuple_fields(&value.ty, value.span)?;
            for (index, pattern) in prefix.iter().enumerate() {
                let field = projection(&value, index, types[index].clone());
                Checker::destructure_fields(pattern, field, statements, registry, depth + 1)?;
            }
            let fields: Vec<_> = types
                .iter()
                .enumerate()
                .skip(prefix.len())
                .map(|(index, ty)| projection(&value, index, ty.clone()))
                .collect();
            let ty = tuple_type(fields.iter().map(|f| f.ty.clone()).collect());
            let kind = if fields.is_empty() {
                ir::ExprKind::Unit
            } else {
                ir::ExprKind::Tuple(fields)
            };
            let tail = ir::Expr {
                kind,
                ty,
                span: value.span,
            };
            Checker::destructure_fields(rest, tail, statements, registry, depth + 1)
        }
        _ => Err(Diagnostic::new(
            value.span,
            "let sequence pattern must be irrefutable; use let-else or match",
        )),
    }
}

/// Build a field access to a saved subject without repeating source effects.
fn projection(value: &ir::Expr, index: usize, ty: Type) -> ir::Expr {
    ir::Expr {
        kind: ir::ExprKind::Field {
            value: Box::new(value.clone()),
            index,
        },
        ty,
        span: value.span,
    }
}

/// Enforce suffix discard obligations inside otherwise unrestricted match patterns.
pub(super) fn reject_discards(
    pattern: &ir::Pattern,
    ty: &Type,
    span: Span,
    registry: &nominal::Registry,
) -> Checked<()> {
    match pattern {
        ir::Pattern::Wildcard if matches!(ty, Type::Union(_)) => {
            control::pattern_discards(pattern, ty, span, registry)
        }
        ir::Pattern::Newtype(inner) => {
            control::pattern_discards(inner, &registry.newtype_inner(ty, span)?, span, registry)
        }
        ir::Pattern::UnionSelect { .. }
        | ir::Pattern::List { .. }
        | ir::Pattern::TupleRest { .. } => control::pattern_discards(pattern, ty, span, registry),
        ir::Pattern::Tuple(fields) if fields.is_empty() => Ok(()),
        ir::Pattern::Tuple(fields) | ir::Pattern::Variant { tag: 0, fields } => {
            let variants = registry.variants(ty, span)?;
            for (p, t) in fields.iter().zip(&variants[0]) {
                reject_discards(p, t, span, registry)?;
            }
            Ok(())
        }
        ir::Pattern::Variant { tag, fields } => {
            let variants = registry.variants(ty, span)?;
            for (p, t) in fields.iter().zip(&variants[*tag]) {
                reject_discards(p, t, span, registry)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
