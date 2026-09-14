//! Shared value regions and conditional coverage keep aliases correlated across source paths.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Key {
    Int(i64),
    Bool(bool),
    String(String),
}
#[derive(Clone, Debug)]
pub(super) struct Value {
    pub node: Rc<Node>,
    pub complete: bool,
}
#[derive(Debug)]
pub(super) struct Node {
    pub id: usize,
    pub kind: Region,
}
#[derive(Debug)]
pub(super) enum Region {
    Empty,
    EditorBorrow,
    Symbolic,
    UnknownCallable,
    Callable {
        function: usize,
        captures: Vec<Value>,
    },
    Scalar(Key),
    Boolean(Predicate),
    Product(Vec<Value>),
    Sum {
        origin: Option<usize>,
        tag: Option<usize>,
        guards: Vec<Predicate>,
        variants: Vec<Vec<Value>>,
    },
    RecursiveCut {
        layout: usize,
        origin: Option<usize>,
        retained: Vec<Value>,
    },
    Nominal {
        layout: usize,
        expanded: RefCell<Option<Value>>,
    },
    Union {
        members: Vec<Type>,
        value: Value,
    },
    List {
        nonempty: Predicate,
        items: Vec<Value>,
        exact: bool,
    },
    Map {
        nonempty: Predicate,
        entries: Vec<(Option<Key>, Value)>,
        exact: bool,
    },
    Choice(Vec<(Predicate, Value)>),
}
impl Value {
    /// A partial projection never upgrades the universal coverage of its underlying family.
    pub(super) fn partial(&self) -> Self {
        Self {
            node: self.node.clone(),
            complete: false,
        }
    }
}
impl Engine<'_> {
    /// Expand concrete shapes with independent duties only on active payload paths.
    pub(super) fn fresh(
        &mut self,
        ty: &Type,
        input: Option<usize>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation type depth limit exceeded",
            ));
        }
        let kind = match ty {
            Type::Function(..) => {
                self.input_callable |= input.is_some();
                Region::UnknownCallable
            }
            Type::Bool => Region::Boolean(self.predicates.variable(&mut self.work, span)?),
            Type::Result(ok, err) => {
                return self.fresh_sum(vec![vec![ok], vec![err]], input, true, span, depth);
            }
            Type::Option(inner) => {
                return self.fresh_sum(vec![vec![inner], vec![]], input, false, span, depth);
            }
            Type::Tuple(types) => {
                Region::Product(self.fresh_values(types.iter(), input, span, depth + 1)?)
            }
            Type::Union(types) => {
                self.charge(types.len(), span)?;
                let value = self.fresh_sum(
                    types.iter().map(|ty| vec![ty]).collect(),
                    input,
                    false,
                    span,
                    depth,
                )?;
                return self.union_value(types, value, span);
            }
            Type::List(inner) => return self.fresh_list(inner, input, span, depth + 1),
            Type::Map(_, inner) => return self.fresh_map(inner, input, span, depth + 1),
            Type::Named(..) => return self.fresh_nominal(ty, input, span, depth),
            Type::Generic(_) if self.mode != Mode::Concrete => Region::Symbolic,
            Type::Infer(_) | Type::Generic(_) | Type::Never => return self.unsupported(span),
            _ => Region::Empty,
        };
        self.node(kind, span)
    }
    /// Product width is charged before allocating child regions.
    fn fresh_values<'b>(
        &mut self,
        types: impl ExactSizeIterator<Item = &'b Type>,
        input: Option<usize>,
        span: Span,
        depth: usize,
    ) -> Checked<Vec<Value>> {
        self.charge(types.len(), span)?;
        types.map(|ty| self.fresh(ty, input, span, depth)).collect()
    }
    /// Sum alternatives are exclusive and exhaustive; nested origins inherit their exact guard.
    pub(super) fn fresh_sum(
        &mut self,
        types: Vec<Vec<&Type>>,
        input: Option<usize>,
        result: bool,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(types.len(), span)?;
        let origin = if result {
            Some(self.origin(input, span)?)
        } else {
            None
        };
        let guards = self.alternatives(types.len(), span)?;
        let parent = self.path;
        let mut variants = Vec::new();
        for (guard, fields) in guards.iter().zip(types) {
            self.path = self.predicates.and(parent, *guard, &mut self.work, span)?;
            variants.push(self.fresh_values(fields.into_iter(), input, span, depth + 1)?);
        }
        self.path = parent;
        self.node(
            Region::Sum {
                origin,
                tag: None,
                guards,
                variants,
            },
            span,
        )
    }
    /// Concrete nominal payloads keep storage identity separate from semantic error layers.
    fn fresh_nominal(
        &mut self,
        ty: &Type,
        input: Option<usize>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        if let Some(layout) = self.lazy_layout(ty, span)? {
            return self.node(
                Region::Nominal {
                    layout,
                    expanded: RefCell::new(None),
                },
                span,
            );
        }
        let index = gate::layout_index(self.program, ty, &mut self.work, span)?;
        self.charge(self.active_nominals.len(), span)?;
        if let Some((_, anchor)) = self.active_nominals.iter().find(|(id, _)| *id == index) {
            return self.recursive_cut(index, *anchor, input, span);
        }
        let anchor = self.node(Region::Empty, span)?.node.id;
        self.active_nominals.push((index, anchor));
        let value = self.fresh_nominal_fields(index, input, span, depth);
        self.active_nominals.pop();
        if let Ok(value) = &value {
            self.nominal_roots.insert(value.node.id, anchor);
            self.certify_ancestors(value.node.id, None, span)?;
        }
        value
    }
    /// Expand stored fields once; repeated layout edges become finite, separately accountable cuts.
    fn fresh_nominal_fields(
        &mut self,
        index: usize,
        input: Option<usize>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        let layout = &self.program.types[index];
        if layout.storage == ir::LayoutStorage::Unboxed {
            let inner = layout
                .variants
                .first()
                .and_then(|v| v.first())
                .ok_or_else(|| Diagnostic::new(span, "invalid newtype obligation layout"))?;
            return self.fresh(inner, input, span, depth + 1);
        }
        let count = layout
            .variants
            .iter()
            .try_fold(layout.variants.len(), |n, v| n.checked_add(v.len()))
            .ok_or_else(|| Diagnostic::new(span, "Result obligation layout size limit exceeded"))?;
        self.charge(count, span)?;
        let types = layout
            .variants
            .iter()
            .map(|fields| fields.iter().collect())
            .collect();
        self.fresh_sum(types, input, false, span, depth)
    }
    /// Allocate bounded mutually exclusive guards without exponential path enumeration.
    pub(super) fn alternatives(&mut self, count: usize, span: Span) -> Checked<Vec<Predicate>> {
        self.charge(count, span)?;
        let mut remaining = Predicate::TRUE;
        let mut guards = Vec::new();
        for i in 0..count {
            if i + 1 == count {
                guards.push(remaining);
                break;
            }
            let variable = self.predicates.variable(&mut self.work, span)?;
            guards.push(
                self.predicates
                    .and(remaining, variable, &mut self.work, span)?,
            );
            let inverse = self.predicates.not(variable, &mut self.work, span)?;
            remaining = self
                .predicates
                .and(remaining, inverse, &mut self.work, span)?;
        }
        Ok(guards)
    }
    /// Coverage maps each original duty to the paths on which this value definitely carries it.
    pub(super) fn origins_of(
        &mut self,
        value: &Value,
        outer: bool,
        span: Span,
    ) -> Checked<BTreeMap<usize, Predicate>> {
        self.coverage(value, outer, span, 0, &mut HashMap::new())
    }
    /// Cache shared regions while preserving predicates; a MAY-alias union is never a guarantee.
    fn coverage(
        &mut self,
        value: &Value,
        outer: bool,
        span: Span,
        depth: usize,
        cache: &mut HashMap<(usize, bool), BTreeMap<usize, Predicate>>,
    ) -> Checked<BTreeMap<usize, Predicate>> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return Err(Diagnostic::new(
                span,
                "Result obligation region depth limit exceeded",
            ));
        }
        if !value.complete {
            return Ok(BTreeMap::new());
        }
        let key = (value.node.id, outer);
        if let Some(found) = cache.get(&key) {
            self.charge(found.len(), span)?;
            return Ok(found.clone());
        }
        let mut result = BTreeMap::new();
        if let Region::Sum {
            origin: Some(id), ..
        } = value.node.kind
        {
            result.insert(id, Predicate::TRUE);
        }
        match &value.node.kind {
            Region::RecursiveCut {
                origin, retained, ..
            } if !outer => {
                if let Some(id) = origin {
                    result.insert(*id, Predicate::TRUE);
                }
                for child in retained {
                    let found = self.coverage(child, false, span, depth + 1, cache)?;
                    self.merge_coverage(&mut result, found, Predicate::TRUE, span)?;
                }
            }
            Region::Nominal { expanded, .. } => {
                if let Some(value) = expanded.borrow().as_ref() {
                    let found = self.coverage(value, outer, span, depth + 1, cache)?;
                    self.merge_coverage(&mut result, found, Predicate::TRUE, span)?;
                }
            }
            Region::Choice(choices) => {
                for (guard, child) in choices {
                    let found = self.coverage(child, outer, span, depth + 1, cache)?;
                    self.merge_coverage(&mut result, found, *guard, span)?;
                }
            }
            Region::Sum {
                guards, variants, ..
            } if !outer => {
                for (guard, fields) in guards.iter().zip(variants) {
                    for child in fields {
                        let found = self.coverage(child, false, span, depth + 1, cache)?;
                        self.merge_coverage(&mut result, found, *guard, span)?;
                    }
                }
            }
            _ if !outer => {
                for child in children(&value.node.kind) {
                    let found = self.coverage(child, false, span, depth + 1, cache)?;
                    self.merge_coverage(&mut result, found, Predicate::TRUE, span)?;
                }
            }
            _ => {}
        }
        self.charge(result.len(), span)?;
        cache.insert(key, result.clone());
        Ok(result)
    }
    /// Join guarded provenance for one origin while charging every set and predicate operation.
    fn merge_coverage(
        &mut self,
        target: &mut BTreeMap<usize, Predicate>,
        source: BTreeMap<usize, Predicate>,
        guard: Predicate,
        span: Span,
    ) -> Checked<()> {
        self.charge(source.len(), span)?;
        for (id, path) in source {
            let path = self.predicates.and(path, guard, &mut self.work, span)?;
            let prior = target.get(&id).copied().unwrap_or(Predicate::FALSE);
            target.insert(id, self.predicates.or(prior, path, &mut self.work, span)?);
        }
        Ok(())
    }
}
/// Borrow unconditionally included product/container children without allocating a work queue.
pub(super) fn children(region: &Region) -> Box<dyn Iterator<Item = &Value> + '_> {
    match region {
        Region::Product(values)
        | Region::List { items: values, .. }
        | Region::RecursiveCut {
            retained: values, ..
        } => Box::new(values.iter()),
        Region::Map { entries, .. } => Box::new(entries.iter().map(|(_, value)| value)),
        Region::Union { value, .. } => Box::new(std::iter::once(value)),
        _ => Box::new(std::iter::empty()),
    }
}
