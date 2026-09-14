//! Known callbacks keep source semantics while removing the per-element call.
use super::*;

fn checked(source: &str) -> morrow_compiler::ir::Program {
    morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap()
}

fn indirect_calls(function: &Function) -> usize {
    function
        .body
        .iter()
        .filter(|statement| {
            matches!(
                statement,
                Statement::Assign {
                    operation: Operation::Call {
                        callee: Operand::Temp(_),
                        ..
                    },
                    ..
                } | Statement::Effect(Operation::Call {
                    callee: Operand::Temp(_),
                    ..
                })
            )
        })
        .count()
}

#[test]
fn short_known_collection_callbacks_are_inlined_but_dynamic_callbacks_remain_calls() {
    let source = r#"
fn known(values: List(Int), delta: Int) -> Int:
    let mapped = List.map(values, (value) -> value + delta)
    let filtered = List.filter(mapped, (value) -> value > delta)
    List.fold(filtered, 0, (total, value) -> total + value)
fn dynamic(values: List(Int), callback: fn(Int) -> Int) -> List(Int):
    List.map(values, callback)
fn main(): ()
"#;
    let checked = checked(source);
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    for (name, expected) in [("known", 0), ("dynamic", 1)] {
        let id = checked
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .id
            .0;
        let function = program
            .functions
            .iter()
            .find(|f| machine::bare(&f.name) == format!("f{id}"))
            .unwrap();
        assert_eq!(
            indirect_calls(function),
            expected,
            "callback calls in {name}"
        );
    }
}

#[test]
fn oversized_known_callback_keeps_bounded_codegen_and_its_ordinary_result() {
    let mut source = String::from("fn main():\n    let mapped = List.map([1, 2], (value) ->\n");
    for index in 0..40 {
        let previous = if index == 0 {
            "value".into()
        } else {
            format!("step{}", index - 1)
        };
        source.push_str(&format!("        let step{index} = {previous} + 1\n"));
    }
    source.push_str("        step39\n    )\n    println(List.fold(mapped, 0, (total, value) -> total + value))\n");
    let checked = checked(&source);
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id
        .0;
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == format!("f{id}"))
        .unwrap();
    assert_eq!(
        indirect_calls(function),
        1,
        "large map body remains a call; short fold body inlines"
    );
    assert_eq!(NativeFixture::new().execute_linked(&program,
        "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }",
        &[core_runtime_archive().into_os_string()]), b"83\n");
}

#[test]
fn inlined_callbacks_keep_capture_order_precise_roots_aliases_and_full_width_payloads() {
    let source = r#"
type Cell:
    wide: Int
    label: String
    fraction: Float
fn input() -> List(Cell):
    println("input once")
    [Cell(9007199254740993, String.repeat("left", 1), 0.5), Cell(-9007199254740993, String.repeat("right", 1), 1.5)]
fn capture() -> String:
    println("capture once")
    String.repeat("λ雪", 1)
fn main():
    let original = input()
    let suffix = capture()
    let quarter = 0.25
    let mapped = List.map(original, (cell) ->
        let suffix = suffix
        Cell(cell.wide + 1, cell.label + suffix, cell.fraction + quarter)
    )
    let selected = List.filter(mapped, (cell) -> cell.wide > 0)
    println(List.fold(selected, 0, (total, cell) -> total + cell.wide))
    for cell in mapped:
        println(cell.label)
        println(cell.fraction)
    for cell in original:
        println(cell.wide)
        println(cell.label)
        println(cell.fraction)
"#;
    let checked = checked(source);
    let main = checked
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id
        .0;
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == format!("f{main}"))
        .unwrap();
    assert_eq!(indirect_calls(function), 0, "exercise actual inline body");
    let mut allocations = 0;
    let mut body = Vec::new();
    for statement in std::mem::take(&mut function.body) {
        if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "morrow_alloc")
        {
            body.push(Statement::Effect(Operation::Call {
                callee: symbol("morrow_gc_collect_precise"),
                args: vec![],
                variadic: None,
            }));
            allocations += 1;
        }
        body.push(statement);
    }
    assert!(
        allocations >= 2,
        "collect before closure and inlined record allocations"
    );
    function.body = body;
    assert_eq!(NativeFixture::new().execute_linked(&program,
        "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }",
        &[core_runtime_archive().into_os_string()]),
        "input once\ncapture once\n9007199254740994\nleftλ雪\n0.75\nrightλ雪\n1.75\n9007199254740993\nleft\n0.5\n-9007199254740993\nright\n1.5\n".as_bytes());
}

#[test]
fn inlined_arithmetic_faults_retire_caller_roots_and_preserve_cleanup() {
    let source = r#"
fn probe(divisor: Int) -> Int:
    defer println("caller cleanup")
    let values = List.map([1, 2], (value) -> (100 / divisor) + value)
    List.fold(values, 0, (total, value) -> total + value)
fn main(): ()
"#;
    let checked = checked(source);
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == "probe")
        .unwrap()
        .id
        .0;
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == format!("f{id}"))
        .unwrap();
    assert_eq!(
        indirect_calls(function),
        0,
        "fault originates in inlined callback"
    );
    function.export = true;
    let harness = format!(
        r#"
unsafe extern "C" {{
    fn f{id}(env: usize, fault: *mut i64, divisor: i64) -> i64;
    fn morrow_gc_collect_precise();
    fn morrow_gc_heap_size() -> usize;
}}
fn main() {{
    let mut fault = 0;
    unsafe {{ f{id}(0, &mut fault, 0); }}
    assert_eq!(fault, 1);
    unsafe {{ morrow_gc_collect_precise(); }}
    assert_eq!(unsafe {{ morrow_gc_heap_size() }}, 0);
    fault = 0;
    assert_eq!(unsafe {{ f{id}(0, &mut fault, 2) }}, 103);
    assert_eq!(fault, 0);
    unsafe {{ morrow_gc_collect_precise(); }}
    assert_eq!(unsafe {{ morrow_gc_heap_size() }}, 0);
    println!("next invocation clean");
}}
"#
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"caller cleanup\ncaller cleanup\nnext invocation clean\n"
    );
}

#[test]
fn callbacks_with_function_scoped_control_keep_their_own_activation() {
    let source = r#"
fn main():
    let values = List.map([1, 2], (value) ->
        defer println("callback cleanup")
        if value == 1: return 10
        20
    )
    println(List.fold(values, 0, (total, value) -> total + value))
"#;
    let checked = checked(source);
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id
        .0;
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let function = program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == format!("f{id}"))
        .unwrap();
    assert_eq!(
        indirect_calls(function),
        1,
        "defer/return callback remains scoped"
    );
    assert_eq!(NativeFixture::new().execute_linked(&program,
        "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }",
        &[core_runtime_archive().into_os_string()]),
        b"callback cleanup\ncallback cleanup\n30\n");
}

#[test]
fn inlined_branching_map_simulation_retains_old_versions_through_precise_collection() {
    for seed in [1_u64, 42, 104_729] {
        let mut source = String::from(
            r#"
type Cell:
    index: Int
    ticks: Int
    label: String
fn update(values: List(Cell), target: Int, delta: Int, suffix: String) -> List(Cell):
    List.map(values, (cell) -> if cell.index == target: Cell(cell.index, cell.ticks + delta, cell.label + suffix) else: cell)
fn main():
    let v0 = [Cell(0, 0, String.repeat("a", 1)), Cell(1, 0, String.repeat("b", 1)), Cell(2, 0, String.repeat("c", 1))]
    let suffix = String.repeat("雪", 1)
"#,
        );
        let mut random = seed;
        let mut ticks = [0_i64; 3];
        let mut changes = [0_usize; 3];
        let mut snapshots = vec![(ticks, changes)];
        for step in 1..=12 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let target = (random % 3) as usize;
            let delta = ((random >> 8) % 17) as i64 - 8;
            ticks[target] += delta;
            changes[target] += 1;
            snapshots.push((ticks, changes));
            source.push_str(&format!(
                "    let v{step} = update(v{}, target: {target}, delta: {delta}, suffix: suffix)\n",
                step - 1
            ));
        }
        let mut expected = String::new();
        for (version, (ticks, changes)) in snapshots.iter().enumerate().rev() {
            source.push_str(&format!("    for cell in v{version}:\n        println(cell.ticks)\n        println(cell.label)\n"));
            for index in 0..3 {
                expected.push_str(&format!(
                    "{}\n{}{}\n",
                    ticks[index],
                    ["a", "b", "c"][index],
                    "雪".repeat(changes[index])
                ));
            }
        }
        let checked = checked(&source);
        let id = checked
            .functions
            .iter()
            .find(|f| f.name == "update")
            .unwrap()
            .id
            .0;
        let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
        let function = program
            .functions
            .iter_mut()
            .find(|f| machine::bare(&f.name) == format!("f{id}"))
            .unwrap();
        assert_eq!(indirect_calls(function), 0, "simulate actual inline branch");
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "morrow_alloc")
            {
                body.push(Statement::Effect(Operation::Call {
                    callee: symbol("morrow_gc_collect_precise"),
                    args: vec![],
                    variadic: None,
                }));
            }
            body.push(statement);
        }
        function.body = body;
        assert_eq!(NativeFixture::new().execute_linked(&program,
            "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }",
            &[core_runtime_archive().into_os_string()]), expected.as_bytes(), "seed={seed}");
    }
}
