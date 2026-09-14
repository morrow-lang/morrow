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
    // Nested strings show as literals so `[a, b]` never hides an empty or spaced string.
    assert_eq!(run("println([\"a\", \"b\"])"), "[\"a\", \"b\"]\n");
    assert_eq!(
        run("println([\"\", \"say \\\"hi\\\"\", \"back\\\\slash\", \"tab\\tline\\nbreak\"])"),
        "[\"\", \"say \\\"hi\\\"\", \"back\\\\slash\", \"tab\\tline\\nbreak\"]\n"
    );
    assert_eq!(run("println(show(\"plain\"))"), "\"plain\"\n");
    assert_eq!(run("println(String.quote(\"x\\\"y\"))"), "\"x\\\"y\"\n");
    assert_eq!(
        run("let x: Option(Int) = Some(3)\n---\nprintln(x)"),
        "Some(3)\n"
    );
    assert_eq!(run("let n: Option(Int) = None\n---\nprintln(n)"), "None\n");
    assert_eq!(run("println((1, \"a\", true))"), "(1, \"a\", true)\n");
    assert_eq!(
        run("let r: Result(Int, String) = Err(\"bad\")\nprintln(r)"),
        "Err(\"bad\")\n"
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
            "[\"\", \"Apple\", \"apple\", \"pear\"]\n",
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
        (
            "List.zip([1, 2, 3], [\"a\", \"b\"])",
            "[(1, \"a\"), (2, \"b\")]\n",
        ),
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
fn generic_bodies_print_any_show_argument() {
    assert_eq!(
        run(
            "fn twice(value: a):\n    println(value)\n    println([value])\n---\ntwice(1)\n---\ntwice(\"s\")\n---\ntwice(Some((1, true)))"
        ),
        "1\n[1]\ns\n[\"s\"]\nSome((1, true))\n[Some((1, true))]\n"
    );
    let message = failure("fn twice(value: a): println(value)\nfn main(): twice(() -> 1)\n");
    assert!(message.contains("Show"), "{message}");
}

#[test]
fn sort_orders_structured_elements_through_ord() {
    let person = "type Person derive(Show, Eq, Ord, Clone):\n    name: String\n    age: Int\n---\n";
    let people =
        "[Person(\"Cy\", 40), Person(\"Ada\", 36), Person(\"Bo\", 40), Person(\"Al\", 36)]";
    assert_eq!(
        run(&format!("{person}println(List.sort({people}))")),
        "[Person(name: \"Ada\", age: 36), Person(name: \"Al\", age: 36), Person(name: \"Bo\", age: 40), Person(name: \"Cy\", age: 40)]\n"
    );
    // Stable: equal ages keep their input order.
    assert_eq!(
        run(&format!(
            "{person}println(List.sort_by({people}, (l: Person, r: Person) -> compare(left: l.age, right: r.age)))"
        )),
        "[Person(name: \"Ada\", age: 36), Person(name: \"Al\", age: 36), Person(name: \"Cy\", age: 40), Person(name: \"Bo\", age: 40)]\n"
    );
    for (expression, expected) in [
        (
            "List.sort([(2, \"b\"), (1, \"z\"), (2, \"a\")])",
            "[(1, \"z\"), (2, \"a\"), (2, \"b\")]\n",
        ),
        (
            "List.sort([Some(3), None, Some(1)])",
            "[Some(1), Some(3), None]\n",
        ),
        ("List.sort([[3, 1], [], [2]])", "[[], [2], [3, 1]]\n"),
        (
            "List.sort_by([5, 3, 9], (l: Int, r: Int) -> compare(left: r, right: l))",
            "[9, 5, 3]\n",
        ),
        (
            "List.sort_by([2.5, -1.0], (l: Float, r: Float) -> compare(left: l, right: r))",
            "[-1, 2.5]\n",
        ),
        (
            "List.sort_by(List.drop([1], 1), (l: Int, r: Int) -> compare(left: l, right: r))",
            "[]\n",
        ),
        (
            "List.sum(List.sort_by(List.range(0, 300), (l: Int, r: Int) -> compare(left: r, right: l)))",
            "44850\n",
        ),
    ] {
        assert_eq!(
            run(&format!("println({expression})")),
            expected,
            "{expression}"
        );
    }
    assert_eq!(
        run(
            "fn sorted(items: List(a)) -> List(a): List.sort(items)\n---\nprintln(sorted([3, 1, 2]))\n---\nprintln(sorted([\"b\", \"a\"]))\n---\nprintln(sorted([(1, 2), (0, 9)]))"
        ),
        "[1, 2, 3]\n[\"a\", \"b\"]\n[(0, 9), (1, 2)]\n"
    );
}

#[test]
fn sort_names_the_missing_ord_implementation() {
    let message =
        failure("type P derive(Show):\n    n: Int\nfn main(): println(List.sort([P(1)]))\n");
    assert!(
        message.contains("Ord") && message.contains("derive"),
        "{message}"
    );
    let message = failure("fn main(): println(List.len(List.sort([() -> 1])))\n");
    assert!(message.contains("List.sort requires"), "{message}");
    let message = failure("fn main(): println(List.sort_by([1], (l: Int, r: Int) -> l < r))\n");
    assert!(message.contains("Ordering"), "{message}");
}
