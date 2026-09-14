use super::*;

fn graph(source: &str) -> Graph {
    analyze(&crate::parse::parse(source).unwrap()).unwrap()
}
fn edges(graph: &Graph, name: &str) -> Vec<String> {
    graph
        .groups
        .iter()
        .find(|g| g.name == name)
        .unwrap()
        .callees
        .iter()
        .map(|index| graph.groups[*index].name.clone())
        .collect()
}
fn components(graph: &Graph) -> Vec<Vec<String>> {
    graph
        .components
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|index| graph.groups[*index].name.clone())
                .collect()
        })
        .collect()
}

#[test]
fn forward_mutual_and_self_recursion_are_callee_first_and_deterministic() {
    let source = "fn first(): second()\nfn second(): third()\nfn third(): second()\nfn alone(): alone()\nfn main(): first()\n";
    let result = graph(source);
    assert_eq!(
        components(&result),
        vec![
            vec!["second", "third"],
            vec!["first"],
            vec!["alone"],
            vec!["main"]
        ]
    );
    assert_eq!(edges(&result, "alone"), vec!["alone"]);
    assert_eq!(components(&result), components(&graph(source)));
}

#[test]
fn clause_groups_retain_source_identity_and_independent_parameter_bindings() {
    let source = "fn f(0: Int) if helper() -> Int: other()\nfn f(helper: Int) -> Int: helper()\nfn helper(): 0\nfn other(): 0\n";
    let syntax = crate::parse::parse(source).unwrap();
    let result = analyze(&syntax).unwrap();
    assert_eq!(result.groups.len(), 3);
    assert_eq!(result.groups[0].clauses, 0..2);
    assert_eq!(
        result.groups[0].group_start,
        syntax.functions[0].group_start
    );
    assert_eq!(result.groups[0].span, syntax.functions[0].span);
    assert_eq!(edges(&result, "f"), vec!["helper", "other"]);
}

#[test]
fn initializer_and_nested_block_shadowing_follow_lexical_scope() {
    let result = graph(
        "fn target(): 0\nfn use():\n    let target = target\n    target()\nfn nested():\n    if true:\n        let target = 0\n        target()\n    target()\nfn shadow(target: Int): target()\n",
    );
    assert_eq!(edges(&result, "use"), vec!["target"]);
    assert_eq!(edges(&result, "nested"), vec!["target"]);
    assert!(edges(&result, "shadow").is_empty());
}

#[test]
fn lambdas_defer_pipes_and_first_class_references_include_globals_only() {
    let result = graph(
        "fn target(): 0\nfn other(): 0\nfn use():\n    let closure = (target) -> other(target())\n    defer other()\n    0 |> target(other)\nfn shadow(target: Int):\n    let closure = () -> target()\n    defer target()\n    0 |> target()\n",
    );
    assert_eq!(edges(&result, "use"), vec!["target", "other"]);
    assert!(edges(&result, "shadow").is_empty());
}

#[test]
fn match_and_for_patterns_bind_only_their_guards_and_bodies() {
    let result = graph(
        "fn target(): 0\nfn guard(): true\nfn use(xs: List(Int)):\n    match target():\n        target if guard() -> target()\n        _ -> 0\n    for target in target():\n        target()\n    target()\nfn shadow(xs: List(Int)):\n    match xs:\n        [target, ..rest] -> target()\n        [] -> 0\n    for (target, ..rest) in xs:\n        target()\n",
    );
    assert_eq!(edges(&result, "use"), vec!["target", "guard"]);
    assert!(edges(&result, "shadow").is_empty());
}

#[test]
fn with_success_bindings_do_not_enter_error_handlers() {
    let result = graph(
        "fn target(): Ok(0)\nfn other(): Ok(0)\nfn use():\n    with target <- target(), other <- target() do\n        other()\n    else\n        Err(error) -> other()\nfn shadow():\n    with target <- other() do\n        target()\n    else\n        Err(target) -> target()\n",
    );
    assert_eq!(edges(&result, "use"), vec!["target", "other"]);
    assert_eq!(edges(&result, "shadow"), vec!["other"]);
}

#[test]
fn let_else_and_pattern_initializers_see_the_outer_scope() {
    let result = graph(
        "fn target(): 0\nfn other(): 0\nfn use():\n    let Some(target) = other() else: return target()\n    target()\nfn nested():\n    let (target, other) = (target, other)\n    target()\n    other()\n",
    );
    assert_eq!(edges(&result, "use"), vec!["target", "other"]);
    assert_eq!(edges(&result, "nested"), vec!["target", "other"]);
}

#[test]
fn qualified_calls_respect_local_dotted_roots_without_string_scanning() {
    let mut syntax = crate::parse::parse("fn api(): 0\nfn use(): tools.api()\nfn shadow(tools: Int): tools.api()\nfn text(): \"tools.api() api()\"\n").unwrap();
    syntax.functions[0].name = "tools.api".into();
    let result = analyze(&syntax).unwrap();
    assert_eq!(edges(&result, "use"), vec!["tools.api"]);
    assert!(edges(&result, "shadow").is_empty());
    assert!(edges(&result, "text").is_empty());
}

#[test]
fn graph_traversal_is_iterative_at_4096_function_identities() {
    let mut source = String::new();
    for index in 0..4095 {
        source.push_str(&format!("fn f{index}(): f{}()\n", index + 1));
    }
    source.push_str("fn f4095(): 0\n");
    let result = graph(&source);
    assert_eq!(result.groups.len(), 4096);
    assert_eq!(result.components.first(), Some(&vec![4095]));
    assert_eq!(result.components.last(), Some(&vec![0]));
}

#[test]
fn aggregate_ast_and_edge_work_is_bounded() {
    let mut syntax = crate::parse::parse("fn target(): 0\nfn use(): target()\n").unwrap();
    let value = ast::Expr {
        kind: ast::ExprKind::Call {
            name: "target".into(),
            args: Vec::new(),
        },
        span: syntax.functions[1].span,
    };
    syntax.functions[1].body.kind = ast::ExprKind::Block(vec![ast::Stmt::Expr(value); 45_000]);
    let error = analyze(&syntax).unwrap_err();
    assert!(
        error.message.contains("dependency work limit"),
        "{}",
        error.message
    );
}

#[test]
fn nested_value_syntax_does_not_hide_function_references() {
    let result = graph(
        "fn a(): 0\nfn b(): 0\nfn c(): 0\nfn use():\n    let values = %{a: b}\n    let record = %{c() | value: a}\n    let text = \"#{b()}\"\n    let closure = () -> (() -> c())\n    (a)()\n",
    );
    assert_eq!(edges(&result, "use"), vec!["a", "b", "c"]);
}

#[test]
fn same_named_binders_in_one_clause_do_not_hide_other_clauses() {
    let result = graph(
        "fn f(target: Int) if false -> Int: target()\nfn f(0: Int) -> Int: target()\nfn target(): 0\n",
    );
    assert_eq!(edges(&result, "f"), vec!["target"]);
}

#[test]
fn invalid_group_identities_and_declaration_counts_are_diagnostics() {
    let mut syntax = crate::parse::parse("fn f(0) -> 0\nfn f(n) -> n\n").unwrap();
    syntax.functions[1].group_start += 1;
    assert!(analyze(&syntax).unwrap_err().message.contains("identity"));
    let mut syntax = crate::parse::parse("fn f(): 0\nfn other(): 0\n").unwrap();
    syntax.functions.push(syntax.functions[0].clone());
    assert!(analyze(&syntax).unwrap_err().message.contains("adjacent"));
    syntax.functions = vec![syntax.functions[0].clone(); 4097];
    assert!(
        analyze(&syntax)
            .unwrap_err()
            .message
            .contains("count limit")
    );
    assert!(
        analyze(&ast::Program::default())
            .unwrap()
            .components
            .is_empty()
    );
}

#[test]
fn an_entire_4096_node_component_never_recurses_on_the_host_stack() {
    let mut source = String::new();
    for index in 0..4096 {
        source.push_str(&format!("fn f{index}(): f{}()\n", (index + 1) % 4096));
    }
    let result = graph(&source);
    assert_eq!(result.components.len(), 1);
    assert_eq!(result.components[0], (0..4096).collect::<Vec<_>>());
}

#[test]
fn small_graph_components_match_an_independent_reachability_oracle() {
    let mut seed = 19_u64;
    for _ in 0..100 {
        let mut source = String::new();
        let mut reach = vec![vec![false; 8]; 8];
        for (caller, row) in reach.iter_mut().enumerate() {
            source.push_str(&format!("fn f{caller}():\n"));
            for (callee, edge) in row.iter_mut().enumerate() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                if seed >> 61 == 0 {
                    *edge = true;
                    source.push_str(&format!("    f{callee}()\n"));
                }
            }
            source.push_str("    0\n");
        }
        let result = graph(&source);
        for middle in 0..8 {
            for from in 0..8 {
                for to in 0..8 {
                    reach[from][to] |= reach[from][middle] && reach[middle][to];
                }
            }
        }
        let mut component_of = [0; 8];
        for (index, members) in result.components.iter().enumerate() {
            for &member in members {
                component_of[member] = index;
            }
        }
        for from in 0..8 {
            for to in 0..8 {
                assert_eq!(
                    component_of[from] == component_of[to],
                    from == to || (reach[from][to] && reach[to][from])
                );
                if reach[from][to] {
                    assert!(component_of[from] >= component_of[to]);
                }
            }
        }
    }
}

#[test]
fn dense_edges_share_the_same_budget_as_ast_visits_and_scc_work() {
    let mut source = String::new();
    for index in 0..160 {
        source.push_str(&format!("fn f{index}(): 0\n"));
    }
    let mut syntax = crate::parse::parse(&source).unwrap();
    for function in &mut syntax.functions {
        let calls = (0..160)
            .map(|index| {
                ast::Stmt::Expr(ast::Expr {
                    kind: ast::ExprKind::Call {
                        name: format!("f{index}"),
                        args: Vec::new(),
                    },
                    span: function.span,
                })
            })
            .collect();
        function.body.kind = ast::ExprKind::Block(calls);
    }
    let error = analyze(&syntax).unwrap_err();
    assert!(
        error.message.contains("dependency work limit"),
        "{}",
        error.message
    );
}
