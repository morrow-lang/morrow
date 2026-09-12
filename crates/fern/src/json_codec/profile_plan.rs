//! Public concrete plans have no unknown profile leaves or unvalidated member identities.
use super::*;
use profiles::{Key, Node, Shape};
impl Plan {
    /// Build borrowed-name profiles after the complete plan/layout audit, with no type witnesses.
    pub(super) fn profiles(&self, audit: &mut Validation) -> Result<(), Diagnostic> {
        audit.charge(self.entries.len())?;
        let mut nodes = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let shape = match &entry.kind {
                Kind::Int | Kind::Float => Shape::Number,
                Kind::Bool => Shape::Bool,
                Kind::String => Shape::String,
                Kind::Unit => Shape::Null,
                Kind::Dynamic => Shape::Any,
                Kind::List(_) => Shape::Array(None),
                Kind::Tuple(fields) => Shape::Array(Some(fields.len())),
                Kind::Map(_) => Shape::Map,
                Kind::Record(fields) => {
                    audit.charge(fields.len())?;
                    Shape::Object(
                        fields
                            .iter()
                            .map(|f| Key {
                                name: &f.name,
                                required: !f.optional,
                            })
                            .collect(),
                    )
                }
                Kind::Sum(variants) => {
                    audit.charge(variants.len())?;
                    Shape::Sum(variants.iter().map(|v| v.wire_tag.as_str()).collect())
                }
                Kind::Newtype(id) | Kind::Option(id) => {
                    audit.charge(1)?;
                    nodes.push(Node::follow(
                        vec![*id],
                        matches!(entry.kind, Kind::Option(_)),
                        false,
                    ));
                    continue;
                }
                Kind::Union(ids) => {
                    audit.charge(ids.len())?;
                    nodes.push(Node::follow(ids.clone(), false, true));
                    continue;
                }
            };
            nodes.push(Node::leaf(shape));
        }
        profiles::validate(&nodes, &mut audit.work, audit.span)
    }
    /// Source union canonical order and exact child identity are independently rechecked.
    pub(super) fn union_members(
        &self,
        index: usize,
        ids: &[usize],
        types: &[Type],
        audit: &mut Validation,
    ) -> Result<bool, Diagnostic> {
        if ids.len() != types.len() || ids.len() < 2 || ids.len() > 128 {
            return Ok(false);
        }
        for (at, (id, ty)) in ids.iter().zip(types).enumerate() {
            audit.charge(type_size(ty))?;
            if matches!(ty, Type::Union(_))
                || (at > 0 && types[at - 1] >= *ty)
                || self.child(*id, index, audit)?.ty != *ty
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
