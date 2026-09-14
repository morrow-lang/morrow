//! Member tests dominate deferred payload reads and immutable subset reboxing.
use super::*;

/// Narrowed values remain unpublished until every sibling structural pattern has succeeded.
pub(super) struct Pending {
    source: Type,
    binding: ir::Param,
    value: String,
}

impl Emitter<'_> {
    /// Require a canonical subject/subset and an exactly matching concrete binder annotation.
    pub(super) fn checked_union_pattern(
        &self,
        subject: &Type,
        narrowed: &Type,
        binding: Option<&ir::Param>,
        span: Span,
    ) -> Lowering<Pattern> {
        resolved(subject, &self.layouts, span, 0)?;
        resolved(narrowed, &self.layouts, span, 0)?;
        if !matches!(subject, Type::Union(_)) || !crate::unions::subset(narrowed, subject) {
            return Err(invalid(
                span,
                "union pattern requires an exact member or subset",
            ));
        }
        if let Some(binding) = binding {
            resolved(&binding.ty, &self.layouts, span, 0)?;
            if binding.ty != *narrowed {
                return Err(invalid(
                    span,
                    "union binding type differs from selected type",
                ));
            }
        }
        Ok(Pattern::UnionSelect {
            narrowed: narrowed.clone(),
            binding: binding.cloned(),
        })
    }

    /// Inspect only the tag before success; discarded member patterns never read or allocate payloads.
    pub(super) fn union_pattern(
        &mut self,
        subject: &Type,
        narrowed: &Type,
        binding: Option<&ir::Param>,
        value: &str,
        state: &mut PatternState<'_>,
        locals: &mut Locals,
    ) -> Lowering<()> {
        let tag = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(value)),
        );
        let mut selected = "0".to_owned();
        for (index, member) in crate::unions::members(subject).iter().enumerate() {
            if crate::unions::members(narrowed).contains(member) {
                let equal = self.assign(
                    locals,
                    Type::Bool,
                    NativeOperation::Binary(
                        MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                        native_operand(&(tag).to_string()),
                        native_operand(&(index).to_string()),
                    ),
                );
                selected = self.assign(
                    locals,
                    Type::Bool,
                    NativeOperation::Binary(
                        MachineBinary::Or,
                        native_operand(&(selected).to_string()),
                        native_operand(&(equal)),
                    ),
                );
            }
        }
        self.require_pattern(&selected, state.failure, locals);
        if let Some(binding) = binding {
            state.union_pending.push(Pending {
                source: subject.clone(),
                binding: binding.clone(),
                value: value.to_owned(),
            });
        }
        Ok(())
    }

    /// Publish full-width bindings on the complete-success predecessor, before evaluating a guard.
    pub(super) fn materialize_unions(
        &mut self,
        state: &mut PatternState<'_>,
        locals: &mut Locals,
    ) -> Lowering<()> {
        for pending in std::mem::take(&mut state.union_pending) {
            let ty = &pending.binding.ty;
            let value = if matches!(ty, Type::Union(_)) {
                self.union_rebox(&pending.source, ty, &pending.value, locals)?
            } else {
                let raw = self.union_payload(&pending.value, locals);
                self.unpack(locals, ty, raw)
            };
            locals.define(pending.binding.id.0, ty.clone(), value, state.span)?;
            state.bindings.push(pending.binding.id.0);
        }
        Ok(())
    }
}

impl Emitter<'_> {
    /// Prove redundancy against all prior rows, including overlapping union subsets in products.
    pub(super) fn covered_candidate(
        &self,
        types: &[Type],
        rows: &[Vec<Pattern>],
        candidate: &[Pattern],
        budget: &mut usize,
        depth: usize,
    ) -> Lowering<bool> {
        if *budget == 0 || depth > MAX_DEPTH {
            return Err(invalid(Span::default(), "match coverage limit exceeded"));
        }
        *budget -= 1;
        if rows.is_empty() {
            return Ok(false);
        }
        if candidate.iter().all(catchall) {
            return self.exhaustive(types, rows, budget, depth);
        }
        if types.is_empty() {
            return Ok(true);
        }
        let common = coverage::shared_columns(rows, types.len(), budget)?;
        let common = candidate
            .iter()
            .take(common)
            .take_while(|p| catchall(p))
            .count();
        if common > 0 {
            let trimmed = coverage::trim_columns(rows, common, budget)?;
            return self.covered_candidate(
                &types[common..],
                &trimmed,
                &candidate[common..],
                budget,
                depth,
            );
        }
        if let Some(variants) = self.coverage_variants(&types[0]) {
            for (tag, fields) in variants.into_iter().enumerate() {
                charge_cells(candidate.len() + fields.len(), budget)?;
                let Some(mut wanted) = specialize(&candidate[0], &types[0], tag, fields.len())
                else {
                    continue;
                };
                let projected = project_rows(rows, &types[0], tag, fields.len(), budget)?;
                wanted.extend_from_slice(&candidate[1..]);
                let mut next = fields;
                next.extend_from_slice(&types[1..]);
                if !self.covered_candidate(&next, &projected, &wanted, budget, depth + 1)? {
                    return Ok(false);
                }
            }
            Ok(true)
        } else {
            let mut projected = vec![];
            for row in rows {
                charge_cells(row.len(), budget)?;
                if subsumes(&row[0], &candidate[0]) {
                    projected.push(row[1..].to_vec());
                }
            }
            self.covered_candidate(&types[1..], &projected, &candidate[1..], budget, depth + 1)
        }
    }
}

/// Charge matrix copying before retaining row payloads under the existing coverage budget.
fn charge_cells(count: usize, budget: &mut usize) -> Lowering<()> {
    *budget = budget
        .checked_sub(count)
        .ok_or_else(|| invalid(Span::default(), "match coverage limit exceeded"))?;
    Ok(())
}

/// Select rows compatible with one exact constructor, preserving subsequent product columns.
fn project_rows(
    rows: &[Vec<Pattern>],
    ty: &Type,
    tag: usize,
    fields: usize,
    budget: &mut usize,
) -> Lowering<Vec<Vec<Pattern>>> {
    let mut projected = vec![];
    for row in rows {
        charge_cells(row.len() + fields, budget)?;
        if let Some(mut head) = specialize(&row[0], ty, tag, fields) {
            head.extend_from_slice(&row[1..]);
            projected.push(head);
        }
    }
    Ok(projected)
}
