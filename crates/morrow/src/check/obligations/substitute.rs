//! Bounded graph substitution preserves actual aliases, conditional effects and fresh call outputs.
use super::*;
pub(super) struct Substitution<'a> {
    summary: &'a Summary,
    values: HashMap<usize, Value>,
    input_origins: HashMap<usize, Value>,
    local_origins: HashMap<usize, usize>,
    family_origins: HashSet<usize>,
    variables: HashMap<usize, Predicate>,
    predicates: HashMap<Predicate, Predicate>,
    family_inputs: HashSet<usize>,
    fold_accumulator: bool,
    borrow_after: Option<usize>,
    family_nodes: HashSet<usize>,
    partial_return: bool,
}
impl<'a> Substitution<'a> {
    /// Separate each call's fresh identities while retaining shared input graph bindings.
    pub(super) fn new(summary: &'a Summary) -> Self {
        Self {
            summary,
            values: HashMap::new(),
            input_origins: HashMap::new(),
            local_origins: HashMap::new(),
            family_origins: HashSet::new(),
            variables: HashMap::new(),
            predicates: HashMap::new(),
            family_inputs: HashSet::new(),
            fold_accumulator: false,
            borrow_after: None,
            family_nodes: HashSet::new(),
            partial_return: false,
        }
    }
    /// A complete traversal universally visits its item; unrelated captured values remain borrowed.
    pub(super) fn traversal(summary: &'a Summary) -> Self {
        let mut substitution = Self::new(summary);
        substitution.family_inputs.insert(0);
        substitution.borrow_after = Some(if summary.breaks == Predicate::FALSE {
            1
        } else {
            0
        });
        substitution
    }
    /// Early-exit search can execute callbacks but proves no universal input acknowledgement.
    pub(super) fn search(summary: &'a Summary) -> Self {
        let mut substitution = Self::traversal(summary);
        substitution.borrow_after = Some(0);
        substitution
    }
    /// A fold universally checks both the old accumulator and each input element.
    pub(super) fn fold(summary: &'a Summary) -> Self {
        let mut substitution = Self::traversal(summary);
        substitution.family_inputs.insert(1);
        substitution.borrow_after = Some(2);
        substitution.fold_accumulator = true;
        substitution
    }
    /// Induction requires every possible old accumulator duty handled or retained, never overwritten.
    fn verify_accumulator(&mut self, engine: &mut Engine<'_>, span: Span) -> Checked<()> {
        for origin in &self.summary.origins {
            if origin.input != Some(0) {
                continue;
            }
            let exists = self.predicate(engine, origin.exists, span, 0)?;
            let exits = self.predicate(engine, self.summary.exits, span, 0)?;
            let required = engine
                .predicates
                .and(exists, exits, &mut engine.work, span)?;
            let handled = self.predicate(engine, origin.handled, span, 0)?;
            let returned = self.predicate(engine, origin.returned, span, 0)?;
            let disposed = engine
                .predicates
                .or(handled, returned, &mut engine.work, span)?;
            if !engine
                .predicates
                .implies(required, disposed, &mut engine.work, span)?
            {
                return Err(Diagnostic::new(
                    span,
                    "Result obligation fold replaces an unhandled accumulator",
                ));
            }
        }
        Ok(())
    }
    /// Bind input shapes first, then substitute fresh origins, effects, output and normal successors.
    pub(super) fn apply(
        &mut self,
        engine: &mut Engine<'_>,
        args: &[Value],
        span: Span,
    ) -> Checked<Value> {
        if args.len() != self.summary.inputs.len() {
            return engine.unsupported(span);
        }
        engine.charge(args.len(), span)?;
        for (index, (formal, actual)) in self.summary.inputs.iter().zip(args).enumerate() {
            self.bind(
                engine,
                formal,
                actual,
                self.family_inputs.contains(&index),
                span,
                0,
            )?;
        }
        if self.fold_accumulator {
            self.verify_accumulator(engine, span)?;
        }
        let parent = engine.path;
        self.fresh_origins(engine, parent, span)?;
        self.effects(engine, parent, span)?;
        self.callback_requirements(engine, parent, span)?;
        engine.path = parent;
        let output = if self.summary.iteration_returns.is_empty() {
            self.value(engine, &self.summary.output, span, 0)?
        } else {
            engine.node(Region::Empty, span)?
        };
        let exits = self.predicate(engine, self.summary.exits, span, 0)?;
        let terminal = self.terminal_paths(engine, parent, span)?;
        if self.borrow_after.is_some() && terminal != Predicate::FALSE {
            self.return_capture_effects(engine, terminal, span)?;
        }
        engine.path = terminal;
        engine.terminate(span, 0)?;
        let normal = engine.predicates.not(terminal, &mut engine.work, span)?;
        let normal = engine
            .predicates
            .and(exits, normal, &mut engine.work, span)?;
        engine.path = engine
            .predicates
            .and(parent, normal, &mut engine.work, span)?;
        Ok(output)
    }
    /// Import the terminal subset without confusing it with ordinary returning paths.
    fn terminal_paths(
        &mut self,
        engine: &mut Engine<'_>,
        parent: Predicate,
        span: Span,
    ) -> Checked<Predicate> {
        let terminal = self.predicate(engine, self.summary.terminated, span, 0)?;
        engine
            .predicates
            .and(parent, terminal, &mut engine.work, span)
    }
    /// Import real function returns separately; a selected iteration cannot return the whole family.
    pub(super) fn iteration_returns(
        &mut self,
        engine: &mut Engine<'_>,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        let parent = engine.path;
        let mut returned = Predicate::FALSE;
        self.partial_return = true;
        for (guard, value) in &self.summary.iteration_returns {
            engine.charge(1, span)?;
            let guard = self.predicate(engine, *guard, span, 0)?;
            let guard = engine
                .predicates
                .and(parent, guard, &mut engine.work, span)?;
            if guard == Predicate::FALSE {
                continue;
            }
            self.return_capture_effects(engine, guard, span)?;
            engine.path = guard;
            let value = self.value(engine, value, span, 0)?;
            engine.function_exit(&value, span, depth)?;
            returned = engine
                .predicates
                .or(returned, guard, &mut engine.work, span)?;
        }
        self.partial_return = false;
        engine.path = parent;
        Ok(returned)
    }
    /// On a real return, guaranteed capture cleanup handles that exit's exact outer aliases.
    fn return_capture_effects(
        &mut self,
        engine: &mut Engine<'_>,
        parent: Predicate,
        span: Span,
    ) -> Checked<()> {
        for (id, origin) in self.summary.origins.iter().enumerate() {
            if !origin.input.is_some_and(|index| index > 0) {
                continue;
            }
            engine.charge(1, span)?;
            let handled = self.predicate(engine, origin.handled, span, 0)?;
            engine.path = engine
                .predicates
                .and(parent, handled, &mut engine.work, span)?;
            let value = self
                .input_origins
                .get(&id)
                .ok_or_else(|| Diagnostic::new(span, "missing iteration capture obligation"))?;
            engine.dispose(value, !origin.aggregate, false, span)?;
        }
        Ok(())
    }
    /// Child bindings are structural, with complete collection representatives tracked separately.
    fn bind(
        &mut self,
        engine: &mut Engine<'_>,
        formal: &Value,
        actual: &Value,
        family: bool,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        engine.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return engine.unsupported(span);
        }
        if family {
            self.family_nodes.insert(formal.node.id);
        }
        self.values.insert(formal.node.id, actual.clone());
        if matches!(actual.node.kind, Region::Empty) {
            return self.absent(engine, formal, actual, span, depth);
        }
        self.bind_kind(engine, formal, actual, family, span, depth)
    }
    /// Apply shape-specific bindings after the common depth, identity and inactive-value checks.
    fn bind_kind(
        &mut self,
        engine: &mut Engine<'_>,
        formal: &Value,
        actual: &Value,
        family: bool,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        match &formal.node.kind {
            Region::RecursiveCut {
                origin: Some(id), ..
            } => self.bind_cut(*id, actual, family),
            Region::Nominal { expanded, .. } => {
                if let Some(value) = expanded.borrow().as_ref() {
                    self.bind(engine, value, actual, family, span, depth + 1)?;
                }
            }
            Region::Union { members, value } => {
                let actual = engine.union_carrier(actual, members, span, depth + 1)?;
                self.bind(engine, value, &actual, family, span, depth + 1)?;
            }
            Region::Callable { function, captures } => {
                self.bind_callable(
                    engine,
                    (*function, captures),
                    actual,
                    family,
                    span,
                    depth + 1,
                )?;
            }
            Region::Boolean(p) if !family => self.bind_boolean(engine, *p, actual, span)?,
            Region::Product(fields) => {
                self.bind_product(engine, fields, actual, family, span, depth + 1)?;
            }
            Region::Sum {
                origin,
                guards,
                variants,
                ..
            } => {
                if let Some(id) = origin {
                    self.bind_cut(*id, actual, family);
                }
                self.bind_variants(engine, (guards, variants), actual, family, span, depth + 1)?;
            }
            Region::List {
                items, nonempty, ..
            } => {
                self.bind_list(engine, (*nonempty, items), actual, family, span, depth + 1)?;
            }
            Region::Map {
                entries, nonempty, ..
            } => {
                self.bind_map(
                    engine,
                    (*nonempty, entries),
                    actual,
                    family,
                    span,
                    depth + 1,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
    /// Scalar input predicates bind only to the actual value's checked Boolean identity.
    fn bind_boolean(
        &mut self,
        engine: &mut Engine<'_>,
        formal: Predicate,
        actual: &Value,
        span: Span,
    ) -> Checked<()> {
        let value = engine.condition(actual, span)?;
        self.bind_variable(engine, formal, value, span)
    }
    /// Product fields retain the exact actual projection selected by their source position.
    fn bind_product(
        &mut self,
        engine: &mut Engine<'_>,
        fields: &[Value],
        actual: &Value,
        family: bool,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        engine.charge(fields.len(), span)?;
        for (index, field) in fields.iter().enumerate() {
            let value = engine.field(actual, index, span)?;
            self.bind(engine, field, &value, family, span, depth)?;
        }
        Ok(())
    }
    /// Family predicates stay universally quantified; one whole-list input binds its cardinality.
    fn bind_list(
        &mut self,
        engine: &mut Engine<'_>,
        formal: (Predicate, &[Value]),
        actual: &Value,
        family: bool,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        if !family {
            let nonempty = engine.list_nonempty(actual, span, 0)?;
            self.bind_variable(engine, formal.0, nonempty, span)?;
        }
        let actual = Self::family(engine, actual, false, span, depth)?;
        for item in formal.1 {
            self.bind(engine, item, &actual, true, span, depth)?;
        }
        Ok(())
    }
    /// Whole-map arguments retain cardinality; universal value families remain independent.
    fn bind_map(
        &mut self,
        engine: &mut Engine<'_>,
        formal: (Predicate, &[(Option<Key>, Value)]),
        actual: &Value,
        family: bool,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        if !family {
            let nonempty = engine.map_nonempty(actual, span, 0)?;
            self.bind_variable(engine, formal.0, nonempty, span)?;
        }
        let actual = Self::family(engine, actual, true, span, depth)?;
        for (_, item) in formal.1 {
            self.bind(engine, item, &actual, true, span, depth)?;
        }
        Ok(())
    }
    /// Target identities are exact while capture scalars and aliases bind structurally.
    fn bind_callable(
        &mut self,
        engine: &mut Engine<'_>,
        formal: (usize, &[Value]),
        actual: &Value,
        family: bool,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        let Region::Callable { function, captures } = &actual.node.kind else {
            return engine.unsupported(span);
        };
        if formal.0 != *function || formal.1.len() != captures.len() {
            return engine.unsupported(span);
        }
        engine.charge(captures.len(), span)?;
        for (formal, actual) in formal.1.iter().zip(captures) {
            self.bind(engine, formal, actual, family, span, depth)?;
        }
        Ok(())
    }
    /// An inactive payload has no actual origins; it must not be replaced by fresh caller debt.
    fn absent(
        &mut self,
        engine: &mut Engine<'_>,
        formal: &Value,
        empty: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        if let Region::Nominal { expanded, .. } = &formal.node.kind {
            return if let Some(value) = expanded.borrow().as_ref() {
                self.bind(engine, value, empty, false, span, depth + 1)
            } else {
                Ok(())
            };
        }
        let mut children: Vec<&Value> = Vec::new();
        match &formal.node.kind {
            Region::RecursiveCut {
                origin: Some(id), ..
            } => {
                self.input_origins.insert(*id, empty.clone());
            }
            Region::Sum {
                origin, variants, ..
            } => {
                if let Some(id) = origin {
                    self.input_origins.insert(*id, empty.clone());
                }
                for fields in variants {
                    for child in fields {
                        engine.charge(1, span)?;
                        children.push(child);
                    }
                }
            }
            _ => {
                for child in values::children(&formal.node.kind) {
                    engine.charge(1, span)?;
                    children.push(child);
                }
            }
        }
        for child in children {
            self.bind(engine, child, empty, false, span, depth + 1)?;
        }
        Ok(())
    }
    /// Each newly allocated selector is the latest variable in its ordered alternative guard.
    fn bind_variable(
        &mut self,
        engine: &mut Engine<'_>,
        formal: Predicate,
        actual: Predicate,
        span: Span,
    ) -> Checked<()> {
        let variable = self
            .summary
            .predicates
            .latest_variable(formal, &mut engine.work, span)?;
        if let Some(variable) = variable {
            self.variables.insert(variable, actual);
        }
        Ok(())
    }
    /// Substitute exclusive scalar variants; dynamic families require universal rather than tag-wide facts.
    fn bind_variants(
        &mut self,
        engine: &mut Engine<'_>,
        variants: (&[Predicate], &[Vec<Value>]),
        actual: &Value,
        family: bool,
        span: Span,
        depth: usize,
    ) -> Checked<()> {
        let (guards, variants) = variants;
        engine.charge(variants.len(), span)?;
        for (index, fields) in variants.iter().enumerate() {
            let (guard, values) = engine.variant(actual, index, span, depth)?;
            if !family && index + 1 < guards.len() {
                self.bind_variable(engine, guards[index], guard, span)?;
            }
            if guard == Predicate::FALSE {
                let empty = engine.node(Region::Empty, span)?;
                for field in fields {
                    self.bind(engine, field, &empty, family, span, depth)?;
                }
            } else {
                if fields.len() != values.len() {
                    return engine.unsupported(span);
                }
                for (field, value) in fields.iter().zip(values) {
                    self.bind(engine, field, &value, family, span, depth)?;
                }
            }
        }
        Ok(())
    }
    /// A representative maps to all actual elements only for proven complete family operations.
    pub(super) fn family(
        engine: &mut Engine<'_>,
        actual: &Value,
        map: bool,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        engine.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return engine.unsupported(span);
        }
        if let Region::Choice(choices) = &actual.node.kind {
            engine.charge(choices.len(), span)?;
            let mut result = Vec::new();
            for (guard, value) in choices {
                if *guard != Predicate::FALSE {
                    let value = if actual.complete {
                        value.clone()
                    } else {
                        value.partial()
                    };
                    result.push((*guard, Self::family(engine, &value, map, span, depth + 1)?));
                }
            }
            return engine.node(Region::Choice(result), span);
        }
        let items: Vec<&Value> = match &actual.node.kind {
            Region::List { items, .. } if !map => {
                engine.charge(items.len(), span)?;
                items.iter().collect()
            }
            Region::Map { entries, .. } if map => {
                engine.charge(entries.len(), span)?;
                entries.iter().map(|(_, v)| v).collect()
            }
            _ => return engine.unsupported(span),
        };
        engine.node(
            Region::Choice(
                items
                    .into_iter()
                    .map(|v| {
                        (
                            Predicate::TRUE,
                            if actual.complete {
                                v.clone()
                            } else {
                                v.partial()
                            },
                        )
                    })
                    .collect(),
            ),
            span,
        )
    }
    /// Import only actual locally produced origins; formal input duties are mapped to caller values.
    fn fresh_origins(
        &mut self,
        engine: &mut Engine<'_>,
        parent: Predicate,
        span: Span,
    ) -> Checked<()> {
        engine.charge(self.summary.origins.len(), span)?;
        for (id, origin) in self.summary.origins.iter().enumerate() {
            if origin.input.is_some() {
                continue;
            }
            let exists = self.predicate(engine, origin.exists, span, 0)?;
            engine.path = engine
                .predicates
                .and(parent, exists, &mut engine.work, span)?;
            let actual = engine.origin(None, span)?;
            engine.origins[actual].aggregate = origin.aggregate;
            self.local_origins.insert(id, actual);
        }
        engine.path = parent;
        Ok(())
    }
    /// Borrowing never discharges; a family effect requires proof for every representative variant.
    fn effects(&mut self, engine: &mut Engine<'_>, parent: Predicate, span: Span) -> Checked<()> {
        for (id, origin) in self.summary.origins.iter().enumerate() {
            if origin
                .input
                .is_some_and(|index| self.borrow_after.is_some_and(|first| index >= first))
            {
                continue;
            }
            let handled = if self.family_origins.contains(&id) {
                if !self.universal(engine, origin, span)? {
                    continue;
                }
                Predicate::TRUE
            } else {
                self.predicate(engine, origin.handled, span, 0)?
            };
            engine.path = engine
                .predicates
                .and(parent, handled, &mut engine.work, span)?;
            if let Some(value) = self.input_origins.get(&id) {
                engine.dispose(value, !origin.aggregate, false, span)?;
            } else if let Some(actual) = self.local_origins.get(&id) {
                engine.origins[*actual].handled = engine.path;
            } else {
                return engine.unsupported(span);
            }
        }
        Ok(())
    }
    /// Deferred callback duties survive composition without granting any actual handling credit.
    fn callback_requirements(
        &mut self,
        engine: &mut Engine<'_>,
        parent: Predicate,
        span: Span,
    ) -> Checked<()> {
        engine.charge(self.summary.pending_callbacks.len(), span)?;
        for (id, guard) in &self.summary.pending_callbacks {
            if self.summary.origins[*id]
                .input
                .is_some_and(|index| self.borrow_after.is_some_and(|first| index >= first))
            {
                continue;
            }
            let guard = self.predicate(engine, *guard, span, 0)?;
            engine.path = engine
                .predicates
                .and(parent, guard, &mut engine.work, span)?;
            if let Some(value) = self.input_origins.get(id) {
                engine.require_callback(value, span)?;
            } else if let Some(actual) = self.local_origins.get(id) {
                let old = engine
                    .pending_callbacks
                    .get(actual)
                    .copied()
                    .unwrap_or(Predicate::FALSE);
                let required = engine
                    .predicates
                    .or(old, engine.path, &mut engine.work, span)?;
                engine.pending_callbacks.insert(*actual, required);
            } else {
                return engine.unsupported(span);
            }
        }
        Ok(())
    }
    /// Test a family contract in the template predicate space before applying it to actual elements.
    fn universal(&mut self, engine: &mut Engine<'_>, origin: &Origin, span: Span) -> Checked<bool> {
        let exists = self.predicate(engine, origin.exists, span, 0)?;
        let handled = self.predicate(engine, origin.handled, span, 0)?;
        engine
            .predicates
            .implies(exists, handled, &mut engine.work, span)
    }
    /// Memoized Boolean substitution preserves caller input correlations and fresh callee choices.
    fn predicate(
        &mut self,
        engine: &mut Engine<'_>,
        formal: Predicate,
        span: Span,
        depth: usize,
    ) -> Checked<Predicate> {
        engine.charge(1, span)?;
        if formal.0 < 2 {
            return Ok(formal);
        }
        if depth >= DEPTH_LIMIT {
            return engine.unsupported(span);
        }
        if let Some(value) = self.predicates.get(&formal) {
            return Ok(*value);
        }
        let (variable, low, high) = self.summary.predicates.decision(formal, span)?;
        let actual = if let Some(value) = self.variables.get(&variable) {
            *value
        } else {
            let value = engine.predicates.variable(&mut engine.work, span)?;
            self.variables.insert(variable, value);
            value
        };
        let low = self.predicate(engine, low, span, depth + 1)?;
        let high = self.predicate(engine, high, span, depth + 1)?;
        let yes = engine
            .predicates
            .and(actual, high, &mut engine.work, span)?;
        let inverse = engine.predicates.not(actual, &mut engine.work, span)?;
        let no = engine
            .predicates
            .and(inverse, low, &mut engine.work, span)?;
        let result = engine.predicates.or(yes, no, &mut engine.work, span)?;
        self.predicates.insert(formal, result);
        Ok(result)
    }
    /// Share bound inputs exactly and reconstruct only locally produced output graph nodes.
    fn value(
        &mut self,
        engine: &mut Engine<'_>,
        formal: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        engine.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return engine.unsupported(span);
        }
        if let Some(value) = self.values.get(&formal.node.id) {
            return Ok(
                if formal.complete
                    && !(self.partial_return && self.family_nodes.contains(&formal.node.id))
                {
                    value.clone()
                } else {
                    value.partial()
                },
            );
        }
        let kind = self.value_kind(engine, &formal.node.kind, span, depth)?;
        let value = engine.node(kind, span)?;
        self.values.insert(formal.node.id, value.clone());
        Ok(if formal.complete {
            value
        } else {
            value.partial()
        })
    }
    /// Reconstruct bounded non-input regions without mixing graph identity and node dispatch.
    fn value_kind(
        &mut self,
        engine: &mut Engine<'_>,
        kind: &Region,
        span: Span,
        depth: usize,
    ) -> Checked<Region> {
        Ok(match kind {
            Region::Empty => Region::Empty,
            Region::UnknownCallable => Region::UnknownCallable,
            Region::Symbolic => {
                if engine.mode == Mode::Concrete {
                    return engine.unsupported(span);
                }
                Region::Symbolic
            }
            Region::Callable { function, captures } => Region::Callable {
                function: *function,
                captures: self.value_list(engine, captures, span, depth + 1)?,
            },
            Region::EditorBorrow => {
                if engine.mode != Mode::Editor {
                    return engine.unsupported(span);
                }
                Region::EditorBorrow
            }
            Region::Scalar(key) => {
                if let Key::String(value) = key {
                    engine.charge(value.len() / 8 + 1, span)?;
                }
                Region::Scalar(key.clone())
            }
            Region::Boolean(p) => Region::Boolean(self.predicate(engine, *p, span, depth + 1)?),
            Region::Product(values) => {
                Region::Product(self.value_list(engine, values, span, depth + 1)?)
            }
            Region::Sum { .. } => self.sum_kind(engine, kind, span, depth + 1)?,
            Region::RecursiveCut {
                layout,
                origin,
                retained,
            } => self.cut_kind(engine, *layout, *origin, retained, span, depth + 1)?,
            Region::Nominal { layout, expanded } => {
                self.nominal_kind(engine, *layout, expanded, span, depth + 1)?
            }
            Region::Union { members, value } => {
                engine.union_members_cost(members, span)?;
                Region::Union {
                    members: members.clone(),
                    value: self.value(engine, value, span, depth + 1)?,
                }
            }
            Region::List {
                items,
                exact,
                nonempty,
            } => Region::List {
                nonempty: self.predicate(engine, *nonempty, span, depth + 1)?,
                items: self.value_list(engine, items, span, depth + 1)?,
                exact: *exact,
            },
            Region::Map {
                entries,
                exact,
                nonempty,
            } => self.map_values(engine, entries, (*exact, *nonempty), span, depth + 1)?,
            Region::Choice(values) => self.choice_kind(engine, values, span, depth + 1)?,
        })
    }
    /// Lazy origin-free nominal expansion preserves only already materialized ordinary values.
    fn nominal_kind(
        &mut self,
        engine: &mut Engine<'_>,
        layout: usize,
        expanded: &RefCell<Option<Value>>,
        span: Span,
        depth: usize,
    ) -> Checked<Region> {
        let expanded = expanded
            .borrow()
            .as_ref()
            .map(|v| self.value(engine, v, span, depth))
            .transpose()?;
        Ok(Region::Nominal {
            layout,
            expanded: RefCell::new(expanded),
        })
    }
    /// Whole-subtree effects bind to actual provenance, preserving sibling identity and partiality.
    fn bind_cut(&mut self, id: usize, actual: &Value, family: bool) {
        self.input_origins.insert(id, actual.clone());
        if family {
            self.family_origins.insert(id);
        }
    }
    /// Fresh returned cuts receive fresh origins, never a copied structural descent certificate.
    fn cut_kind(
        &mut self,
        engine: &mut Engine<'_>,
        layout: usize,
        origin: Option<usize>,
        retained: &[Value],
        span: Span,
        depth: usize,
    ) -> Checked<Region> {
        let origin = origin
            .map(|id| {
                self.local_origins
                    .get(&id)
                    .copied()
                    .ok_or_else(|| Diagnostic::new(span, "missing recursive subtree origin"))
            })
            .transpose()?;
        Ok(Region::RecursiveCut {
            layout,
            origin,
            retained: self.value_list(engine, retained, span, depth)?,
        })
    }
    /// Rebuild branch alternatives with the same substituted source predicates.
    fn choice_kind(
        &mut self,
        engine: &mut Engine<'_>,
        values: &[(Predicate, Value)],
        span: Span,
        depth: usize,
    ) -> Checked<Region> {
        engine.charge(values.len(), span)?;
        let mut result = Vec::new();
        for (guard, value) in values {
            let guard = self.predicate(engine, *guard, span, depth)?;
            if guard != Predicate::FALSE {
                result.push((guard, self.value(engine, value, span, depth)?));
            }
        }
        Ok(Region::Choice(result))
    }
    /// Reconstruct only locally owned sum nodes; input sums have already been structurally bound.
    fn sum_kind(
        &mut self,
        engine: &mut Engine<'_>,
        kind: &Region,
        span: Span,
        depth: usize,
    ) -> Checked<Region> {
        let Region::Sum {
            origin,
            tag,
            guards,
            variants,
        } = kind
        else {
            return engine.unsupported(span);
        };
        let origin = origin
            .map(|id| {
                self.local_origins
                    .get(&id)
                    .copied()
                    .ok_or_else(|| Diagnostic::new(span, "unbound Result obligation origin"))
            })
            .transpose()?;
        let guards = self.predicate_list(engine, guards, span, depth + 1)?;
        engine.charge(variants.len(), span)?;
        let variants = variants
            .iter()
            .map(|v| self.value_list(engine, v, span, depth + 1))
            .collect::<Checked<_>>()?;
        Ok(Region::Sum {
            origin,
            tag: *tag,
            guards,
            variants,
        })
    }
    /// Precharge output product width before cloning or recursively importing its children.
    fn value_list(
        &mut self,
        engine: &mut Engine<'_>,
        values: &[Value],
        span: Span,
        depth: usize,
    ) -> Checked<Vec<Value>> {
        engine.charge(values.len(), span)?;
        values
            .iter()
            .map(|v| self.value(engine, v, span, depth))
            .collect()
    }
    /// Sum guard tables are bounded independently from their payload graph edges.
    fn predicate_list(
        &mut self,
        engine: &mut Engine<'_>,
        values: &[Predicate],
        span: Span,
        depth: usize,
    ) -> Checked<Vec<Predicate>> {
        engine.charge(values.len(), span)?;
        values
            .iter()
            .map(|p| self.predicate(engine, *p, span, depth))
            .collect()
    }
    /// Preserve map keys and values while charging copied string contents before allocation.
    fn map_values(
        &mut self,
        engine: &mut Engine<'_>,
        entries: &[(Option<Key>, Value)],
        metadata: (bool, Predicate),
        span: Span,
        depth: usize,
    ) -> Checked<Region> {
        engine.charge(entries.len(), span)?;
        let mut result = Vec::new();
        for (key, value) in entries {
            if let Some(Key::String(key)) = key {
                engine.charge(key.len() / 8 + 1, span)?;
            }
            result.push((key.clone(), self.value(engine, value, span, depth)?));
        }
        Ok(Region::Map {
            entries: result,
            exact: metadata.0,
            nonempty: self.predicate(engine, metadata.1, span, depth)?,
        })
    }
}
