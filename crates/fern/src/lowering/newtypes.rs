//! Zero-cost nominal wrappers retain type identity while using their payload ABI.
use super::*;

/// Resolve only unboxed nominal layers; pointer-backed recursive payloads terminate the walk.
pub(super) fn representation<'t>(
    mut ty: &'t Type,
    layouts: &'t HashMap<Type, &ir::TypeLayout>,
    span: Span,
) -> Lowering<&'t Type> {
    for _ in 0..=MAX_DEPTH {
        let Type::Named(_, _) = ty else {
            return Ok(ty);
        };
        let layout = layouts
            .get(ty)
            .ok_or_else(|| invalid(span, "missing concrete nominal layout"))?;
        if layout.storage == ir::LayoutStorage::Tagged {
            return Ok(ty);
        }
        ty = payload(layout, span)?;
    }
    Err(invalid(
        span,
        "recursive or excessively nested unboxed layout",
    ))
}

/// Validate a newtype's unique payload without accepting record metadata or extra variants.
pub(super) fn payload(layout: &ir::TypeLayout, span: Span) -> Lowering<&Type> {
    if layout.storage != ir::LayoutStorage::Unboxed
        || !layout.fields.is_empty()
        || !layout.variant_names.is_empty()
        || layout.variants.len() != 1
        || layout.variants[0].len() != 1
    {
        return Err(invalid(
            span,
            "newtype requires one unboxed payload and no record fields",
        ));
    }
    Ok(&layout.variants[0][0])
}

impl Emitter<'_> {
    /// Select the scalar ABI after all nominal layouts and concrete references were validated.
    pub(super) fn representation<'t>(&'t self, ty: &'t Type) -> &'t Type {
        representation(ty, &self.layouts, Span::default()).expect("validated nominal ABI")
    }

    /// Return QBE width of the ultimate payload while preserving semantic types elsewhere.
    pub(super) fn width(&self, ty: Type) -> char {
        scalar_width(self.representation(&ty).clone())
    }

    /// Require one specific newtype layer rather than erasing nominal argument equality.
    pub(super) fn newtype_payload(&self, ty: &Type, span: Span) -> Lowering<Type> {
        let layout = self
            .layouts
            .get(ty)
            .ok_or_else(|| invalid(span, "newtype requires concrete nominal layout"))?;
        Ok(payload(layout, span)?.clone())
    }

    /// Validate Wrap/Unwrap types before evaluating the child once; emit no allocation or conversion.
    pub(super) fn newtype_expr(
        &mut self,
        expr: &Expr,
        value: &Expr,
        wrap: bool,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let (outer, inner) = if wrap {
            (&expr.ty, &value.ty)
        } else {
            (&value.ty, &expr.ty)
        };
        let expected = self.newtype_payload(outer, expr.span)?;
        expect_type(inner.clone(), expected, expr.span)?;
        let value = self.expr(value, locals, depth)?;
        Ok((expr.ty.clone(), value))
    }
}
