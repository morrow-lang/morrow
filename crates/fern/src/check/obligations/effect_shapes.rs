//! Callable effect specialization keys retain code identity, never runtime scalar values.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Shape {
    Plain,
    Callable(usize, Vec<Shape>),
}
#[derive(Default)]
pub(super) struct Cache {
    ready: HashMap<(usize, Vec<Shape>), Rc<Summary>>,
    active: HashSet<(usize, Vec<Shape>)>,
}
impl Engine<'_> {
    /// Resolve a bounded target signature without deriving ownership from function representation.
    fn effect_function(&mut self, id: usize, span: Span) -> Checked<&ir::Function> {
        self.charge(self.program.functions.len(), span)?;
        self.program
            .functions
            .iter()
            .find(|f| f.id.0 == id)
            .ok_or_else(|| Diagnostic::new(span, "missing Result obligation callable definition"))
    }
    /// Abstract known environments recursively; scalar choices stay parameters of the cached proof.
    pub(super) fn effect_shape(
        &mut self,
        value: &Value,
        span: Span,
        depth: usize,
    ) -> Checked<Shape> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        let Region::Callable { function, captures } = &value.node.kind else {
            return Ok(Shape::Plain);
        };
        self.charge(captures.len(), span)?;
        let captures = captures
            .iter()
            .map(|v| self.effect_shape(v, span, depth + 1))
            .collect::<Checked<_>>()?;
        Ok(Shape::Callable(*function, captures))
    }
    /// Rebuild formal callable environments using checked capture types and symbolic scalar values.
    pub(super) fn fresh_shaped(
        &mut self,
        ty: &Type,
        input: Option<usize>,
        shape: Option<&Shape>,
        span: Span,
        depth: usize,
    ) -> Checked<Value> {
        self.charge(1, span)?;
        if depth >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        let Some(Shape::Callable(function, shapes)) = shape else {
            return self.fresh(ty, input, span, depth);
        };
        if !matches!(ty, Type::Function(..)) {
            return self.unsupported(span);
        }
        let target = self.effect_function(*function, span)?;
        if target.captures.len() != shapes.len() {
            return self.unsupported(span);
        }
        // The program is immutable and independently bounded by the checker entry boundary.
        let program = self.program;
        let target = program
            .functions
            .iter()
            .find(|f| f.id.0 == *function)
            .ok_or_else(|| Diagnostic::new(span, "missing callable capture types"))?;
        self.charge(shapes.len(), span)?;
        let captures = target
            .captures
            .iter()
            .zip(shapes)
            .map(|(p, s)| self.fresh_shaped(&p.ty, input, Some(s), span, depth + 1))
            .collect::<Checked<_>>()?;
        self.node(
            Region::Callable {
                function: *function,
                captures,
            },
            span,
        )
    }
    /// Cache complete summaries per target/environment shape with no recursive self-discharge.
    pub(super) fn effect_summary(
        &mut self,
        id: usize,
        args: &[Value],
        span: Span,
    ) -> Checked<Option<Rc<Summary>>> {
        self.charge(args.len(), span)?;
        let shapes = args
            .iter()
            .map(|v| self.effect_shape(v, span, 0))
            .collect::<Checked<Vec<_>>>()?;
        if shapes.iter().all(|s| *s == Shape::Plain) {
            return Ok(None);
        }
        let key = (id, shapes);
        if let Some(summary) = self.effect_cache.borrow().ready.get(&key) {
            return Ok(Some(summary.clone()));
        }
        if self.effect_cache.borrow().active.len() >= DEPTH_LIMIT {
            return self.unsupported(span);
        }
        if !self.effect_cache.borrow_mut().active.insert(key.clone()) {
            return Err(Diagnostic::new(
                span,
                "Result obligation recursive callable summary requires a fixed point",
            ));
        }
        let result = self.build_effect_summary(id, &key.1, span);
        self.effect_cache.borrow_mut().active.remove(&key);
        let summary = Rc::new(result?);
        self.work = summary.work;
        self.effect_cache
            .borrow_mut()
            .ready
            .insert(key, summary.clone());
        Ok(Some(summary))
    }
    /// A shape proof shares the entire caller budget and substitutes fresh formal inputs once.
    fn build_effect_summary(
        &mut self,
        id: usize,
        shapes: &[Shape],
        span: Span,
    ) -> Checked<Summary> {
        self.charge(self.program.functions.len(), span)?;
        let function = self
            .program
            .functions
            .iter()
            .find(|f| f.id.0 == id)
            .ok_or_else(|| Diagnostic::new(span, "missing Result obligation effect target"))?;
        let mut engine = Engine::new(self.program);
        engine.work = self.work;
        engine.mode = self.mode;
        engine.summaries = self.summaries;
        engine.relevance = self.relevance;
        engine.effect_cache = self.effect_cache.clone();
        analyze_shaped_body(engine, function, shapes)
    }
}
