//! Bounded control-flow dominance checks, including predecessor-specific phi uses.
use super::*;
use std::collections::BTreeMap;
const MAX_BLOCKS: usize = 65_536;
const MAX_WORK: usize = 8_000_000;
const ABSENT: usize = usize::MAX;
struct Graph<'a> {
    labels: BTreeMap<&'a str, usize>,
    starts: Vec<usize>,
    ends: Vec<usize>,
    successors: Vec<Vec<usize>>,
    predecessors: Vec<Vec<usize>>,
    definitions: BTreeMap<&'a str, (usize, usize)>,
}
/// Validate definitions against reachable dominators and same-block source order.
pub(super) fn function(function: &Function, work: &mut usize) -> Result<(), String> {
    let graph = Graph::new(function)?;
    let (idom, rank) = graph.dominators(work)?;
    let mut block = 0;
    for (index, statement) in function.body.iter().enumerate() {
        if let Statement::Label(label) = statement {
            block = graph.labels[bare(label)];
            continue;
        }
        if let Statement::Assign {
            operation: Operation::Phi(incoming),
            ..
        } = statement
        {
            for (label, value) in incoming {
                let predecessor = graph.labels[bare(label)];
                graph.check(
                    value,
                    predecessor,
                    graph.ends[predecessor],
                    &idom,
                    &rank,
                    work,
                )?;
            }
        } else {
            for value in operands(statement) {
                graph.check(value, block, index, &idom, &rank, work)?;
            }
        }
    }
    Ok(())
}
impl<'a> Graph<'a> {
    /// Build linear-size adjacency and definition maps after structural validation.
    fn new(function: &'a Function) -> Result<Self, String> {
        let mut graph = Self {
            labels: BTreeMap::new(),
            starts: vec![],
            ends: vec![],
            successors: vec![],
            predecessors: vec![],
            definitions: BTreeMap::new(),
        };
        for (_, name) in &function.params {
            graph.definitions.insert(bare(name), (ABSENT, 0));
        }
        let mut block = 0;
        for (index, statement) in function.body.iter().enumerate() {
            match statement {
                Statement::Label(label) => {
                    block = graph.starts.len();
                    graph.labels.insert(bare(label), block);
                    graph.starts.push(index);
                }
                Statement::Assign { destination, .. } => {
                    graph.definitions.insert(bare(destination), (block, index));
                }
                _ => {}
            }
        }
        if graph.starts.len() > MAX_BLOCKS {
            return Err("machine dominance block limit exceeded".into());
        }
        graph.successors.resize_with(graph.starts.len(), Vec::new);
        graph.predecessors.resize_with(graph.starts.len(), Vec::new);
        for index in 0..graph.starts.len() {
            let end = graph
                .starts
                .get(index + 1)
                .copied()
                .unwrap_or(function.body.len())
                - 1;
            graph.ends.push(end);
            match &function.body[end] {
                Statement::Jump(label) => graph.successors[index].push(graph.labels[bare(label)]),
                Statement::Branch {
                    then_label,
                    else_label,
                    ..
                } => {
                    graph.successors[index].push(graph.labels[bare(then_label)]);
                    if bare(else_label) != bare(then_label) {
                        graph.successors[index].push(graph.labels[bare(else_label)]);
                    }
                }
                _ => {}
            }
            for target in &graph.successors[index] {
                graph.predecessors[*target].push(index);
            }
        }
        if !graph.predecessors[0].is_empty() {
            return Err("machine entry block has incoming control edges".into());
        }
        Ok(graph)
    }
    /// Compute reverse postorder iteratively, avoiding the native call stack on deep CFGs.
    fn reverse_postorder(&self) -> Vec<usize> {
        let mut seen = vec![false; self.starts.len()];
        let mut pending = vec![(0, false)];
        let mut postorder = vec![];
        while let Some((node, expanded)) = pending.pop() {
            if expanded {
                postorder.push(node);
                continue;
            }
            if seen[node] {
                continue;
            }
            seen[node] = true;
            pending.push((node, true));
            for successor in self.successors[node].iter().rev() {
                if !seen[*successor] {
                    pending.push((*successor, false));
                }
            }
        }
        postorder.reverse();
        postorder
    }
    /// Cooper-style immediate dominators use linear memory and a strict aggregate work budget.
    fn dominators(&self, work: &mut usize) -> Result<(Vec<usize>, Vec<usize>), String> {
        let order = self.reverse_postorder();
        let mut rank = vec![ABSENT; self.starts.len()];
        for (index, node) in order.iter().enumerate() {
            rank[*node] = index;
        }
        let mut idom = vec![ABSENT; self.starts.len()];
        idom[0] = 0;
        let mut changed = true;
        while changed {
            changed = false;
            for node in order.iter().skip(1).copied() {
                spend(work)?;
                let mut parent = ABSENT;
                for predecessor in &self.predecessors[node] {
                    spend(work)?;
                    if idom[*predecessor] == ABSENT {
                        continue;
                    }
                    parent = if parent == ABSENT {
                        *predecessor
                    } else {
                        intersect(parent, *predecessor, &idom, &rank, work)?
                    };
                }
                if idom[node] != parent {
                    idom[node] = parent;
                    changed = true;
                }
            }
        }
        Ok((idom, rank))
    }
    /// Phi operands are checked at predecessor terminators, never at the merge itself.
    fn check(
        &self,
        value: &Operand,
        block: usize,
        index: usize,
        idom: &[usize],
        rank: &[usize],
        work: &mut usize,
    ) -> Result<(), String> {
        let Operand::Temp(name) = value else {
            return Ok(());
        };
        let (definition, position) = self.definitions[bare(name)];
        if definition == ABSENT {
            return Ok(());
        }
        if definition == block {
            return if position < index {
                Ok(())
            } else {
                Err(format!("machine definition does not dominate use: {name}"))
            };
        }
        // Dominance is vacuous in unreachable code; physical types and local ordering still apply.
        if rank[block] == ABSENT {
            return Ok(());
        }
        let mut cursor = block;
        while cursor != definition && cursor != 0 && rank[cursor] > rank[definition] {
            spend(work)?;
            cursor = idom[cursor];
        }
        if cursor != definition {
            return Err(format!("machine definition does not dominate use: {name}"));
        }
        Ok(())
    }
}
/// Intersect immediate-dominator chains, charging every potentially repeated ascent.
fn intersect(
    mut left: usize,
    mut right: usize,
    idom: &[usize],
    rank: &[usize],
    work: &mut usize,
) -> Result<usize, String> {
    while left != right {
        spend(work)?;
        if rank[left] > rank[right] {
            left = idom[left];
        } else {
            right = idom[right];
        }
    }
    Ok(left)
}
/// Reject adversarial CFG analysis before quadratic iteration can dominate compilation.
fn spend(work: &mut usize) -> Result<(), String> {
    *work += 1;
    if *work > MAX_WORK {
        Err("machine dominance work limit exceeded".into())
    } else {
        Ok(())
    }
}
/// Enumerate real value uses; phi operands have their own predecessor context.
fn operands(statement: &Statement) -> Vec<&Operand> {
    match statement {
        Statement::Assign { operation, .. } | Statement::Effect(operation) => match operation {
            Operation::Unary(_, value) | Operation::Load(_, value) => vec![value],
            Operation::Binary(_, left, right) => vec![left, right],
            Operation::Call { callee, args, .. } => std::iter::once(callee)
                .chain(args.iter().map(|(_, value)| value))
                .collect(),
            _ => vec![],
        },
        Statement::Store { value, address, .. } => vec![value, address],
        Statement::Branch { condition, .. } => vec![condition],
        Statement::Return(value) => value.iter().collect(),
        _ => vec![],
    }
}
