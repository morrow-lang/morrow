//! Candidate J4 plan contract; private until executable lowering and boundary validation are ready.
use crate::Type;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Id(pub usize);
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Field {
    pub name: String,
    pub index: usize,
    pub codec: Id,
    pub optional: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Variant {
    pub wire_tag: String,
    pub fields: Vec<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Private reserved graph slot; completion is required before any proof or publication.
    Pending,
    Int,
    Float,
    Bool,
    String,
    Unit,
    Dynamic,
    List(Id),
    Newtype(Id),
    Option(Id),
    Tuple(Vec<Id>),
    Map(Id),
    Record(Vec<Field>),
    Sum(Vec<Variant>),
    Union(Vec<Id>),
    /// Only unused-derive validation uses this symbolic leaf; never publish in executable IR.
    Parameter,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub ty: Type,
    pub kind: Kind,
}
#[cfg(test)]
#[derive(Debug)]
pub(super) struct Plans {
    pub root: Id,
    pub entries: Vec<Plan>,
}
