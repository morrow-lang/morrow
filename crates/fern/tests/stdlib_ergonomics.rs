//! Printing structured values, argument diagnostics, checked arithmetic and list utilities.
use fern_compiler::repl::Session;
use fern_compiler::{check, parse};

fn failure(source: &str) -> String {
    let program = parse::parse(source).unwrap_or_else(|e| panic!("{source}: {}", e.message));
    check::check(&program)
        .err()
        .unwrap_or_else(|| panic!("{source} unexpectedly checked"))
        .message
}

fn run(source: &str) -> String {
    let mut session = Session::default();
    let mut output = String::new();
    for statement in source.split("\n---\n") {
        output.push_str(
            &session
                .evaluate(statement)
                .unwrap_or_else(|e| panic!("{statement}: {e}")),
        );
    }
    output
}

#[test]
fn type_mismatch_is_reported_before_a_missing_label() {
    let message =
        failure("fn add(a: Int, b: Int) -> Int: a + b\nfn main(): println(add(1, \"two\"))\n");
    assert!(
        message.contains("Int") && message.contains("String"),
        "{message}"
    );
    assert!(!message.contains("label"), "{message}");
    let message = failure("fn add(a: Int, b: Int) -> Int: a + b\nfn main(): println(add(1, 2))\n");
    assert!(message.contains("requires label"), "{message}");
}

#[test]
fn println_shows_lists_options_tuples_records_and_newtypes() {
    assert_eq!(run("println([1, 2, 3])"), "[1, 2, 3]\n");
    // Show(String) is the text itself, matching the existing derived contract.
    assert_eq!(run("println([\"a\", \"b\"])"), "[a, b]\n");
    assert_eq!(
        run("let x: Option(Int) = Some(3)\n---\nprintln(x)"),
        "Some(3)\n"
    );
    assert_eq!(run("let n: Option(Int) = None\n---\nprintln(n)"), "None\n");
    assert_eq!(run("println((1, \"a\", true))"), "(1, a, true)\n");
    assert_eq!(
        run("let r: Result(Int, String) = Err(\"bad\")\nprintln(r)"),
        "Err(bad)\n"
    );
    assert_eq!(
        run("type Point derive(Show):\n    x: Int\n    y: Int\n---\nprintln(Point(1, 2))"),
        "Point(x: 1, y: 2)\n"
    );
    assert_eq!(
        run("type Color derive(Show):\n    Red\n    Green\n---\nprintln([Red, Green])"),
        "[Red, Green]\n"
    );
    assert_eq!(
        run("newtype UserId derive(Show) = UserId(Int)\n---\nprintln(UserId(7))"),
        "UserId(7)\n"
    );
    assert_eq!(run("print([1])\n---\nprintln(\"!\")"), "[1]!\n");
    assert_eq!(run("println(\"plain\")"), "plain\n");
    assert_eq!(run("println(1.5)"), "1.5\n");
}

#[test]
fn println_of_a_type_without_show_names_the_missing_trait() {
    let message = failure("type Secret:\n    value: Int\nfn main(): println(Secret(1))\n");
    assert!(message.contains("Show"), "{message}");
    let message = failure("fn main():\n    let f = (x: Int) -> x\n    println(f)\n");
    assert!(message.contains("Show"), "{message}");
    let message = failure("fn main(): println(())\n");
    assert!(message.contains("print argument"), "{message}");
    // A program that defines a prelude name keeps the direct explanation instead of a clash.
    let message = failure("fn show(x: Int) -> String: \"x\"\nfn main(): println([1])\n");
    assert!(message.contains("implements Show"), "{message}");
}

#[test]
fn checked_arithmetic_returns_options() {
    for (expression, expected) in [
        ("Option.unwrap_or(Int.checked_add(1, 2), -1)", "3\n"),
        (
            "Option.is_none(Int.checked_add(9223372036854775807, 1))",
            "true\n",
        ),
        (
            "Option.is_none(Int.checked_sub(-9223372036854775808, 1))",
            "true\n",
        ),
        ("Option.unwrap_or(Int.checked_mul(6, 7), -1)", "42\n"),
        (
            "Option.is_none(Int.checked_mul(4294967296, 4294967296))",
            "true\n",
        ),
        ("Option.unwrap_or(Int.checked_div(7, 2), -1)", "3\n"),
        ("Option.is_none(Int.checked_div(7, 0))", "true\n"),
        (
            "Option.is_none(Int.checked_div(-9223372036854775808, -1))",
            "true\n",
        ),
        ("Option.unwrap_or(Int.checked_rem(-7, 2), 9)", "-1\n"),
        ("Option.is_none(Int.checked_rem(7, 0))", "true\n"),
        ("Option.unwrap_or(Int.checked_neg(5), 0)", "-5\n"),
        (
            "Option.is_none(Int.checked_neg(-9223372036854775808))",
            "true\n",
        ),
    ] {
        assert_eq!(
            run(&format!("println({expression})")),
            expected,
            "{expression}"
        );
    }
}

#[test]
fn sort_zip_range_and_sum_match_native_contracts() {
    for (expression, expected) in [
        (
            "List.sort([3, -9223372036854775808, 1, 9223372036854775807])",
            "[-9223372036854775808, 1, 3, 9223372036854775807]\n",
        ),
        (
            "List.sort([\"pear\", \"Apple\", \"apple\", \"\"])",
            "[, Apple, apple, pear]\n",
        ),
        ("List.sort([true, false, true])", "[false, true, true]\n"),
        ("List.sort([2.5, -0.5, 1.0])", "[-0.5, 1, 2.5]\n"),
        ("List.range(0, 5)", "[0, 1, 2, 3, 4]\n"),
        ("List.range(-2, 1)", "[-2, -1, 0]\n"),
        ("List.range(3, 3)", "[]\n"),
        ("List.range(3, -3)", "[]\n"),
        ("List.sum([1, 2, 3])", "6\n"),
        ("List.sum(List.range(1, 101))", "5050\n"),
        ("List.sum(List.drop([1], 1))", "0\n"),
        (
            "List.sum([9223372036854775807, 1])",
            "-9223372036854775808\n",
        ),
        ("List.zip([1, 2, 3], [\"a\", \"b\"])", "[(1, a), (2, b)]\n"),
        (
            "List.len(List.zip(List.range(0, 3), List.take([true], 0)))",
            "0\n",
        ),
        (
            "List.sum(List.map(List.zip([1, 2], [10, 20]), (pair: (Int, Int)) -> pair.0 * pair.1))",
            "50\n",
        ),
    ] {
        assert_eq!(
            run(&format!("println({expression})")),
            expected,
            "{expression}"
        );
    }
}

#[test]
fn sort_rejects_structured_elements_at_check_time() {
    let message = failure("fn main(): println(List.len(List.sort([(1, 2)])))\n");
    assert!(message.contains("List.sort requires"), "{message}");
}
