//! Symbolic derivations use the same shallow proof without publishing parameter witnesses.
use super::*;
use crate::json_codec::profiles::{self, Key, Node, Shape};
/// Borrow all source names after precharging every temporary profile vector.
pub(super) fn validate(entries: &[Plan], work: &mut usize, span: Span) -> Checked<()> {
    profiles::charge(work, entries.len(), span)?;
    let mut nodes = Vec::with_capacity(entries.len());
    for entry in entries {
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
                profiles::charge(work, fields.len(), span)?;
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
                profiles::charge(work, variants.len(), span)?;
                Shape::Sum(variants.iter().map(|v| v.wire_tag.as_str()).collect())
            }
            Kind::Newtype(id) | Kind::Option(id) => {
                profiles::charge(work, 1, span)?;
                nodes.push(Node::follow(
                    vec![id.0],
                    matches!(entry.kind, Kind::Option(_)),
                    false,
                ));
                continue;
            }
            Kind::Union(ids) => {
                profiles::charge(work, ids.len(), span)?;
                nodes.push(Node::follow(
                    ids.iter().map(|id| id.0).collect(),
                    false,
                    true,
                ));
                continue;
            }
            Kind::Parameter => Shape::Unknown,
            Kind::Pending => return Err(Diagnostic::new(span, "incomplete JSON codec profile")),
        };
        nodes.push(Node::leaf(shape));
    }
    profiles::validate(&nodes, work, span)
}
