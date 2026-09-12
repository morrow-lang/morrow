//! Derived sum fields retain native order and independent source wire names.
use super::*;
impl Planner<'_> {
    /// Charge every name and substituted payload before publishing a source constructor clause.
    pub(super) fn sum(
        &mut self,
        ty: &Type,
        decl: &ast::TypeDecl,
        span: Span,
        depth: usize,
    ) -> Checked<Kind> {
        self.layout_work(ty, decl, span)?;
        for variant in &decl.variants {
            let name = variant.name.rsplit('.').next().unwrap_or("");
            if !crate::json_codec::sums::source_name(name) {
                return Err(Diagnostic::new(
                    variant.span,
                    "invalid JSON source constructor name",
                ));
            }
        }
        let layout = self.registry.layout(ty, span)?;
        let mut variants = Vec::with_capacity(layout.variants.len());
        for (index, fields) in layout.variants.iter().enumerate() {
            let mut children = Vec::with_capacity(fields.len());
            for (field, ty) in fields.iter().enumerate() {
                children.push(self.plan(ty, decl.variants[index].fields[field].span, depth)?);
            }
            variants.push(plan::Variant {
                wire_tag: layout.variant_names[index].clone(),
                fields: children,
            });
        }
        Ok(Kind::Sum(variants))
    }
}
