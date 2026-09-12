//! Complete collection traversal handles Result duties; metadata and partial visits only borrow.
use fern_compiler::{check, parse};

/// Parse every fixture independently and reject only for an explicit Result obligation.
fn cases(fixtures: &[(&str, bool, &str)]) {
    let mut failures = Vec::new();
    for (name, accepted, source) in fixtures {
        let program = parse::parse(source)
            .unwrap_or_else(|error| panic!("{name}: invalid fixture: {error:?}"));
        match check::check(&program) {
            Ok(_) if !accepted => {
                failures.push(format!("{name}: silently accepted unhandled Result duties"))
            }
            Err(error) if *accepted => failures.push(format!(
                "{name}: rejected complete handling: {}",
                error.message
            )),
            Err(error) if !error.message.contains("Result") => failures.push(format!(
                "{name}: rejected for an unrelated reason: {}",
                error.message
            )),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn dynamic_list_metadata_is_borrowing_until_complete_traversal() {
    cases(&[
        (
            "full",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    for r in values:
        println(observe(r))
"#,
        ),
        (
            "metadata",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(List.len(values))
"#,
        ),
        (
            "metadata_then_full",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(List.len(values))
    for r in values:
        println(observe(r))
"#,
        ),
    ]);
}

#[test]
fn map_values_preserve_and_fully_traverse_original_obligations() {
    cases(&[
        (
            "map_full",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: Map(String, Result(Int, String)) = %{"a": Ok(1), "b": Err("later")}
    for r in Map.values(values):
        println(observe(r))
"#,
        ),
        (
            "map_metadata",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: Map(String, Result(Int, String)) = %{"a": Ok(1), "b": Err("later")}
    println(List.len(Map.values(values)))
"#,
        ),
    ]);
}

#[test]
fn head_and_index_do_not_acknowledge_the_remaining_dynamic_elements() {
    cases(&[
        (
            "head",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(observe(List.head(values)))
"#,
        ),
        (
            "get",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(observe(List.get(values, 0)))
"#,
        ),
        (
            "head_then_full",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(observe(List.head(values)))
    for r in values:
        println(observe(r))
"#,
        ),
    ]);
}

#[test]
fn short_circuit_search_predicates_do_not_establish_complete_handling() {
    cases(&[
        (
            "any",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(List.any(values, observe))
"#,
        ),
        (
            "all",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(List.all(values, observe))
"#,
        ),
        (
            "find",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    match List.find(values, observe):
        Some(r) -> println(observe(r))
        None -> ()
"#,
        ),
        (
            "search_then_full",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(List.any(values, observe))
    for r in values:
        println(observe(r))
"#,
        ),
    ]);
}

#[test]
fn early_break_can_be_followed_by_an_independent_complete_traversal() {
    cases(&[
        (
            "break_only",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    for r in values:
        println(observe(r))
        break
"#,
        ),
        (
            "break_then_full",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    for r in values:
        println(observe(r))
        break
    for r in values:
        println(observe(r))
"#,
        ),
    ]);
}

#[test]
fn continue_and_return_cannot_skip_fresh_per_iteration_results() {
    cases(&[
        (
            "continue",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    for value in [1, 2]:
        let r: Result(Int, String) = Err("iteration")
        continue if value == 1
        println(observe(r))
"#,
        ),
        (
            "return",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    for value in [1, 2]:
        let r: Result(Int, String) = Err("iteration")
        return () if value == 1
        println(observe(r))
"#,
        ),
        (
            "handled_before_continue",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    for value in [1, 2]:
        let r: Result(Int, String) = Err("iteration")
        println(observe(r))
        continue if value == 1
"#,
        ),
    ]);
}

#[test]
fn handling_outer_tags_leaves_nested_payload_results_pending() {
    cases(&[
        (
            "outer_only",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: List(Result(Result(Int, String), String)) = [Ok(Err("nested"))]
    for r in values:
        println(Result.is_ok(r))
"#,
        ),
        (
            "every_layer",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: List(Result(Result(Int, String), String)) = [Ok(Err("nested"))]
    for r in values:
        match r:
            Ok(inner) -> println(observe(inner))
            Err(message) -> println(message)
"#,
        ),
    ]);
}

#[test]
fn sequence_prefix_and_rest_are_disjoint_parts_of_one_obligation() {
    cases(&[
        (
            "prefix_only",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    match values:
        [] -> ()
        [first, ..rest] ->
            println(observe(first))
            println(List.len(rest))
"#,
        ),
        (
            "prefix_and_rest",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    match values:
        [] -> ()
        [first, ..rest] ->
            println(observe(first))
            for r in rest:
                println(observe(r))
"#,
        ),
    ]);
}

#[test]
fn map_overwrite_and_delete_do_not_erase_original_value_duties() {
    cases(&[
        (
            "overwrite",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: Map(String, Result(Int, String)) = %{"a": Ok(1), "b": Err("later")}
    let changed = Map.put(values, "b", Ok(9))
    for r in Map.values(changed):
        println(observe(r))
"#,
        ),
        (
            "overwrite_then_original",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: Map(String, Result(Int, String)) = %{"a": Ok(1), "b": Err("later")}
    let changed = Map.put(values, "b", Ok(9))
    for r in Map.values(changed):
        println(observe(r))
    for r in Map.values(values):
        println(observe(r))
"#,
        ),
        (
            "delete",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: Map(String, Result(Int, String)) = %{"a": Ok(1), "b": Err("later")}
    let changed = Map.delete(values, "b")
    for r in Map.values(changed):
        println(observe(r))
"#,
        ),
        (
            "delete_then_original",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values: Map(String, Result(Int, String)) = %{"a": Ok(1), "b": Err("later")}
    let changed = Map.delete(values, "b")
    for r in Map.values(changed):
        println(observe(r))
    for r in Map.values(values):
        println(observe(r))
"#,
        ),
    ]);
}

#[test]
fn mapping_identity_borrows_while_complete_callbacks_can_handle_every_element() {
    cases(&[
        (
            "identity_metadata",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn identity(r: Result(Int, String)) -> Result(Int, String): r
fn main():
    let values = items(flag: System.args_count() > 0)
    let copied = List.map(values, identity)
    println(List.len(copied))
"#,
        ),
        (
            "identity_full",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn identity(r: Result(Int, String)) -> Result(Int, String): r
fn main():
    let values = items(flag: System.args_count() > 0)
    let copied = List.map(values, identity)
    for r in copied:
        println(observe(r))
"#,
        ),
        (
            "handling_map",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    println(List.len(List.map(values, observe)))
"#,
        ),
    ]);
}

#[test]
fn filter_predicates_visit_all_elements_but_filtered_outputs_do_not_cover_rejected_inputs() {
    cases(&[
        (
            "borrowed_filter",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn reject(r: Result(Int, String)) -> Bool:
    let boxed = [r]
    List.len(boxed) == 0
fn main():
    let values = items(flag: System.args_count() > 0)
    let selected = List.filter(values, reject)
    for r in selected:
        println(observe(r))
"#,
        ),
        (
            "handling_filter",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn main():
    let values = items(flag: System.args_count() > 0)
    let selected = List.filter(values, observe)
    println(List.len(selected))
"#,
        ),
    ]);
}

#[test]
fn fold_preserves_replaced_accumulator_and_zero_iteration_obligations() {
    cases(&[
        (
            "replaced_accumulator",
            false,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn replace(acc: List(Result(Int, String)), r: Result(Int, String)) -> List(Result(Int, String)):
    println(List.len(acc))
    println(observe(r))
    []
fn main():
    let values = items(flag: System.args_count() > 0)
    let initial: List(Result(Int, String)) = [Err("initial")]
    let final = List.fold(values, initial, replace)
    for r in final:
        println(observe(r))
"#,
        ),
        (
            "handling_fold",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn sum(acc: Int, r: Result(Int, String)) -> Int:
    if observe(r): acc + 1 else: acc
fn main():
    let values = items(flag: System.args_count() > 0)
    println(List.fold(values, 0, sum))
"#,
        ),
        (
            "empty_fold_returns_initial",
            true,
            r#"fn items(flag: Bool) -> List(Result(Int, String)):
    if flag: [Ok(1)] else: [Ok(1), Err("later"), Err("last")]
fn observe(r: Result(Int, String)) -> Bool: Result.is_ok(r)
fn replace(acc: List(Result(Int, String)), r: Result(Int, String)) -> List(Result(Int, String)):
    println(List.len(acc))
    println(observe(r))
    []
fn main():
    let empty: List(Result(Int, String)) = []
    let initial: List(Result(Int, String)) = [Err("initial")]
    let final = List.fold(empty, initial, replace)
    for r in final:
        println(observe(r))
"#,
        ),
    ]);
}
