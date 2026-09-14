//! Bounded canonical path predicates; no full-path enumeration and no MAY-to-MUST conversion.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct Predicate(pub usize);
impl Predicate {
    pub const FALSE: Self = Self(0);
    pub const TRUE: Self = Self(1);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Branch {
    variable: usize,
    low: Predicate,
    high: Predicate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Operation {
    And,
    Or,
    Xor,
}
#[derive(Debug, Default)]
pub(super) struct Predicates {
    nodes: Vec<Branch>,
    intern: HashMap<Branch, Predicate>,
    cache: HashMap<(Operation, Predicate, Predicate), Predicate>,
    variables: usize,
}
impl Predicates {
    /// Report allocated canonical nodes for bounded proof metadata tests.
    #[cfg(test)]
    pub(super) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Allocate a stable decision identity under the caller's shared work budget.
    pub(super) fn variable(&mut self, work: &mut usize, span: Span) -> Checked<Predicate> {
        let variable = self.variables;
        let result = self.branch(
            Branch {
                variable,
                low: Predicate::FALSE,
                high: Predicate::TRUE,
            },
            work,
            span,
        )?;
        self.variables += 1;
        Ok(result)
    }
    /// Canonical conjunction preserves correlations between aliases selected on one branch.
    pub(super) fn and(
        &mut self,
        a: Predicate,
        b: Predicate,
        work: &mut usize,
        span: Span,
    ) -> Checked<Predicate> {
        self.apply(Operation::And, a, b, work, span, 0)
    }
    /// Alternative handled paths cover their union without conflating the underlying values.
    pub(super) fn or(
        &mut self,
        a: Predicate,
        b: Predicate,
        work: &mut usize,
        span: Span,
    ) -> Checked<Predicate> {
        self.apply(Operation::Or, a, b, work, span, 0)
    }
    /// Negation is structural and bounded rather than a new unrelated branch variable.
    pub(super) fn not(&mut self, a: Predicate, work: &mut usize, span: Span) -> Checked<Predicate> {
        self.apply(Operation::Xor, a, Predicate::TRUE, work, span, 0)
    }
    /// Reject unproved paths by asking whether their difference is satisfiable.
    pub(super) fn implies(
        &mut self,
        a: Predicate,
        b: Predicate,
        work: &mut usize,
        span: Span,
    ) -> Checked<bool> {
        let missing = self.not(b, work, span)?;
        Ok(self.and(a, missing, work, span)? == Predicate::FALSE)
    }
    /// Intern only after charging, with an independent bounded decision-node ceiling.
    fn branch(&mut self, node: Branch, work: &mut usize, span: Span) -> Checked<Predicate> {
        charge(work, 1, span)?;
        if node.low == node.high {
            return Ok(node.low);
        }
        if let Some(id) = self.intern.get(&node) {
            return Ok(*id);
        }
        if self.nodes.len() >= 32_768 {
            return Err(Diagnostic::new(
                span,
                "Result obligation predicate node limit exceeded",
            ));
        }
        let id = Predicate(self.nodes.len() + 2);
        self.nodes.push(node);
        self.intern.insert(node, id);
        Ok(id)
    }
    /// Apply a Boolean operator by memoized ordered splitting, never enumerating complete paths.
    fn apply(
        &mut self,
        op: Operation,
        mut a: Predicate,
        mut b: Predicate,
        work: &mut usize,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        charge(work, 1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation predicate depth limit exceeded",
            ));
        }
        if b < a {
            std::mem::swap(&mut a, &mut b);
        }
        if let Some(value) = terminal(op, a, b) {
            return Ok(value);
        }
        if let Some(value) = self.cache.get(&(op, a, b)) {
            return Ok(*value);
        }
        let left = self.get(a, span)?;
        let right = self.get(b, span)?;
        let variable = left
            .map_or(usize::MAX, |n| n.variable)
            .min(right.map_or(usize::MAX, |n| n.variable));
        let (al, ah) = split(left, variable, a);
        let (bl, bh) = split(right, variable, b);
        let low = self.apply(op, al, bl, work, span, depth + 1)?;
        let high = self.apply(op, ah, bh, work, span, depth + 1)?;
        let result = self.branch(
            Branch {
                variable,
                low,
                high,
            },
            work,
            span,
        )?;
        charge(work, 1, span)?;
        self.cache.insert((op, a, b), result);
        Ok(result)
    }
    /// Expose checked decision edges to bounded summary substitution without copying the graph.
    pub(super) fn decision(
        &self,
        id: Predicate,
        span: Span,
    ) -> Checked<(usize, Predicate, Predicate)> {
        let node = self.get(id, span)?.ok_or_else(|| {
            Diagnostic::new(span, "constant Result obligation predicate has no decision")
        })?;
        Ok((node.variable, node.low, node.high))
    }
    /// Alternative guards add one fresh selector after all earlier selectors.
    pub(super) fn latest_variable(
        &self,
        id: Predicate,
        work: &mut usize,
        span: Span,
    ) -> Checked<Option<usize>> {
        let mut pending = vec![id];
        let mut seen = HashSet::new();
        let mut latest = None;
        while let Some(id) = pending.pop() {
            charge(work, 1, span)?;
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = self.get(id, span)? {
                latest = Some(latest.map_or(node.variable, |v: usize| v.max(node.variable)));
                charge(work, 2, span)?;
                pending.push(node.low);
                pending.push(node.high);
            }
        }
        Ok(latest)
    }
    /// Validate opaque internal predicate IDs before indexing the node table.
    fn get(&self, id: Predicate, span: Span) -> Checked<Option<Branch>> {
        if id.0 < 2 {
            return Ok(None);
        }
        self.nodes
            .get(id.0 - 2)
            .copied()
            .map(Some)
            .ok_or_else(|| Diagnostic::new(span, "invalid Result obligation predicate"))
    }
}
/// Constant reductions keep common joins small without allocating decision nodes.
fn terminal(op: Operation, a: Predicate, b: Predicate) -> Option<Predicate> {
    let f = Predicate::FALSE;
    let t = Predicate::TRUE;
    match op {
        Operation::And if a == f || b == f => Some(f),
        Operation::And if a == t || a == b => Some(b),
        Operation::And if b == t => Some(a),
        Operation::Or if a == t || b == t => Some(t),
        Operation::Or if a == f || a == b => Some(b),
        Operation::Or if b == f => Some(a),
        Operation::Xor if a == b => Some(f),
        Operation::Xor if a == f => Some(b),
        Operation::Xor if b == f => Some(a),
        _ => None,
    }
}
/// Split only nodes with the selected earliest variable; constants remain unchanged.
fn split(node: Option<Branch>, variable: usize, original: Predicate) -> (Predicate, Predicate) {
    match node {
        Some(node) if node.variable == variable => (node.low, node.high),
        _ => (original, original),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn predicates_retain_branch_correlation_without_enumerating_paths() {
        let mut p = Predicates::default();
        let mut work = 0;
        let span = Span::default();
        let a = p.variable(&mut work, span).unwrap();
        let b = p.variable(&mut work, span).unwrap();
        let not_a = p.not(a, &mut work, span).unwrap();
        assert_eq!(p.and(a, not_a, &mut work, span).unwrap(), Predicate::FALSE);
        assert_eq!(p.or(a, not_a, &mut work, span).unwrap(), Predicate::TRUE);
        let ab = p.and(a, b, &mut work, span).unwrap();
        let other = p.and(not_a, b, &mut work, span).unwrap();
        assert_eq!(p.or(ab, other, &mut work, span).unwrap(), b);
    }
    #[test]
    fn predicate_work_is_shared_and_charged_before_variable_allocation() {
        let mut p = Predicates::default();
        let mut work = WORK_LIMIT;
        assert!(p.variable(&mut work, Span::default()).is_err());
    }
}
