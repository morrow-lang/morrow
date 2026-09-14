//! Atomic pattern bindings and suffix allocation after every structural check succeeds.
use super::*;

enum Binding<'a> {
    Value(ir::LocalId, &'a Value),
    Tail(ir::LocalId, &'a Rc<Vec<Value>>, usize, bool),
}

impl Machine {
    /// Stage bindings, reserve all copy work, then publish values atomically.
    pub(super) fn pattern(&mut self, pattern: &ir::Pattern, value: &Value) -> Eval<bool> {
        let mut bindings = Vec::new();
        if !self.collect_pattern(pattern, value, &mut bindings, 0)? {
            return Ok(false);
        }
        for binding in &bindings {
            if let Binding::Tail(_, values, offset, _) = binding
                && *offset != 0
            {
                self.charge_steps(values.len() - offset)?;
            }
        }
        for binding in bindings {
            let (id, value) = match binding {
                Binding::Value(id, value) => (id, value.clone()),
                Binding::Tail(id, values, offset, tuple) => {
                    let suffix = if offset == 0 {
                        values.clone()
                    } else {
                        Rc::new(values[offset..].to_vec())
                    };
                    let value = if tuple && suffix.is_empty() {
                        Value::Unit
                    } else if tuple {
                        Value::Sum(0, suffix)
                    } else {
                        Value::List(suffix)
                    };
                    (id, value)
                }
            };
            self.locals.insert(id.0, value);
        }
        Ok(true)
    }

    /// Check nested tags and lengths without allocating any list or tuple suffix.
    fn collect_pattern<'a>(
        &mut self,
        pattern: &ir::Pattern,
        value: &'a Value,
        bindings: &mut Vec<Binding<'a>>,
        depth: usize,
    ) -> Eval<bool> {
        use ir::Pattern::*;
        self.charge_step()?;
        if depth >= 128 {
            return Err(fault("interactive pattern depth limit exceeded"));
        }
        Ok(match (pattern, value) {
            (UnionSelect { narrowed, binding }, Value::Union(carrier)) => {
                if !crate::unions::members(narrowed).contains(&carrier.member) {
                    return Ok(false);
                }
                if let Some(binding) = binding {
                    let selected = if matches!(narrowed, Type::Union(_)) {
                        value
                    } else {
                        &carrier.value
                    };
                    bindings.push(Binding::Value(binding.id, selected));
                }
                true
            }
            (Newtype(inner), value) => self.collect_pattern(inner, value, bindings, depth + 1)?,
            (Wildcard, _) => true,
            (Bind(id), value) => {
                bindings.push(Binding::Value(*id, value));
                true
            }
            (Int(a), Value::Int(b)) => a == b,
            (Bool(a), Value::Bool(b)) => a == b,
            (String(a), Value::String(b)) => a == b.as_ref(),
            (Tuple(fields), Value::Unit) => fields.is_empty(),
            (Tuple(fields), Value::Sum(0, values)) => {
                fields.len() == values.len() && self.prefix(fields, values, bindings, depth)?
            }
            (Variant { tag, fields }, Value::Sum(actual, values)) => {
                tag == actual
                    && fields.len() == values.len()
                    && self.prefix(fields, values, bindings, depth)?
            }
            (List { prefix, rest }, Value::List(values)) => {
                self.sequence_pattern(prefix, rest.as_deref(), values, false, bindings, depth)?
            }
            (TupleRest { prefix, rest }, Value::Sum(0, values)) => {
                self.sequence_pattern(prefix, Some(rest), values, true, bindings, depth)?
            }
            (TupleRest { prefix, rest }, Value::Unit) if prefix.is_empty() => {
                self.collect_pattern(rest, value, bindings, depth + 1)?
            }
            (
                Constructor {
                    constructor,
                    binding,
                },
                Value::Sum(tag, fields),
            ) => Self::constructor_pattern(*constructor, *binding, *tag, fields, bindings),
            _ => false,
        })
    }

    /// Traverse a prefix only after its owner has proved sufficient length.
    fn prefix<'a>(
        &mut self,
        patterns: &[ir::Pattern],
        values: &'a [Value],
        bindings: &mut Vec<Binding<'a>>,
        depth: usize,
    ) -> Eval<bool> {
        for (pattern, value) in patterns.iter().zip(values) {
            if !self.collect_pattern(pattern, value, bindings, depth + 1)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A named rest queues a copy; ignored rest has no allocation or copy work.
    fn sequence_pattern<'a>(
        &mut self,
        prefix: &[ir::Pattern],
        rest: Option<&ir::Pattern>,
        values: &'a Rc<Vec<Value>>,
        tuple: bool,
        bindings: &mut Vec<Binding<'a>>,
        depth: usize,
    ) -> Eval<bool> {
        if values.len() < prefix.len() || (rest.is_none() && values.len() != prefix.len()) {
            return Ok(false);
        }
        if !self.prefix(prefix, values, bindings, depth)? {
            return Ok(false);
        }
        match rest {
            Some(ir::Pattern::Bind(id)) => {
                self.charge_step()?;
                bindings.push(Binding::Tail(*id, values, prefix.len(), tuple));
            }
            Some(ir::Pattern::Wildcard) | None => {}
            _ => return Err(fault("invalid sequence rest pattern")),
        }
        Ok(true)
    }

    /// Legacy single-payload constructors share the same staged binding behavior.
    fn constructor_pattern<'a>(
        constructor: crate::Constructor,
        binding: Option<ir::LocalId>,
        tag: usize,
        fields: &'a [Value],
        bindings: &mut Vec<Binding<'a>>,
    ) -> bool {
        let expected = usize::from(matches!(
            constructor,
            crate::Constructor::None | crate::Constructor::Err
        ));
        if expected != tag {
            return false;
        }
        if let Some(id) = binding {
            let Some(value) = fields.first() else {
                return false;
            };
            bindings.push(Binding::Value(id, value));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail_pattern() -> ir::Pattern {
        ir::Pattern::List {
            prefix: vec![ir::Pattern::Wildcard],
            rest: Some(Box::new(ir::Pattern::Bind(ir::LocalId(1)))),
        }
    }

    #[test]
    fn failed_later_sibling_never_copies_or_publishes_a_tail() {
        let mut machine = Machine::new(Rc::new(ir::Program::default()), HashMap::new());
        let pattern = ir::Pattern::Tuple(vec![tail_pattern(), ir::Pattern::Bool(true)]);
        let values = Value::List(Rc::new(vec![Value::Int(0); 65_536]));
        let subject = Value::Sum(0, Rc::new(vec![values, Value::Bool(false)]));
        assert!(!machine.pattern(&pattern, &subject).unwrap());
        assert!(machine.steps < 10);
        assert!(machine.locals.is_empty());
    }

    #[test]
    fn copy_work_is_reserved_before_any_success_binding_is_published() {
        let mut machine = Machine::new(Rc::new(ir::Program::default()), HashMap::new());
        machine.locals.insert(1, Value::Int(42));
        machine.steps = 99_000;
        let subject = Value::List(Rc::new(vec![Value::Int(0); 65_536]));
        assert!(machine.pattern(&tail_pattern(), &subject).is_err());
        assert_eq!(machine.locals[&1], Value::Int(42));
        assert_eq!(machine.locals.len(), 1);
    }

    #[test]
    fn whole_list_rest_preserves_the_original_allocation() {
        let mut machine = Machine::new(Rc::new(ir::Program::default()), HashMap::new());
        let values = Rc::new(vec![Value::Int(7); 65_536]);
        let subject = Value::List(values.clone());
        let pattern = ir::Pattern::List {
            prefix: Vec::new(),
            rest: Some(Box::new(ir::Pattern::Bind(ir::LocalId(1)))),
        };
        assert!(machine.pattern(&pattern, &subject).unwrap());
        let Value::List(bound) = &machine.locals[&1] else {
            panic!("expected list");
        };
        assert!(Rc::ptr_eq(bound, &values));
        assert!(machine.steps < 10);
    }
}
