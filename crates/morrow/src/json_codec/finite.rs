//! A charged least fixed point proves finite values without recursive graph walks.
use crate::{Diagnostic, Span};

pub(crate) struct Proof<'a> {
    reverse: Vec<Vec<usize>>,
    remaining: Vec<usize>,
    disjunction: Vec<bool>,
    required: usize,
    work: &'a mut usize,
    span: Span,
}
impl<'a> Proof<'a> {
    /// Reserve bounded graph storage before creating per-entry slots.
    pub(crate) fn new(count: usize, work: &'a mut usize, span: Span) -> Result<Self, Diagnostic> {
        charge(work, count.saturating_mul(4), span)?;
        if count > super::MAX_ENTRIES {
            return Err(Diagnostic::new(
                span,
                "JSON codec plan count limit exceeded",
            ));
        }
        Ok(Self {
            reverse: vec![Vec::new(); count],
            remaining: vec![0; count],
            disjunction: vec![false; count],
            required: count,
            work,
            span,
        })
    }
    /// Strict constructors require every child; List, Map and Option supply independent bases.
    pub(crate) fn edge(&mut self, parent: usize, child: usize) -> Result<(), Diagnostic> {
        charge(self.work, 2, self.span)?;
        if parent >= self.remaining.len() || child >= self.remaining.len() {
            return Err(Diagnostic::new(self.span, "invalid JSON codec child slot"));
        }
        self.remaining[parent] += 1;
        self.reverse[child].push(parent);
        Ok(())
    }
    /// A sum has no implicit empty base; one explicit constructor product must complete.
    pub(crate) fn disjunction(&mut self, id: usize) -> Result<(), Diagnostic> {
        charge(self.work, 1, self.span)?;
        if id >= self.required || self.remaining[id] != 0 {
            return Err(Diagnostic::new(self.span, "invalid JSON sum proof vertex"));
        }
        self.disjunction[id] = true;
        self.remaining[id] = 1;
        Ok(())
    }
    /// Auxiliary product clauses are proof metadata, never fabricated executable plan entries.
    pub(crate) fn alternative(&mut self, parent: usize) -> Result<usize, Diagnostic> {
        charge(self.work, 6, self.span)?;
        if parent >= self.required || !self.disjunction[parent] {
            return Err(Diagnostic::new(self.span, "invalid JSON sum proof parent"));
        }
        let id = self.remaining.len();
        self.remaining.push(0);
        self.disjunction.push(false);
        self.reverse.push(vec![parent]);
        Ok(id)
    }
    /// Each ready vertex and dependency is visited once; OR completion never double-enqueues.
    pub(crate) fn finish(mut self) -> Result<(), Diagnostic> {
        charge(self.work, self.remaining.len(), self.span)?;
        let mut ready: Vec<_> = self
            .remaining
            .iter()
            .enumerate()
            .filter_map(|(id, count)| (*count == 0).then_some(id))
            .collect();
        while let Some(id) = ready.pop() {
            charge(
                self.work,
                self.reverse[id].len().saturating_add(1),
                self.span,
            )?;
            for parent in &self.reverse[id] {
                if self.remaining[*parent] == 0 {
                    continue;
                }
                if self.disjunction[*parent] {
                    self.remaining[*parent] = 0;
                } else {
                    self.remaining[*parent] -= 1;
                }
                if self.remaining[*parent] == 0 {
                    ready.push(*parent);
                }
            }
        }
        if self.remaining[..self.required].iter().any(|n| *n != 0) {
            return Err(Diagnostic::new(
                self.span,
                "recursive JSON schema has no finite value",
            ));
        }
        Ok(())
    }
}
/// All proof storage, edges and scans spend the caller's existing aggregate allowance.
fn charge(work: &mut usize, amount: usize, span: Span) -> Result<(), Diagnostic> {
    *work = work.saturating_add(amount);
    if *work > super::MAX_WORK {
        return Err(Diagnostic::new(span, "JSON codec plan work limit exceeded"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_four_vertex_strict_graphs_match_independent_fixed_point() {
        for bits in 0u32..65536 {
            let mut dependencies = [0u32; 4];
            let mut work = 0;
            let mut proof = Proof::new(4, &mut work, Span::default()).unwrap();
            for (parent, row) in dependencies.iter_mut().enumerate() {
                *row = (bits >> (parent * 4)) & 15;
                for child in 0..4 {
                    if *row & (1 << child) != 0 {
                        proof.edge(parent, child).unwrap();
                    }
                }
            }
            let mut known = 0u32;
            for _ in 0..4 {
                let mut next = known;
                for (id, row) in dependencies.iter().enumerate() {
                    if row & !known == 0 {
                        next |= 1 << id;
                    }
                }
                known = next;
            }
            assert_eq!(proof.finish().is_ok(), known == 15, "graph {bits}");
        }
    }
    #[test]
    fn finite_sum_products_match_independent_least_fixed_point() {
        let mut seed = 71u64;
        for _ in 0..16384 {
            let mut equations = [[0u8; 2]; 4];
            let mut work = 0;
            let mut proof = Proof::new(4, &mut work, Span::default()).unwrap();
            for (id, variants) in equations.iter_mut().enumerate() {
                proof.disjunction(id).unwrap();
                for fields in variants {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    *fields = (seed >> 32) as u8 & 15;
                    let product = proof.alternative(id).unwrap();
                    for child in 0..4 {
                        if *fields & (1 << child) != 0 {
                            proof.edge(product, child).unwrap();
                        }
                    }
                }
            }
            let mut known = 0u8;
            for _ in 0..4 {
                let mut next = known;
                for (id, variants) in equations.iter().enumerate() {
                    if variants.iter().any(|fields| fields & !known == 0) {
                        next |= 1 << id;
                    }
                }
                known = next;
            }
            assert_eq!(proof.finish().is_ok(), known == 15, "{equations:?}");
        }
    }
    #[test]
    fn alternatives_reserve_storage_before_push_and_empty_sums_have_no_base() {
        let mut work = super::super::MAX_WORK - 10;
        let mut proof = Proof::new(1, &mut work, Span::default()).unwrap();
        proof.disjunction(0).unwrap();
        assert!(proof.alternative(0).is_err());
        assert_eq!(proof.remaining.len(), 1);
        let mut work = 0;
        let mut proof = Proof::new(1, &mut work, Span::default()).unwrap();
        proof.disjunction(0).unwrap();
        assert!(proof.finish().is_err());
    }
    #[test]
    fn proof_reserves_storage_and_edges_from_one_existing_allowance() {
        let mut work = super::super::MAX_WORK - 2;
        assert!(Proof::new(1, &mut work, Span::default()).is_err());
        let mut work = super::super::MAX_WORK - 5;
        let mut proof = Proof::new(1, &mut work, Span::default()).unwrap();
        assert!(proof.edge(0, 0).is_err());
        assert!(proof.reverse[0].is_empty());
    }
}
