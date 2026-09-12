//! Origin-free recursive layouts use bounded lazy field shapes instead of recursive type expansion.
use super::*;
impl Engine<'_> {
    /// Use actual storage fields, ignoring phantom arguments; only recursive origin-free graphs defer.
    pub(super) fn lazy_layout(&mut self, ty: &Type, span: Span) -> Checked<Option<usize>> {
        let root = gate::layout_index(self.program, ty, &mut self.work, span)?;
        let mut pending = vec![ty];
        let mut visited = HashSet::new();
        let mut recursive = false;
        while let Some(ty) = pending.pop() {
            gate::type_cost(ty, &mut self.work, span)?;
            match ty {
                Type::Named(..) => {
                    let index = gate::layout_index(self.program, ty, &mut self.work, span)?;
                    if !visited.insert(index) {
                        recursive = true;
                        continue;
                    }
                    for fields in &self.program.types[index].variants {
                        self.charge(fields.len(), span)?;
                        pending.extend(fields);
                    }
                }
                Type::Pid(inner) | Type::List(inner) | Type::Option(inner) => pending.push(inner),
                Type::Map(key, value) => {
                    self.charge(2, span)?;
                    pending.extend([key.as_ref(), value.as_ref()]);
                }
                Type::Tuple(fields) | Type::Union(fields) => {
                    self.charge(fields.len(), span)?;
                    pending.extend(fields);
                }
                Type::Generic(_) if self.mode != Mode::Concrete => {}
                Type::Result(..)
                | Type::Function(..)
                | Type::Generic(_)
                | Type::Infer(_)
                | Type::Never => return Ok(None),
                _ => {}
            }
        }
        Ok(recursive.then_some(root))
    }
    /// One observed tag/field installs a stable finite expansion; recursive children stay deferred.
    pub(super) fn expand_nominal(
        &mut self,
        value: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        let Region::Nominal { layout, expanded } = &value.node.kind else {
            return self.unsupported(span);
        };
        if let Some(cached) = expanded.borrow().as_ref() {
            return Ok(if value.complete {
                cached.clone()
            } else {
                cached.partial()
            });
        }
        let layout =
            self.program.types.get(*layout).ok_or_else(|| {
                Diagnostic::new(span, "missing deferred nominal obligation layout")
            })?;
        let mut types = Vec::new();
        self.charge(layout.variants.len(), span)?;
        for fields in &layout.variants {
            self.charge(fields.len(), span)?;
            types.push(fields.iter().collect());
        }
        let output = self.fresh_sum(types, None, false, span, depth + 1)?;
        self.charge(1, span)?;
        expanded.replace(Some(output.clone()));
        Ok(if value.complete {
            output
        } else {
            output.partial()
        })
    }
}
/// Recursive interfaces with no stored duty, generic, or callable can return arbitrary fresh shapes.
pub(super) fn closed_signature(
    program: &ir::Program,
    function: &ir::Function,
    work: &mut usize,
) -> Checked<bool> {
    for ty in function
        .params
        .iter()
        .chain(&function.captures)
        .map(|p| &p.ty)
        .chain([&function.return_type])
    {
        if gate::contains_mode(program, ty, work, function.body.span, true)? {
            return Ok(false);
        }
    }
    Ok(true)
}
