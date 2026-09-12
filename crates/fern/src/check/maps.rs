//! Immutable maps and record updates preserve contextual types and source evaluation order.
use super::{Checked, Checker, Diagnostic, HashSet, Span, Type, TypedKind, ast, ir};

/// Validate a key annotation, allowing a generic template only before specialization.
pub(super) fn validate_key(key: &Type, span: Span, generic: bool) -> Checked<()> {
    if super::scalar(key) || (generic && matches!(key, Type::Generic(_))) {
        Ok(())
    } else {
        Err(Diagnostic::new(
            span,
            "map key must be Int, Bool, or String",
        ))
    }
}

impl Checker<'_> {
    /// Infer homogeneous entries with outer context applied before checking callback values.
    pub(super) fn map(
        &mut self,
        entries: &[(ast::Expr, ast::Expr)],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let key = self.inference.fresh();
        let value = self.inference.fresh();
        let ty = Type::Map(Box::new(key.clone()), Box::new(value.clone()));
        self.constrain_result(&ty, expected, span)?;
        let directional = self.directional_context(Some(&value), span)?;
        let mut checked = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let k = self
                .expression_expected(k, Some(&key), depth)
                .map_err(|e| super::closures::context(e, "map key"))?;
            let v = if directional {
                self.expression_expected(v, Some(&value), depth)
            } else {
                self.expression_equal(v, &value, depth)
            }
            .map_err(|e| super::closures::context(e, "map value"))?;
            checked.push((k, v));
        }
        Ok((ir::ExprKind::Map(checked), ty))
    }

    /// Instantiate the public map APIs with independent key and value variables per use.
    pub(super) fn map_signature(&mut self, builtin: ir::Builtin) -> (Vec<Type>, Type) {
        use ir::Builtin::*;
        let key = self.inference.fresh();
        let value = self.inference.fresh();
        let map = Type::Map(Box::new(key.clone()), Box::new(value.clone()));
        match builtin {
            MapNew => (vec![], map),
            MapGet => (vec![map, key], Type::Option(Box::new(value))),
            MapPut => (vec![map.clone(), key, value], map),
            MapDelete => (vec![map.clone(), key], map),
            MapLen => (vec![map], Type::Int),
            MapIsEmpty => (vec![map], Type::Bool),
            MapContains => (vec![map, key], Type::Bool),
            MapKeys => (vec![map], Type::List(Box::new(key))),
            MapValues => (vec![map], Type::List(Box::new(value))),
            _ => unreachable!("map signature dispatch is exhaustive"),
        }
    }

    /// Finalize both halves so all nested callbacks and Result uses remain visible.
    pub(super) fn finalize_map(&self, entries: &mut [(ir::Expr, ir::Expr)]) -> Checked<()> {
        for (key, value) in entries {
            self.finalize(key)?;
            self.finalize(value)?;
        }
        Ok(())
    }

    /// Evaluate the base and written updates once, then rebuild declaration-ordered fields.
    pub(super) fn record_update(
        &mut self,
        base: &ast::Expr,
        fields: &[ast::RecordField],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let base_context = expected
            .map(|ty| self.inference.resolve(ty, span))
            .transpose()?;
        let base = self.expression_expected(
            base,
            base_context
                .as_ref()
                .filter(|ty| matches!(ty, Type::Named(..))),
            depth,
        )?;
        if base.ty == Type::Never {
            return Ok((base.kind, Type::Never));
        }
        if self.unknown_shape(&base.ty, span)? {
            return self.defer_update(base, fields, span, depth);
        }
        super::returns::shape_ready(&self.inference, &base.ty, span)?;
        let ty = self.inference.resolve(&base.ty, span)?;
        let layout = self.registry.layout(&ty, span)?;
        if layout.fields.is_empty() {
            return Err(Diagnostic::new(span, "record update requires a record"));
        }
        let indices = update_indices(fields, &layout.fields)?;
        let base_local = self.update_local(base);
        let mut statements = vec![base_local.0];
        let mut replacements = std::collections::HashMap::new();
        for (field, index) in fields.iter().zip(indices) {
            let value = self
                .expression_expected(&field.value, Some(&layout.variants[0][index]), depth)
                .map_err(|e| super::closures::context(e, "record field"))?;
            if value.ty == Type::Never {
                statements.push(ir::Stmt::Expr(value));
                return Ok((ir::ExprKind::Block(statements), Type::Never));
            }
            let (statement, local) = self.update_local(value);
            statements.push(statement);
            replacements.insert(index, local);
        }
        let values = layout.variants[0]
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                replacements.remove(&index).unwrap_or_else(|| ir::Expr {
                    kind: ir::ExprKind::Field {
                        value: Box::new(base_local.1.clone()),
                        index,
                    },
                    ty: ty.clone(),
                    span,
                })
            })
            .collect();
        statements.push(ir::Stmt::Expr(ir::Expr {
            kind: ir::ExprKind::CustomConstruct {
                tag: 0,
                fields: values,
            },
            ty: ty.clone(),
            span,
        }));
        Ok((ir::ExprKind::Block(statements), ty))
    }

    /// Allocate an inaccessible temporary while retaining a typed read of the stored value.
    fn update_local(&mut self, value: ir::Expr) -> (ir::Stmt, ir::Expr) {
        let id = self.bind("_", value.ty.clone());
        let local = ir::Expr {
            kind: ir::ExprKind::Local(id),
            ty: value.ty.clone(),
            span: value.span,
        };
        (ir::Stmt::Let { id, value }, local)
    }
}

/// Reject unknown and repeated names before lowering field initializers.
fn update_indices(fields: &[ast::RecordField], names: &[String]) -> Checked<Vec<usize>> {
    let mut seen = HashSet::new();
    fields
        .iter()
        .map(|field| {
            if !seen.insert(&field.name) {
                return Err(Diagnostic::new(
                    field.span,
                    format!("duplicate record field '{}'", field.name),
                ));
            }
            names
                .iter()
                .position(|name| name == &field.name)
                .ok_or_else(|| {
                    Diagnostic::new(field.span, format!("unknown record field '{}'", field.name))
                })
        })
        .collect()
}
