//! A required field or tuple position can prove an empty structural intersection.
use super::*;

impl Proof<'_, '_> {
    pub(super) fn nested_disjoint(
        &mut self,
        left: usize,
        right: usize,
        active: &mut Vec<(usize, usize)>,
    ) -> Result<bool, Diagnostic> {
        charge(self.work, active.len() + 1, self.span)?;
        // Recursive equality is not evidence of disjointness. A finite sibling
        // discriminator may still prove the pair after this conservative return.
        if active.len() >= 128 || active.contains(&(left, right)) || active.contains(&(right, left))
        {
            return Ok(false);
        }
        active.push((left, right));
        let result = self.profile_pairs(left, right, active);
        active.pop();
        result
    }

    fn profile_pairs(
        &mut self,
        left: usize,
        right: usize,
        active: &mut Vec<(usize, usize)>,
    ) -> Result<bool, Diagnostic> {
        for a in 0..self.profiles[left].len() {
            for b in 0..self.profiles[right].len() {
                charge(self.work, 1, self.span)?;
                let a = self.profiles[left][a];
                let b = self.profiles[right][b];
                let shape_a = match a {
                    Atom::Null => &Shape::Null,
                    Atom::Leaf(id) => &self.nodes[id].shape,
                };
                let shape_b = match b {
                    Atom::Null => &Shape::Null,
                    Atom::Leaf(id) => &self.nodes[id].shape,
                };
                if compare::disjoint(shape_a, shape_b, self.work, self.span)? {
                    continue;
                }
                let (Atom::Leaf(a), Atom::Leaf(b)) = (a, b) else {
                    return Ok(false);
                };
                if !self.child_disjoint(a, b, active)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn child_disjoint(
        &mut self,
        left: usize,
        right: usize,
        active: &mut Vec<(usize, usize)>,
    ) -> Result<bool, Diagnostic> {
        let nodes = self.nodes;
        let a = &nodes[left];
        let b = &nodes[right];
        match (&a.shape, &b.shape) {
            (Shape::Sum(tags_a), Shape::Sum(tags_b))
                if a.variants.len() == tags_a.len() && b.variants.len() == tags_b.len() =>
            {
                for (i, tag) in tags_a.iter().enumerate() {
                    for (j, other) in tags_b.iter().enumerate() {
                        charge(self.work, tag.len().min(other.len()) + 1, self.span)?;
                        if tag != other {
                            continue;
                        }
                        let fields_a = &a.variants[i];
                        let fields_b = &b.variants[j];
                        if fields_a.len() != fields_b.len() {
                            continue;
                        }
                        let mut disjoint = false;
                        for (&left, &right) in fields_a.iter().zip(fields_b) {
                            if self.nested_disjoint(left, right, active)? {
                                disjoint = true;
                                break;
                            }
                        }
                        if !disjoint {
                            return Ok(false);
                        }
                    }
                }
                return Ok(true);
            }
            (Shape::Array(Some(n)), Shape::Array(Some(m)))
                if n == m && a.children.len() == *n && b.children.len() == *m =>
            {
                for (&left, &right) in a.children.iter().zip(&b.children) {
                    if self.nested_disjoint(left, right, active)? {
                        return Ok(true);
                    }
                }
            }
            (Shape::Object(keys_a), Shape::Object(keys_b))
                if a.children.len() == keys_a.len() && b.children.len() == keys_b.len() =>
            {
                for (i, key) in keys_a.iter().enumerate() {
                    for (j, other) in keys_b.iter().enumerate() {
                        charge(
                            self.work,
                            key.name.len().min(other.name.len()) + 1,
                            self.span,
                        )?;
                        if key.name == other.name
                            && (key.required || other.required)
                            && self.nested_disjoint(a.children[i], b.children[j], active)?
                        {
                            return Ok(true);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }
}
