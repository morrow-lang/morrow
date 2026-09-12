//! Charged shallow wire profiles: no value conversion, candidate allocation or priority.
use crate::{Diagnostic, Span};
mod compare;
use std::collections::VecDeque;
#[derive(Clone, Copy)]
pub(crate) struct Key<'a> {
    pub name: &'a str,
    pub required: bool,
}
pub(crate) enum Shape<'a> {
    Number,
    Bool,
    String,
    Null,
    Any,
    Array(Option<usize>),
    Object(Vec<Key<'a>>),
    Map,
    Sum(Vec<&'a str>),
    Unknown,
    Follow,
}
pub(crate) struct Node<'a> {
    pub shape: Shape<'a>,
    pub links: Vec<usize>,
    pub option: bool,
    pub union: bool,
}
impl<'a> Node<'a> {
    pub(crate) fn leaf(shape: Shape<'a>) -> Self {
        Self {
            shape,
            links: Vec::new(),
            option: false,
            union: false,
        }
    }
    pub(crate) fn follow(links: Vec<usize>, option: bool, union: bool) -> Self {
        Self {
            shape: Shape::Follow,
            links,
            option,
            union,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Atom {
    Null,
    Leaf(usize),
}
struct Proof<'a, 'n> {
    nodes: &'n [Node<'a>],
    profiles: Vec<Vec<Atom>>,
    work: &'n mut usize,
    span: Span,
}
/// Include every inactive node and spend the caller's original aggregate proof allowance.
pub(crate) fn validate(nodes: &[Node<'_>], work: &mut usize, span: Span) -> Result<(), Diagnostic> {
    charge(work, nodes.len().saturating_mul(4), span)?;
    let mut proof = Proof {
        nodes,
        profiles: vec![Vec::new(); nodes.len()],
        work,
        span,
    };
    proof.propagate()?;
    for (id, node) in nodes.iter().enumerate() {
        if proof.profiles[id].is_empty() {
            return Err(Diagnostic::new(
                span,
                "recursive JSON profile has no finite discriminator",
            ));
        }
        charge(proof.work, 1, proof.span)?;
        if node.option {
            charge(proof.work, proof.profiles[node.links[0]].len(), proof.span)?;
        }
        if node.option
            && proof.profiles[node.links[0]]
                .iter()
                .any(|a| proof.nullable(*a))
        {
            return Err(Diagnostic::new(
                span,
                "Option payload can encode null; transparent JSON Option would be ambiguous",
            ));
        }
        if node.union {
            proof.union(&node.links)?;
        }
    }
    Ok(())
}
impl Proof<'_, '_> {
    /// A least fixed point follows transparent edges; cached paths never skip siblings.
    fn propagate(&mut self) -> Result<(), Diagnostic> {
        let mut reverse = vec![Vec::new(); self.nodes.len()];
        let mut queue = VecDeque::new();
        for (id, node) in self.nodes.iter().enumerate() {
            for child in &node.links {
                charge(self.work, 2, self.span)?;
                let parents = reverse
                    .get_mut(*child)
                    .ok_or_else(|| Diagnostic::new(self.span, "invalid JSON profile child"))?;
                parents.push(id);
            }
            let atom = if node.option {
                Some(Atom::Null)
            } else if !matches!(node.shape, Shape::Follow) {
                Some(Atom::Leaf(id))
            } else {
                None
            };
            if let Some(atom) = atom {
                charge(self.work, 2, self.span)?;
                self.profiles[id].push(atom);
                queue.push_back((id, atom));
            }
        }
        while let Some((id, atom)) = queue.pop_front() {
            charge(self.work, reverse[id].len() + 1, self.span)?;
            for parent in &reverse[id] {
                charge(self.work, self.profiles[*parent].len() + 1, self.span)?;
                if !self.profiles[*parent].contains(&atom) {
                    charge(self.work, 2, self.span)?;
                    self.profiles[*parent].push(atom);
                    queue.push_back((*parent, atom));
                }
            }
        }
        Ok(())
    }
    fn nullable(&self, atom: Atom) -> bool {
        match atom {
            Atom::Null => true,
            Atom::Leaf(id) => matches!(self.nodes[id].shape, Shape::Null | Shape::Any),
        }
    }
    /// Only proved disjoint pairs pass; unknown symbolic leaves remain conditional obligations.
    fn union(&mut self, children: &[usize]) -> Result<(), Diagnostic> {
        for (at, left) in children.iter().enumerate() {
            for right in &children[at + 1..] {
                charge(self.work, 1, self.span)?;
                for a in &self.profiles[*left] {
                    for b in &self.profiles[*right] {
                        charge(self.work, 1, self.span)?;
                        let a = match a {
                            Atom::Null => &Shape::Null,
                            Atom::Leaf(id) => &self.nodes[*id].shape,
                        };
                        let b = match b {
                            Atom::Null => &Shape::Null,
                            Atom::Leaf(id) => &self.nodes[*id].shape,
                        };
                        if !compare::disjoint(a, b, self.work, self.span)? {
                            return Err(Diagnostic::new(
                                self.span,
                                "JSON union alternatives are not provably disjoint; use a derived sum with distinct constructors",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
/// Charge before storing, copying or comparing profile metadata; no helper owns a fresh budget.
pub(crate) fn charge(work: &mut usize, amount: usize, span: Span) -> Result<(), Diagnostic> {
    *work = work.saturating_add(amount);
    if *work > super::MAX_WORK {
        return Err(Diagnostic::new(
            span,
            "JSON codec profile work limit exceeded",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
