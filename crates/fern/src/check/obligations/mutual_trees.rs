//! Finite SCCs admit simultaneous induction only when every member is a complete tree handler.
use super::*;
/// Build one bounded graph over the current unfinished dependency closure.
pub(super) fn groups(
    program: &ir::Program,
    functions: &[&ir::Function],
    work: &mut usize,
) -> Checked<Vec<Vec<(usize, usize)>>> {
    if functions.len() < 2 {
        return Ok(Vec::new());
    }
    let span = functions
        .first()
        .map_or_else(Span::default, |f| f.body.span);
    charge(work, functions.len(), span)?;
    let index: HashMap<_, _> = functions
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.0, i))
        .collect();
    let mut edges = vec![Vec::new(); functions.len()];
    for (caller, function) in functions.iter().enumerate() {
        for callee in calls::ordered_dependencies(program, &function.body, work)? {
            charge(work, 1, function.body.span)?;
            if let Some(&callee) = index.get(&callee) {
                edges[caller].push(callee);
            }
        }
    }
    let mut output = Vec::new();
    for component in components(&edges, work, span)? {
        if component.len() < 2 {
            continue;
        }
        let mut handlers = Vec::new();
        for member in component {
            let function = functions[member];
            let Some(parameter) = recursive_trees::candidate(program, function, work)? else {
                handlers.clear();
                break;
            };
            charge(work, 1, span)?;
            handlers.push((function.id.0, parameter));
        }
        if !handlers.is_empty() {
            output.push(handlers);
        }
    }
    Ok(output)
}
/// Explicit DFS frames avoid recursive host-stack growth for large source function cycles.
fn finish_order(edges: &[Vec<usize>], work: &mut usize, span: Span) -> Checked<Vec<usize>> {
    charge(work, edges.len(), span)?;
    let mut seen = vec![false; edges.len()];
    let mut finish = Vec::new();
    for start in 0..edges.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut pending = vec![(start, 0)];
        while let Some((node, cursor)) = pending.last_mut() {
            charge(work, 1, span)?;
            if let Some(&next) = edges[*node].get(*cursor) {
                *cursor += 1;
                if !seen[next] {
                    seen[next] = true;
                    pending.push((next, 0));
                }
            } else {
                finish.push(*node);
                pending.pop();
            }
        }
    }
    Ok(finish)
}
/// Reverse reachability collects exact components, including cross edges to finished DFS nodes.
fn components(edges: &[Vec<usize>], work: &mut usize, span: Span) -> Checked<Vec<Vec<usize>>> {
    let finish = finish_order(edges, work, span)?;
    charge(work, edges.len(), span)?;
    let mut incoming = vec![Vec::new(); edges.len()];
    for (caller, children) in edges.iter().enumerate() {
        for &callee in children {
            charge(work, 1, span)?;
            incoming[callee].push(caller);
        }
    }
    let mut seen = vec![false; edges.len()];
    let mut output = Vec::new();
    for start in finish.into_iter().rev() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut component = Vec::new();
        let mut pending = vec![start];
        while let Some(node) = pending.pop() {
            charge(work, 1, span)?;
            component.push(node);
            for &next in &incoming[node] {
                charge(work, 1, span)?;
                if !seen[next] {
                    seen[next] = true;
                    pending.push(next);
                }
            }
        }
        output.push(component);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    mod corpus {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/mutual_tree_cases.rs"
        ));
    }
    #[test]
    fn every_negative_source_is_independently_valid_without_result_proof() {
        for (name, _, source) in corpus::cases() {
            let ast = crate::parse::parse(&source).unwrap();
            super::super::super::pipeline_mode(&ast, |_, _, _| Ok(()), false)
                .unwrap_or_else(|e| panic!("{name}: {e:?}\n{source}"));
        }
    }
    #[test]
    fn small_components_match_independent_reachability() {
        let mut work = 0;
        for bits in 0..512usize {
            let edges: Vec<Vec<usize>> = (0..3)
                .map(|a| (0..3).filter(|b| bits & (1 << (a * 3 + b)) != 0).collect())
                .collect();
            let actual = components(&edges, &mut work, Span::default()).unwrap();
            let mut reachable = [[false; 3]; 3];
            for a in 0..3 {
                reachable[a][a] = true;
                for &b in &edges[a] {
                    reachable[a][b] = true;
                }
            }
            for k in 0..3 {
                for a in 0..3 {
                    for b in 0..3 {
                        reachable[a][b] |= reachable[a][k] && reachable[k][b];
                    }
                }
            }
            for a in 0..3 {
                for b in 0..3 {
                    assert_eq!(
                        actual.iter().any(|c| c.contains(&a) && c.contains(&b)),
                        reachable[a][b] && reachable[b][a]
                    );
                }
            }
        }
    }
    #[test]
    fn graph_stack_is_iterative_and_charges_before_allocating() {
        let edges: Vec<Vec<usize>> = (0..4096).map(|i| vec![(i + 1) % 4096]).collect();
        let groups = components(&edges, &mut 0, Span::default()).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 4096);
        let error = components(&edges, &mut (WORK_LIMIT - 1), Span::default()).unwrap_err();
        assert!(error.message.contains("work limit"));
    }
}
