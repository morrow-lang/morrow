//! Independent value/ABI oracles for compiler-owned immutable list loops.
use super::*;

#[test]
fn bounded_lists_preserve_filter_prefixes_payload_bits_and_callback_order_under_precise_gc() {
    let source = r#"
type Cell:
    index: Int
    wide: Int
    text: String
fn select(values: List(Cell), divisor: Int) -> List(Cell):
    let capture = String.repeat("λ雪", 2)
    List.filter(values, (cell) ->
        let measured = String.len(cell.text + capture)
        cell.index % divisor == 0
    )
fn dump(values: List(Cell)):
    println(List.len(values))
    for cell in values:
        println(cell.index)
        println(cell.wide)
        println(cell.text)
fn observed(value: Int) -> Bool:
    println(value)
    value == 2
fn main():
    let original = [Cell(0, -9223372036854775808, String.repeat("zero", 1)), Cell(1, 9223372036854775807, String.repeat("one", 1)), Cell(2, 9007199254740993, String.repeat("two", 1)), Cell(3, -9007199254740993, String.repeat("three", 1))]
    let sparse = select(original, divisor: 2)
    let none = List.filter(sparse, (cell) ->
        let measured = String.len(cell.text)
        false
    )
    let all = select(sparse, divisor: 1)
    dump(none)
    dump(all)
    dump(sparse)
    dump(original)
    let empty: List(Cell) = []
    dump(select(empty, divisor: 1))
    let floats = [0.0, -0.0, 1.25, -2.5]
    let mapped = List.map(floats, (value) ->
        let measured = String.len(String.repeat("gc", 1))
        value
    )
    println(1.0 / List.get(mapped, 0) > 0.0)
    println(1.0 / List.get(mapped, 1) < 0.0)
    println(List.get(mapped, 2) == 1.25)
    println(List.get(mapped, 3) == -2.5)
    let flags = List.map([true, false], (value) -> not value)
    println(List.get(flags, 0))
    println(List.get(flags, 1))
    println(List.any([1, 2, 3], observed))
    println(List.all([2, 1, 3], observed))
    match List.find([1, 2, 3], observed):
        Some(value) -> println(value == 2)
        None -> println(false)
    println(List.any([], observed))
    println(List.all([], observed))
"#;
    let checked =
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let mut program = fern_compiler::lowering::lower(&checked).unwrap();
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
    assert!(points >= 3, "collect inside every allocating callback kind");
    let expected = concat!(
        "0\n",
        "2\n0\n-9223372036854775808\nzero\n2\n9007199254740993\ntwo\n",
        "2\n0\n-9223372036854775808\nzero\n2\n9007199254740993\ntwo\n",
        "4\n0\n-9223372036854775808\nzero\n1\n9223372036854775807\none\n2\n9007199254740993\ntwo\n3\n-9007199254740993\nthree\n",
        "0\ntrue\ntrue\ntrue\ntrue\nfalse\ntrue\n",
        "1\n2\ntrue\n2\n1\nfalse\n1\n2\ntrue\nfalse\ntrue\n"
    );
    assert_eq!(NativeFixture::new().execute_linked(
        &program,
        "unsafe extern \"C\" { fn fern_main() -> i32; } fn main() { assert_eq!(unsafe { fern_main() }, 0); }",
        &[core_runtime_archive().into_os_string()],
    ), expected.as_bytes());
}
