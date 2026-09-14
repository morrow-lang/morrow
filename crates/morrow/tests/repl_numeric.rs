use morrow_compiler::repl::Session;

#[test]
fn powers_are_right_associative_and_wrap_at_full_width() {
    let mut s = Session::default();
    for (source, expected) in [
        ("2 ** 3 ** 2", "512 : Int\n"),
        ("-2 ** 2", "4 : Int\n"),
        ("0 ** 0", "1 : Int\n"),
        ("9223372036854775807 ** 2", "1 : Int\n"),
        ("2 ** 63", "-9223372036854775808 : Int\n"),
        ("2 ** 64", "0 : Int\n"),
        ("(-1) ** 9223372036854775807", "-1 : Int\n"),
        ("2.0 ** -3.0", "0.125 : Float\n"),
    ] {
        assert_eq!(s.evaluate(source).unwrap(), expected, "{source}");
    }
}

#[test]
fn bitwise_operations_use_defined_full_width_shift_counts() {
    let mut s = Session::default();
    for (source, expected) in [
        ("0xFF &&& 0b1010", "10 : Int\n"),
        ("0o7 ||| 0x10", "23 : Int\n"),
        ("0xFF ^^^ 0xF0", "15 : Int\n"),
        ("~~~0", "-1 : Int\n"),
        ("1 <<< 63", "-9223372036854775808 : Int\n"),
        ("1 <<< 64", "1 : Int\n"),
        ("1 <<< -1", "-9223372036854775808 : Int\n"),
        ("-8 >>> 2", "-2 : Int\n"),
        ("-9223372036854775808 >>> 63", "-1 : Int\n"),
        ("-9223372036854775808 / -1", "-9223372036854775808 : Int\n"),
        ("-9223372036854775808 % -1", "0 : Int\n"),
    ] {
        assert_eq!(s.evaluate(source).unwrap(), expected, "{source}");
    }
}

#[test]
fn float_contains_uses_ieee_value_equality() {
    let mut s = Session::default();
    for source in [
        "List.contains([0.0], -0.0)",
        "List.contains([-0.0], 0.0)",
        "List.contains([1.0 / 0.0], 1.0 / 0.0)",
        "List.contains([0.0 / 0.0, 2.0], 2.0)",
    ] {
        assert_eq!(s.evaluate(source).unwrap(), "true : Bool\n", "{source}");
    }
    assert_eq!(
        s.evaluate("List.contains([0.0 / 0.0], 0.0 / 0.0)").unwrap(),
        "false : Bool\n"
    );
}

#[test]
fn domain_failures_preserve_first_error_and_run_cleanup() {
    let mut s = Session::default();
    s.evaluate("fn cleanup() -> (): println(2 ** -1)").unwrap();
    s.evaluate("fn failing() -> Int:\n    defer cleanup()\n    1 / 0")
        .unwrap();
    assert_eq!(
        s.evaluate("failing()").unwrap_err(),
        "integer division by zero"
    );
    assert_eq!(
        s.evaluate("2 ** -1").unwrap_err(),
        "negative integer exponent"
    );
    assert_eq!(s.evaluate("2 ** 3").unwrap(), "8 : Int\n");
}
