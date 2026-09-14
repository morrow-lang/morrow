//! Independent full-width and cleanup oracles for constant integer arithmetic.
use super::*;

#[test]
fn constant_division_and_remainder_match_wide_oracles_for_boundaries_and_seeded_inputs() {
    let divisors = [
        i64::MIN,
        i64::MIN + 1,
        -4294967297,
        -2147483647,
        -1024,
        -17,
        -3,
        -2,
        -1,
        1,
        2,
        3,
        7,
        17,
        1024,
        2147483647,
        4294967297,
        i64::MAX,
    ];
    let mut source = String::new();
    for (index, divisor) in divisors.iter().enumerate() {
        source.push_str(&format!(
            "fn quotient{index}(value: Int) -> Int: value / {divisor}\nfn remainder{index}(value: Int) -> Int: value % {divisor}\n"
        ));
    }
    source.push_str("fn main(): ()\n");
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(&source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let mut declarations = String::new();
    let mut cases = String::new();
    for (index, divisor) in divisors.iter().enumerate() {
        let mut names = Vec::new();
        for prefix in ["quotient", "remainder"] {
            let id = checked
                .functions
                .iter()
                .find(|function| function.name == format!("{prefix}{index}"))
                .unwrap()
                .id
                .0;
            let name = format!("f{id}");
            program
                .functions
                .iter_mut()
                .find(|function| machine::bare(&function.name) == name)
                .unwrap()
                .export = true;
            declarations.push_str(&format!(
                "fn {name}(env: usize, fault: *mut i64, value: i64) -> i64;\n"
            ));
            names.push(name);
        }
        cases.push_str(&format!("({}, {}, {divisor}_i64),\n", names[0], names[1]));
    }
    let harness = format!(
        r#"
unsafe extern "C" {{
    {declarations}
}}
type Operation = unsafe extern "C" fn(usize, *mut i64, i64) -> i64;
fn main() {{
    let cases: &[(Operation, Operation, i64)] = &[
        {cases}
    ];
    let mut values = vec![i64::MIN, i64::MIN + 1, -9007199254740993,
        -4294967297, -2147483648, -18, -17, -16, -8, -7, -3, -2, -1,
        0, 1, 2, 3, 7, 8, 16, 17, 18, 2147483648, 4294967297,
        9007199254740993, i64::MAX - 1, i64::MAX];
    for &(_, _, divisor) in cases {{
        values.extend([divisor.saturating_sub(1), divisor, divisor.saturating_add(1)]);
    }}
    for mut seed in [1_u64, 0x4645524e, 104729] {{
        for _ in 0..1024 {{
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            values.push(seed as i64);
        }}
    }}
    for &(quotient, remainder, divisor) in cases {{
        for &value in &values {{
            let mut fault = 0;
            // These are exported generated functions with the validated Morrow ABI;
            // the fault slot remains live and writable throughout each call.
            let actual_q = unsafe {{ quotient(0, &mut fault, value) }};
            assert_eq!(fault, 0, "quotient fault: {{value}} / {{divisor}}");
            let actual_r = unsafe {{ remainder(0, &mut fault, value) }};
            assert_eq!(fault, 0, "remainder fault: {{value}} % {{divisor}}");
            // i128 keeps MIN / -1 representable; the final cast independently
            // models Morrow's wrapping result without using Rust i64 division.
            assert_eq!(actual_q, (i128::from(value) / i128::from(divisor)) as i64,
                "quotient: {{value}} / {{divisor}}");
            assert_eq!(actual_r, (i128::from(value) % i128::from(divisor)) as i64,
                "remainder: {{value}} % {{divisor}}");
        }}
    }}
    println!("constant arithmetic oracles passed");
}}
"#
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"constant arithmetic oracles passed\n"
    );
}

#[test]
fn constant_operations_preserve_operand_effects_and_callback_payloads() {
    let source = r#"
fn observed(value: Int) -> Int:
    println("operand")
    value
fn main():
    println(observed(-17) / 3)
    println(observed(-17) % 3)
    println(observed(-9223372036854775808) / -1)
    println(observed(-9223372036854775808) % -1)
    println(observed(7) % 1)
    let original = [-9223372036854775808, -17, 17, 9223372036854775807]
    for value in List.map(original, (value) -> value / 3): println(value)
    for value in List.map(original, (value) -> value % -3): println(value)
    for value in original: println(value)
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }",
            &[core_runtime_archive().into_os_string()]
        ),
        concat!(
            "operand\n-5\noperand\n-2\noperand\n-9223372036854775808\noperand\n0\noperand\n0\n",
            "-3074457345618258602\n-5\n5\n3074457345618258602\n",
            "-2\n-2\n2\n1\n",
            "-9223372036854775808\n-17\n17\n9223372036854775807\n"
        ).as_bytes()
    );
}

#[test]
fn zero_divisors_still_fault_after_operand_evaluation_and_retire_roots() {
    let source = r#"
fn observed(value: Int) -> Int:
    println("operand")
    value
fn probe(mode: Int, divisor: Int) -> Int:
    let label = String.repeat("cleanup", 1)
    defer println(label)
    if mode == 0: return observed(17) / 0
    if mode == 1: return observed(17) % 0
    if mode == 2: return observed(17) / divisor
    observed(17) % divisor
fn main(): ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == "probe")
        .unwrap()
        .id
        .0;
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == format!("f{id}"))
        .unwrap()
        .export = true;
    let harness = format!(
        r#"
unsafe extern "C" {{
    fn f{id}(env: usize, fault: *mut i64, mode: i64, divisor: i64) -> i64;
    fn morrow_gc_collect_precise();
    fn morrow_gc_heap_size() -> usize;
}}
fn main() {{
    for mode in 0..4 {{
        let mut fault = 0;
        // The fault slot is live and writable; collection runs only after the
        // generated activation returns, with no remaining host-held Morrow value.
        unsafe {{ f{id}(0, &mut fault, mode, 0); }}
        assert_eq!(fault, 1);
        unsafe {{ morrow_gc_collect_precise(); }}
        assert_eq!(unsafe {{ morrow_gc_heap_size() }}, 0);
    }}
    for (mode, expected) in [(2, 5), (3, 2)] {{
        let mut fault = 0;
        assert_eq!(unsafe {{ f{id}(0, &mut fault, mode, 3) }}, expected);
        assert_eq!(fault, 0);
        unsafe {{ morrow_gc_collect_precise(); }}
        assert_eq!(unsafe {{ morrow_gc_heap_size() }}, 0);
    }}
}}
"#
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        "operand\ncleanup\n".repeat(6).as_bytes()
    );
}

#[test]
fn integer_tail_backedges_keep_simultaneous_parameters_and_wrapping_seeded_results() {
    let source = r#"
fn rotate(n: Int, a: Int, b: Int, c: Int) -> Int:
    if n == 0: return a + 3 * b + 5 * c
    match n % 3:
        0 -> rotate(n: n - 1, a: b, b: c, c: a + n)
        1 ->
            if n == 1: rotate(n: 0, a: c, b: a - 3, c: b)
            else: rotate(n: n - 1, a: c, b: a - 3, c: b)
        _ -> rotate(n: n - 1, a: a * 48271, b: c, c: b)
fn sum(n: Int) -> Int:
    if n == 0: 0
    else: n + sum(n - 1)
fn main(): ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let rotate = export_checked_function(&checked, &mut program, "rotate");
    let sum = export_checked_function(&checked, &mut program, "sum");
    let harness = format!(
        r#"
unsafe extern "C" {{
    fn {rotate}(env: usize, fault: *mut i64, n: i64, a: i64, b: i64, c: i64) -> i64;
    fn {sum}(env: usize, fault: *mut i64, n: i64) -> i64;
}}
fn oracle(mut n: i64, mut a: i64, mut b: i64, mut c: i64) -> i64 {{
    while n > 0 {{
        (a, b, c) = match n % 3 {{
            0 => (b, c, a.wrapping_add(n)),
            1 => (c, a.wrapping_sub(3), b),
            _ => (a.wrapping_mul(48271), c, b),
        }};
        n -= 1;
    }}
    a.wrapping_add(b.wrapping_mul(3)).wrapping_add(c.wrapping_mul(5))
}}
fn main() {{
    let mut inputs = vec![(i64::MIN, i64::MAX, -1), (0, 1, 2),
        (9007199254740993, -9007199254740993, 4294967297)];
    for mut seed in [1_u64, 0x4645524e, 104729] {{
        for _ in 0..16 {{
            let mut triple = [0; 3];
            for value in &mut triple {{
                seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
                *value = seed as i64;
            }}
            inputs.push((triple[0], triple[1], triple[2]));
        }}
    }}
    for (a, b, c) in inputs {{
        for n in [0, 1, 2, 3, 17, 128, 100000] {{
            let mut fault = 0;
            // All arguments are scalar ABI values and the fault slot is live.
            assert_eq!(unsafe {{ {rotate}(0, &mut fault, n, a, b, c) }},
                oracle(n, a, b, c), "n={{n}} a={{a}} b={{b}} c={{c}}");
            assert_eq!(fault, 0);
        }}
    }}
    let mut fault = 0;
    assert_eq!(unsafe {{ {sum}(0, &mut fault, 1000) }}, 500500);
    assert_eq!(fault, 0);
    println!("tail backedge oracles passed");
}}
"#
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"tail backedge oracles passed\n"
    );
}

#[test]
fn integer_tail_arguments_keep_effect_order_and_unwind_later_argument_faults() {
    let source = r#"
fn observed(value: Int) -> Int:
    println(value)
    value
fn later(value: Int) -> Int:
    defer println("argument cleanup")
    1 / value
fn cycle(n: Int, first: Int, last: Int) -> Int:
    if n == 0: return first + last
    cycle(n: n - 1, first: observed(first + 1), last: later(n - 1))
fn probe() -> Int:
    defer println("caller cleanup")
    cycle(n: 2, first: 1, last: 9)
fn main(): ()
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let probe = export_checked_function(&checked, &mut program, "probe");
    let harness = format!(
        r#"
unsafe extern "C" {{
    fn {probe}(env: usize, fault: *mut i64) -> i64;
    fn morrow_gc_collect_precise();
    fn morrow_gc_heap_size() -> usize;
}}
fn main() {{
    let mut fault = 0;
    // The generated ABI receives a live writable fault context.
    unsafe {{ {probe}(0, &mut fault); }}
    assert_eq!(fault, 1);
    unsafe {{ morrow_gc_collect_precise(); }}
    assert_eq!(unsafe {{ morrow_gc_heap_size() }}, 0);
}}
"#
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"2\nargument cleanup\n3\nargument cleanup\ncaller cleanup\n"
    );
}

#[test]
fn integer_tail_parameters_do_not_discard_managed_body_roots_at_collection() {
    let source = r#"
fn walk(n: Int, total: Int) -> Int:
    let text = String.repeat("root", 1)
    let scratch = String.len(String.repeat("temporary", 1))
    let width = String.len(text)
    if n == 0: total + width + scratch
    else: walk(n: n - 1, total: total + width + scratch)
fn main():
    println(walk(n: 512, total: 9007199254740993))
"#;
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = morrow_compiler::lowering::lower(&checked).unwrap();
    let walk = format!(
        "f{}",
        checked
            .functions
            .iter()
            .find(|f| f.name == "walk")
            .unwrap()
            .id
            .0
    );
    let mut points = 0;
    for function in program
        .functions
        .iter_mut()
        .filter(|f| machine::bare(&f.name) == walk)
    {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "morrow_str_len")
            {
                body.push(Statement::Effect(Operation::Call {
                    callee: symbol("morrow_gc_collect_precise"),
                    args: vec![],
                    variadic: None,
                }));
                points += 1;
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert_eq!(points, 2, "collect with the managed body-local text live");
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            "unsafe extern \"C\" { fn morrow_main() -> i32; } fn main() { assert_eq!(unsafe { morrow_main() }, 0); }",
            &[core_runtime_archive().into_os_string()]
        ),
        b"9007199254747662\n"
    );
}

fn export_checked_function(
    checked: &morrow_compiler::ir::Program,
    program: &mut Program,
    name: &str,
) -> String {
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap()
        .id
        .0;
    let name = format!("f{id}");
    program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == name)
        .unwrap()
        .export = true;
    name
}
