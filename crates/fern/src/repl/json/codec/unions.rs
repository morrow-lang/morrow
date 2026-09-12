//! Descriptor-only selection cannot execute or allocate a candidate payload.
use super::*;
mod selectors;
impl Execution<'_, '_> {
    /// The interpreter stores semantic member identity; native values already store its ordinal.
    pub(super) fn encode_union(
        &mut self,
        children: &[usize],
        value: &crate::repl::UnionValue,
        depth: usize,
    ) -> Result<Json> {
        let plan = self.plan;
        let (_, nodes) =
            crate::repl::storage::type_size(&value.member).map_err(|_| error(4, -1))?;
        for child in children {
            let cost = crate::unions::cost(&plan.entries[*child].ty, crate::Span::default())
                .map_err(|_| error(4, -1))?;
            Limits::charge(&mut self.budget.limits.work, cost.saturating_add(nodes))?;
            if plan.entries[*child].ty == value.member {
                return self.encode(*child, &value.value, depth + 1);
            }
        }
        Err(error(14, -1))
    }
    /// Select once, decode once, then allocate the successful semantic union envelope.
    pub(super) fn decode_union(
        &mut self,
        children: &[usize],
        input: &Json,
        depth: usize,
    ) -> Result<Value> {
        let selected = self.select(children, input)?;
        let child = children[selected];
        let value = self.decode(child, input, depth + 1)?;
        self.budget.work(1)?;
        self.budget.allocate(16)?;
        let ty = &self.plan.entries[child].ty;
        let (bytes, nodes) = crate::repl::storage::type_size(ty).map_err(|_| error(4, -1))?;
        Limits::charge(&mut self.budget.limits.work, nodes)?;
        Limits::charge(
            &mut self.budget.limits.allocated,
            bytes + std::mem::size_of::<crate::repl::UnionValue>(),
        )?;
        Ok(Value::Union(Rc::new(crate::repl::UnionValue {
            member: ty.clone(),
            value,
        })))
    }
    /// Kind filtering preserves exact primitive failures; refinements never call decode.
    fn select(&mut self, children: &[usize], input: &Json) -> Result<usize> {
        let bit = kind_bit(&input.kind);
        let (mut count, mut selected) = (0, 0);
        for (index, child) in children.iter().enumerate() {
            if self.wire_mask(*child, 0)? & bit != 0 {
                count += 1;
                selected = index;
            }
        }
        if count == 1 {
            return Ok(selected);
        }
        if count == 0 {
            return Err(error(14, -1));
        }
        let mut sum_only = bit == 32;
        if sum_only {
            for child in children {
                if self.wire_mask(*child, 0)? & bit != 0 && self.sum_profile(*child, 0)? != 1 {
                    sum_only = false;
                }
            }
        }
        count = 0;
        for (index, child) in children.iter().enumerate() {
            if self.wire_mask(*child, 0)? & bit != 0
                && self.shape_matches(*child, input, sum_only, 0)?
            {
                count += 1;
                selected = index;
            }
        }
        if count == 1 {
            Ok(selected)
        } else {
            Err(error(14, -1))
        }
    }
    /// Follow only transparent descriptor edges, charging every visit before descent.
    fn wire_mask(&mut self, id: usize, depth: usize) -> Result<u8> {
        self.step(depth)?;
        let plan = self.plan;
        Ok(match &plan.entries[id].kind {
            Wire::Unit => 1,
            Wire::Bool => 2,
            Wire::Int | Wire::Float => 4,
            Wire::String => 8,
            Wire::List(_) | Wire::Tuple(_) => 16,
            Wire::Record(_) | Wire::Map(_) | Wire::Sum(_) => 32,
            Wire::Dynamic => 63,
            Wire::Newtype(child) => self.wire_mask(*child, depth + 1)?,
            Wire::Option(child) => 1 | self.wire_mask(*child, depth + 1)?,
            Wire::Union(children) => {
                let mut mask = 0;
                for child in children {
                    mask |= self.wire_mask(*child, depth + 1)?;
                }
                mask
            }
        })
    }
    /// Object branches are sum-only exactly when every possible object leaf is a sum envelope.
    fn sum_profile(&mut self, id: usize, depth: usize) -> Result<u8> {
        self.step(depth)?;
        let plan = self.plan;
        Ok(match &plan.entries[id].kind {
            Wire::Sum(_) => 1,
            Wire::Record(_) | Wire::Map(_) | Wire::Dynamic => 2,
            Wire::Newtype(child) | Wire::Option(child) => self.sum_profile(*child, depth + 1)?,
            Wire::Union(children) => {
                let mut mask = 0;
                for child in children {
                    mask |= self.sum_profile(*child, depth + 1)?;
                }
                mask
            }
            _ => 0,
        })
    }
}
fn kind_bit(kind: &Kind) -> u8 {
    match kind {
        Kind::Null => 1,
        Kind::Bool(_) => 2,
        Kind::Number(_) => 4,
        Kind::String(_) => 8,
        Kind::Array(_) => 16,
        Kind::Object(..) => 32,
    }
}

#[cfg(test)]
mod tests;
