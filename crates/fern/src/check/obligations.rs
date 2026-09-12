//! Decision95 typed Result provenance with conservative production proof boundaries.
use super::{Checked, ir};
use crate::{Constructor, Diagnostic, Span, Type};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
};
#[path = "obligations/expression.rs"]
mod expression;
#[path = "obligations/intrinsics.rs"]
mod intrinsics;
#[path = "obligations/predicates.rs"]
mod predicates;
#[path = "obligations/sum_calls.rs"]
mod sum_calls;
#[cfg(test)]
#[path = "obligations/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "obligations/tests_calls.rs"]
mod tests_calls;
#[cfg(test)]
#[path = "obligations/tests_flow.rs"]
mod tests_flow;
#[cfg(test)]
#[path = "obligations/tests_gate.rs"]
mod tests_gate;
#[cfg(test)]
#[path = "obligations/tests_unions.rs"]
mod tests_unions;
#[path = "obligations/unions.rs"]
mod unions;
#[path = "obligations/values.rs"]
mod values;
use predicates::{Predicate, Predicates};
use values::{Key, Region, Value};
#[path = "obligations/callable.rs"]
mod callable;
#[path = "obligations/calls.rs"]
mod calls;
#[path = "obligations/cardinality.rs"]
mod cardinality;
#[path = "obligations/collection_calls.rs"]
mod collection_calls;
#[path = "obligations/collection_fold.rs"]
mod collection_fold;
#[path = "obligations/collection_search.rs"]
mod collection_search;
#[path = "obligations/effect_shapes.rs"]
mod effect_shapes;
#[path = "obligations/flow.rs"]
mod flow;
#[path = "obligations/gate.rs"]
mod gate;
#[path = "obligations/iteration.rs"]
mod iteration;
#[path = "obligations/map_views.rs"]
mod map_views;
#[path = "obligations/mutual_trees.rs"]
mod mutual_trees;
#[path = "obligations/nominal.rs"]
mod nominal;
#[path = "obligations/partitions.rs"]
mod partitions;
#[path = "obligations/patterns.rs"]
mod patterns;
#[path = "obligations/recursive.rs"]
mod recursive;
#[path = "obligations/recursive_builders.rs"]
mod recursive_builders;
#[path = "obligations/recursive_callables.rs"]
mod recursive_callables;
#[path = "obligations/recursive_handlers.rs"]
mod recursive_handlers;
#[path = "obligations/recursive_trees.rs"]
mod recursive_trees;
#[path = "obligations/sequences.rs"]
mod sequences;
#[path = "obligations/substitute.rs"]
mod substitute;
#[path = "obligations/tree_iteration.rs"]
mod tree_iteration;
pub(super) use gate::{check, check_recovery, templates};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Concrete,
    Template,
    Editor,
}

const WORK_LIMIT: usize = 400_000;
const DEPTH_LIMIT: usize = 128;

#[derive(Debug)]
struct Origin {
    aggregate: bool,
    input: Option<usize>,
    span: Span,
    exists: Predicate,
    handled: Predicate,
    returned: Predicate,
}
/// Finalized input duties remain distinguishable from locally produced obligations.
#[derive(Debug)]
struct Summary {
    origins: Vec<Origin>,
    inputs: Vec<Value>,
    work: usize,
    exits: Predicate,
    breaks: Predicate,
    iteration_returns: Vec<(Predicate, Value)>,
    output: Value,
    predicates: Predicates,
    pending_callbacks: BTreeMap<usize, Predicate>,
    #[cfg(test)]
    handled_input_count: usize,
    #[cfg(test)]
    borrowed_input_count: usize,
}
#[cfg(test)]
impl Summary {
    /// Count universally acknowledged parameter duties; partial views never count.
    fn handled_inputs(&self) -> usize {
        self.handled_input_count
    }
    /// Borrowing remains explicit when any reachable return leaves input responsibility retained.
    fn borrowed_inputs(&self) -> usize {
        self.borrowed_input_count
    }
    /// Keep the bounded predicate graph alongside conditional return/input provenance.
    fn decision_nodes(&self) -> usize {
        self.predicates.node_count()
    }
}
#[derive(Clone)]
struct Cleanup {
    guard: Predicate,
    function: usize,
    captures: Vec<Value>,
    span: Span,
}
struct Engine<'a> {
    program: &'a ir::Program,
    source_function: Option<usize>,
    locals: HashMap<usize, Value>,
    parameters: HashMap<usize, usize>,
    used_inputs: HashSet<usize>,
    origins: Vec<Origin>,
    work: usize,
    nodes: usize,
    predicates: Predicates,
    path: Predicate,
    exits: Predicate,
    cleanups: Vec<Cleanup>,
    returned_values: Vec<(Predicate, Value)>,
    inputs: Vec<Value>,
    summaries: Option<&'a calls::Summaries>,
    relevance: Option<&'a HashSet<usize>>,
    mode: Mode,
    input_callable: bool,
    iteration_breaks: Option<Predicate>,
    iteration_returns: Vec<(Predicate, Value)>,
    pending_callbacks: BTreeMap<usize, Predicate>,
    effect_cache: Rc<RefCell<effect_shapes::Cache>>,
    partitions: HashMap<usize, partitions::Partition>,
    sequence_offsets: HashMap<usize, (usize, usize)>,
    active_nominals: Vec<(usize, usize)>,
    nominal_roots: HashMap<usize, usize>,
    nominal_descendants: HashMap<usize, HashSet<usize>>,
    tree_context: Option<(usize, usize, usize)>,
}

/// Analyze supported typed bodies while refusing forms without a complete handling proof.
#[cfg(test)]
fn analyze_function(program: &ir::Program, function: &ir::Function) -> Checked<Summary> {
    calls::analyze(program, function)
}
/// Record the original input graph alongside the checked effects and all returned output paths.
fn analyze_body(engine: Engine<'_>, function: &ir::Function) -> Checked<Summary> {
    analyze_shaped_body(engine, function, &[])
}
/// Callable shapes refine effects while scalar captures remain independent formal input regions.
fn analyze_shaped_body(
    mut engine: Engine<'_>,
    function: &ir::Function,
    shapes: &[effect_shapes::Shape],
) -> Checked<Summary> {
    engine.source_function = Some(function.id.0);
    engine.charge(
        function
            .params
            .len()
            .saturating_add(function.captures.len()),
        function.body.span,
    )?;
    for (index, param) in function.params.iter().chain(&function.captures).enumerate() {
        let value = engine.fresh_shaped(
            &param.ty,
            Some(index),
            shapes.get(index),
            function.body.span,
            0,
        )?;
        engine.inputs.push(value.clone());
        engine.locals.insert(param.id.0, value);
        engine.parameters.insert(param.id.0, index);
    }
    let value = engine.expression(&function.body, 0)?;
    engine.exit(&value, function.body.span, 0)?;
    let output = engine.output(function.body.span)?;
    engine.finish(output)
}
impl<'a> Engine<'a> {
    /// Begin one bounded function proof with an unconditional reachable entry.
    fn new(program: &'a ir::Program) -> Self {
        Self {
            program,
            source_function: None,
            locals: HashMap::new(),
            parameters: HashMap::new(),
            used_inputs: HashSet::new(),
            origins: Vec::new(),
            work: 0,
            nodes: 0,
            predicates: Predicates::default(),
            path: Predicate::TRUE,
            exits: Predicate::FALSE,
            cleanups: Vec::new(),
            returned_values: Vec::new(),
            inputs: Vec::new(),
            summaries: None,
            relevance: None,
            mode: Mode::Concrete,
            input_callable: false,
            iteration_breaks: None,
            iteration_returns: Vec::new(),
            pending_callbacks: BTreeMap::new(),
            effect_cache: Rc::new(RefCell::new(effect_shapes::Cache::default())),
            partitions: HashMap::new(),
            sequence_offsets: HashMap::new(),
            active_nominals: Vec::new(),
            nominal_roots: HashMap::new(),
            nominal_descendants: HashMap::new(),
            tree_context: None,
        }
    }
    /// Bound every traversal, allocation and copied edge before doing its work.
    fn charge(&mut self, amount: usize, span: Span) -> Checked<()> {
        charge(&mut self.work, amount, span)?;
        Ok(())
    }
    /// Region creation is shared by aliases and carries a stable analysis-only identity.
    fn node(&mut self, kind: Region, span: Span) -> Checked<Value> {
        self.charge(1, span)?;
        let id = self.nodes;
        self.nodes += 1;
        Ok(Value {
            node: Rc::new(values::Node { id, kind }),
            complete: true,
        })
    }
    /// An outer Result construction is separate from all Results in its payloads.
    fn origin(&mut self, input: Option<usize>, span: Span) -> Checked<usize> {
        self.charge(1, span)?;
        let id = self.origins.len();
        self.origins.push(Origin {
            aggregate: false,
            input,
            span,
            exists: self.path,
            handled: Predicate::FALSE,
            returned: Predicate::FALSE,
        });
        Ok(id)
    }
    /// Preserve the existing unused-input rule independently of borrowing summaries.
    fn finish(mut self, output: Value) -> Checked<Summary> {
        #[cfg(test)]
        let mut handled_input_count = 0;
        #[cfg(test)]
        let mut borrowed_input_count = 0;
        for (id, origin) in self.origins.iter().enumerate() {
            if origin.input.is_some_and(|i| !self.used_inputs.contains(&i)) {
                return Err(Diagnostic::new(
                    origin.span,
                    "Result binding parameter is never used",
                ));
            }
            let required =
                self.predicates
                    .and(origin.exists, self.exits, &mut self.work, origin.span)?;
            let disposed =
                self.predicates
                    .or(origin.handled, origin.returned, &mut self.work, origin.span)?;
            let complete =
                self.predicates
                    .implies(required, disposed, &mut self.work, origin.span)?;
            let possible = self
                .pending_callbacks
                .get(&id)
                .copied()
                .unwrap_or(Predicate::FALSE);
            let possible = self
                .predicates
                .or(disposed, possible, &mut self.work, origin.span)?;
            let conditional = self.input_callable
                && self
                    .predicates
                    .implies(required, possible, &mut self.work, origin.span)?;
            if origin.input.is_none() && !complete && !conditional {
                return Err(Diagnostic::new(
                    origin.span,
                    "Result obligation remains unhandled on this exit",
                ));
            }
            #[cfg(test)]
            if origin.input.is_some() && required != Predicate::FALSE {
                handled_input_count += usize::from(self.predicates.implies(
                    required,
                    origin.handled,
                    &mut self.work,
                    origin.span,
                )?);
                borrowed_input_count += usize::from(!complete);
            }
        }
        Ok(Summary {
            origins: self.origins,
            inputs: self.inputs,
            work: self.work,
            exits: self.exits,
            breaks: self.iteration_breaks.unwrap_or(Predicate::FALSE),
            iteration_returns: self.iteration_returns,
            output,
            predicates: self.predicates,
            pending_callbacks: self.pending_callbacks,
            #[cfg(test)]
            handled_input_count,
            #[cfg(test)]
            borrowed_input_count,
        })
    }
    /// Unsupported proof forms fail closed until their separate semantic stage is implemented.
    fn unsupported<T>(&self, span: Span) -> Checked<T> {
        Err(Diagnostic::new(
            span,
            "Result obligation analysis does not yet prove this operation",
        ))
    }
}

/// One shared budget covers region work and all path-predicate operations.
fn charge(work: &mut usize, amount: usize, span: Span) -> Checked<()> {
    *work = work
        .checked_add(amount)
        .filter(|n| *n <= WORK_LIMIT)
        .ok_or_else(|| Diagnostic::new(span, "Result obligation analysis work limit exceeded"))?;
    Ok(())
}
