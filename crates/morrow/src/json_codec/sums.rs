//! Sum metadata is checked against independently retained source constructor identities.
use super::*;
impl Plan {
    /// Validate native tag order and every child reference, including unused alternatives.
    pub(super) fn sum(
        &self,
        index: usize,
        variants: &[Variant],
        entry: &Entry,
        layouts: &HashMap<&Type, &ir::TypeLayout>,
        audit: &mut Validation,
    ) -> Result<bool, Diagnostic> {
        let layout = layouts
            .get(&entry.ty)
            .ok_or_else(|| audit.error("JSON sum codec is missing its checked layout"))?;
        if variants.is_empty()
            || variants.len() > 255
            || layout.storage != ir::LayoutStorage::Tagged
            || !layout.fields.is_empty()
            || layout.variants.len() != variants.len()
            || layout.variant_names.len() != variants.len()
        {
            return Ok(false);
        }
        let mut names = std::collections::HashSet::new();
        for (tag, variant) in variants.iter().enumerate() {
            audit.charge(variant.wire_tag.len())?;
            if !source_name(&variant.wire_tag)
                || !names.insert(&variant.wire_tag)
                || variant.wire_tag != layout.variant_names[tag]
                || variant.fields.len() != layout.variants[tag].len()
            {
                return Ok(false);
            }
            for (id, ty) in variant.fields.iter().zip(&layout.variants[tag]) {
                if self.child(*id, index, audit)?.ty != *ty {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
/// Unqualified constructor tokens cannot contain whitespace, punctuation or embedded terminators.
pub(crate) fn source_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    crate::parse::identifier_char(first, true)
        && chars.all(|c| crate::parse::identifier_char(c, false))
}
