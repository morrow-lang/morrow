//! Native value-semantics oracles, independent of collection/GC implementation.
use super::*;

fn compiled(source: &str) -> Program {
    let checked =
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    fern_compiler::lowering::lower(&checked).unwrap()
}

/// Force collection at an observable operation inside captured callbacks. This
/// deliberately ignores conservative stack/register roots; generated frames
/// must retain the input, unfinished output, captures and historical versions.
fn collect_before_string_length(program: &mut Program) {
    let mut points = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in std::mem::take(&mut function.body) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if machine::bare(name) == "fern_str_len")
            {
                body.push(Statement::Effect(Operation::Call {
                    callee: symbol("fern_gc_collect_precise"),
                    args: vec![],
                    variadic: None,
                }));
                points += 1;
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert!(points >= 2, "exercise both nested callback bodies");
}

const MODEL: &str = r#"
type Cell:
    index: Int
    ticks: Int
    wide: Int
    fraction: Float
    label: String
    chunks: List((Int, String))
fn update(values: List(Cell), target: Int, delta: Int) -> List(Cell):
    let suffix = String.repeat("λ雪", 1)
    List.map(values, (cell) ->
        let outer = String.len(cell.label)
        if cell.index == target:
            let chunks = List.map(cell.chunks, (chunk) ->
                let inner = String.len(suffix)
                (chunk.0 + delta, chunk.1 + suffix)
            )
            %{ cell | ticks: cell.ticks + delta, fraction: cell.fraction + 0.25, label: cell.label + suffix, chunks: chunks }
        else: cell
    )
fn dump(values: List(Cell), fractions: List(Float)):
    println(List.len(values))
    for cell in values:
        println(cell.ticks)
        println(cell.wide)
        println(cell.fraction == List.get(fractions, cell.index))
        println(cell.label)
        for chunk in cell.chunks:
            println(chunk.0)
            println(chunk.1)
fn main():
    let empty: List(Cell) = []
    println(List.len(update(empty, target: 0, delta: 1)))
"#;

#[test]
fn immutable_gc_seeded_nested_maps_retain_every_historical_version_and_exact_payload() {
    const WIDTH: usize = 8;
    const STEPS: usize = 24;
    for seed in [1_u64, 42, 0x4645_524e] {
        let mut source = String::from(MODEL);
        let mut expected = String::from("0\n");
        let initial = (0..WIDTH)
            .map(|index| {
                let wide = if index % 2 == 0 { i64::MIN } else { i64::MAX };
                format!(
                    "Cell({index}, 0, {wide}, 0.5, String.repeat(\"cell-{index}\", 1), [(-9007199254740993, String.repeat(\"left\", 1)), (9007199254740993, String.repeat(\"right\", 1))])"
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        source.push_str(&format!("    let v0 = [{initial}]\n"));

        // The oracle uses ordinary Rust arrays and snapshots, not Fern's map,
        // record lowering, root registry, or callback machinery.
        let mut ticks = [0_i64; WIDTH];
        let mut changes = [0_u32; WIDTH];
        let mut snapshots = vec![(ticks, changes)];
        let mut random = seed;
        for step in 1..=STEPS {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let target = (random % WIDTH as u64) as usize;
            let delta = ((random >> 8) % 17) as i64 - 8;
            ticks[target] += delta;
            changes[target] += 1;
            snapshots.push((ticks, changes));
            source.push_str(&format!(
                "    let v{step} = update(v{}, target: {target}, delta: {delta})\n",
                step - 1
            ));
        }
        // Observe old versions only after all later versions have been made.
        // A copy-on-write or builder regression therefore cannot pass by
        // producing the right newest result while corrupting its aliases.
        for (version, (ticks, changes)) in snapshots.iter().enumerate().rev() {
            let fractions = changes
                .iter()
                .map(|count| format!("{:.2}", 0.5 + f64::from(*count) / 4.0))
                .collect::<Vec<_>>()
                .join(", ");
            source.push_str(&format!("    dump(v{version}, fractions: [{fractions}])\n"));
            expected.push_str(&format!("{WIDTH}\n"));
            for index in 0..WIDTH {
                let wide = if index % 2 == 0 { i64::MIN } else { i64::MAX };
                let suffix = "λ雪".repeat(changes[index] as usize);
                expected.push_str(&format!(
                    "{}\n{wide}\ntrue\ncell-{index}{suffix}\n{}\nleft{suffix}\n{}\nright{suffix}\n",
                    ticks[index],
                    -9_007_199_254_740_993_i64 + ticks[index],
                    9_007_199_254_740_993_i64 + ticks[index],
                ));
            }
        }
        let mut program = compiled(&source);
        collect_before_string_length(&mut program);
        let harness = r#"unsafe extern "C" { fn fern_main() -> i32; }
fn main() { assert_eq!(unsafe { fern_main() }, 0); }"#;
        assert_eq!(
            NativeFixture::new().execute_linked(
                &program,
                harness,
                &[core_runtime_archive().into_os_string()]
            ),
            expected.as_bytes(),
            "immutable collection simulation seed={seed}"
        );
    }
}

#[test]
fn immutable_gc_nested_callback_fault_retires_frames_before_next_invocation() {
    let source = r#"
fn probe(divisor: Int) -> Int:
    defer println("caller cleanup")
    let capture = String.repeat("λ雪", 2)
    let result = List.map([1, 2], (value) ->
        defer println("outer cleanup")
        let outer = String.len(capture)
        let nested = List.map([value], (inner) ->
            defer println("inner cleanup")
            let length = String.len(capture)
            (100 / divisor) + inner + length
        )
        List.head(nested)
    )
    List.fold(result, 0, (total, value) -> total + value)
fn main(): ()
"#;
    let checked =
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == "probe")
        .unwrap()
        .id
        .0;
    let mut program = fern_compiler::lowering::lower(&checked).unwrap();
    program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == format!("f{id}"))
        .unwrap()
        .export = true;
    collect_before_string_length(&mut program);
    let harness = format!(
        r#"unsafe extern "C" {{
    fn f{id}(env: usize, fault: *mut i64, divisor: i64) -> i64;
    fn fern_gc_collect_precise();
    fn fern_gc_heap_size() -> usize;
}}
fn main() {{
    let mut fault = 0;
    unsafe {{ f{id}(0, &mut fault, 0); }}
    assert_eq!(fault, 1);
    unsafe {{ fern_gc_collect_precise(); }}
    assert_eq!(unsafe {{ fern_gc_heap_size() }}, 0, "fault leaked a root frame");
    fault = 0;
    assert_eq!(unsafe {{ f{id}(0, &mut fault, 2) }}, 123);
    assert_eq!(fault, 0);
    unsafe {{ fern_gc_collect_precise(); }}
    assert_eq!(unsafe {{ fern_gc_heap_size() }}, 0, "success leaked a root frame");
    println!("original fault preserved; next invocation clean");
}}"#
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"inner cleanup\nouter cleanup\ncaller cleanup\ninner cleanup\nouter cleanup\ninner cleanup\nouter cleanup\ncaller cleanup\noriginal fault preserved; next invocation clean\n"
    );
}
