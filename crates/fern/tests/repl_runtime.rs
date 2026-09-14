use fern_compiler::repl::Session;

fn output(expression: &str) -> Result<String, String> {
    Session::default().evaluate(&format!("println({expression})"))
}

#[test]
fn ascii_case_and_native_whitespace_are_preserved() {
    for (expression, expected) in [
        ("String.to_upper(\"straße élève az\")", "STRAßE éLèVE AZ\n"),
        ("str_to_lower(\"ÄBC İ Z\")", "Äbc İ z\n"),
        ("String.trim(\" \\t x \\r\\n\")", " x \n"),
        ("String.trim_start(\" \\tx \\n\")", "x \n\n"),
        ("String.trim_end(\" x \\r\\n\")", " x\n"),
    ] {
        assert_eq!(output(expression).unwrap(), expected, "{expression}");
    }
}

#[test]
fn split_and_lines_match_native_empty_and_delimiter_edges() {
    for (expression, expected) in [
        ("String.join(String.split(\"abc\", \"\"), \"|\")", "a|b|c\n"),
        ("List.len(String.split(\"\", \"\"))", "0\n"),
        ("List.len(String.split(\"\", \",\"))", "1\n"),
        (
            "String.join(String.split(\",a,,\", \",\"), \"|\")",
            "|a||\n",
        ),
        (
            "String.join(String.lines(\"a\\r\\nb\\rc\\n\"), \"|\")",
            "a|b|c\n",
        ),
        ("List.len(String.lines(\"\"))", "1\n"),
        ("List.len(String.lines(\"\\n\\n\"))", "2\n"),
        ("String.join([], \",\")", "\n"),
    ] {
        assert_eq!(output(expression).unwrap(), expected, "{expression}");
    }
}

#[test]
fn byte_indices_slices_replacements_and_repetition_match_native() {
    for (expression, expected) in [
        ("Option.unwrap_or(String.char_at(\"é\", 0), -1)", "195\n"),
        ("Option.unwrap_or(String.char_at(\"é\", 1), -1)", "169\n"),
        ("Option.unwrap_or(String.char_at(\"a\", -1), -1)", "-1\n"),
        (
            "Option.unwrap_or(String.index_of(\"éx\", \"x\"), -1)",
            "2\n",
        ),
        ("Option.unwrap_or(String.index_of(\"x\", \"\"), -1)", "0\n"),
        ("Option.is_none(String.index_of(\"x\", \"z\"))", "true\n"),
        ("str_slice(\"abcdef\", -5, 3)", "abc\n"),
        ("String.slice(\"abc\", 10, 2)", "\n"),
        ("String.slice(\"éx\", 0, 2)", "é\n"),
        ("String.replace(\"aaaa\", \"aa\", \"b\")", "bb\n"),
        ("String.replace(\"abc\", \"\", \"x\")", "abc\n"),
        ("str_repeat(\"ab\", 3)", "ababab\n"),
        ("String.repeat(\"ab\", -1)", "\n"),
    ] {
        assert_eq!(output(expression).unwrap(), expected, "{expression}");
    }
}

#[test]
fn positional_access_and_integer_parsing_match_native_contracts() {
    for (expression, expected) in [
        ("Option.unwrap_or(List.at([1, 2, 3], 2), -1)", "3\n"),
        ("Option.unwrap_or(List.at([1, 2, 3], 3), -1)", "-1\n"),
        ("Option.unwrap_or(List.at([1, 2, 3], -1), -1)", "-1\n"),
        ("Option.unwrap_or(List.first([\"a\", \"b\"]), \"z\")", "a\n"),
        ("Option.unwrap_or(List.last([\"a\", \"b\"]), \"z\")", "b\n"),
        ("Option.is_none(List.first(List.drop([1], 1)))", "true\n"),
        ("Option.is_none(List.last(List.take([1], 0)))", "true\n"),
        ("List.len(List.take([1, 2, 3], 2))", "2\n"),
        ("List.len(List.take([1, 2, 3], -1))", "0\n"),
        ("List.len(List.take([1, 2, 3], 99))", "3\n"),
        ("List.len(List.drop([1, 2, 3], 2))", "1\n"),
        ("List.len(List.drop([1, 2, 3], 99))", "0\n"),
        ("List.len(List.drop([1, 2, 3], -1))", "3\n"),
        (
            "Option.unwrap_or(List.first(List.drop([1, 2, 3], 2)), 0)",
            "3\n",
        ),
        ("Option.unwrap_or(Int.parse(\"42\"), 0)", "42\n"),
        ("Option.unwrap_or(Int.parse(\"+7\"), 0)", "7\n"),
        (
            "Option.unwrap_or(Int.parse(\"-9223372036854775808\"), 0)",
            "-9223372036854775808\n",
        ),
        (
            "Option.is_none(Int.parse(\"9223372036854775808\"))",
            "true\n",
        ),
        ("Option.is_none(Int.parse(\"\"))", "true\n"),
        ("Option.is_none(Int.parse(\"-\"))", "true\n"),
        ("Option.is_none(Int.parse(\" 1\"))", "true\n"),
        ("Option.is_none(Int.parse(\"1.0\"))", "true\n"),
        ("Option.is_none(Int.parse(\"１２\"))", "true\n"),
    ] {
        assert_eq!(output(expression).unwrap(), expected, "{expression}");
    }
}

#[test]
fn list_aliases_share_values_and_sum_predicates_preserve_tags() {
    let mut session = Session::default();
    session.evaluate("let xs = [1, 2]").unwrap();
    assert_eq!(
        session
            .evaluate("println(list_len(list_push(xs, 3)))")
            .unwrap(),
        "3\n"
    );
    assert_eq!(session.evaluate("println(List.len(xs))").unwrap(), "2\n");
    assert_eq!(output("List.len(List.tail([1]))").unwrap(), "0\n");
    assert_eq!(output("List.contains([\"é\"], \"é\")").unwrap(), "true\n");
    session
        .evaluate("fn failure() -> Result(Int, String): Err(\"bad\")")
        .unwrap();
    assert_eq!(
        session
            .evaluate("println(Result.is_err(failure()))")
            .unwrap(),
        "true\n"
    );
    assert_eq!(
        session
            .evaluate("println(Result.unwrap_or(failure(), 9))")
            .unwrap(),
        "9\n"
    );
    assert!(output("List.get([1], -1)").unwrap_err().contains("bounds"));
}

#[test]
fn allocations_are_rejected_before_growth_and_session_recovers() {
    let mut session = Session::default();
    for expression in [
        "String.repeat(\"x\", 9223372036854775807)",
        "String.join(String.split(String.repeat(\"x\", 65536), \"\"), String.repeat(\"a\", 65536))",
        "String.split(String.repeat(\"x\", 65537), \"\")",
        "String.replace(String.repeat(\"x\", 1024), \"x\", String.repeat(\"y\", 2048))",
    ] {
        assert!(
            session.evaluate(expression).unwrap_err().contains("limit"),
            "{expression}"
        );
    }
    assert_eq!(session.evaluate("1 + 1").unwrap(), "2 : Int\n");
}

#[test]
fn unsupported_operations_and_invalid_utf8_fragments_name_source_api() {
    let error = output("Result.is_ok(http.get(\"https://example.invalid\"))").unwrap_err();
    assert!(
        error.contains("http.get") && !error.contains("fern_http_get"),
        "{error}"
    );
    let error = Session::default()
        .evaluate("String.slice(\"é\", 0, 1)")
        .unwrap_err();
    assert!(
        error.contains("UTF-8") && error.contains("String.slice"),
        "{error}"
    );
}

#[test]
fn local_file_results_use_fern_error_codes_and_effects_run_once() {
    let path = std::env::temp_dir().join(format!("fern-repl-runtime-{}", std::process::id()));
    let missing = path.join("missing");
    let missing = format!("{:?}", missing.to_string_lossy());
    for (api, code) in [("File.read", 1), ("File.size", 1), ("File.delete", 1)] {
        let expression = format!("match {api}({missing}):\n    Ok(_) -> 0\n    Err(code) -> code");
        assert_eq!(
            Session::default().evaluate(&expression).unwrap(),
            format!("{code} : Int\n")
        );
    }
    assert_eq!(
        Session::default()
            .evaluate(&format!(
                "match File.write({missing}, \"x\"):\n    Ok(_) -> 0\n    Err(code) -> code"
            ))
            .unwrap(),
        "2 : Int\n"
    );
    let filename = format!("{:?}", path.to_string_lossy());
    let mut session = Session::default();
    assert_eq!(
        session
            .evaluate(&format!(
                "println(Result.unwrap_or(File.write({filename}, \"é\"), -1))"
            ))
            .unwrap(),
        "2\n"
    );
    assert_eq!(
        session
            .evaluate(&format!(
                "let n = Result.unwrap_or(File.append({filename}, \"x\"), -1)"
            ))
            .unwrap(),
        ""
    );
    session.evaluate("println(n)").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "éx");
    assert_eq!(
        session
            .evaluate(&format!(
                "println(Result.unwrap_or(File.read({filename}), \"bad\"))"
            ))
            .unwrap(),
        "éx\n"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn output_and_list_growth_respect_limits_before_copying() {
    let mut session = Session::default();
    for source in [
        "println(String.repeat(\"x\", 1048576))",
        "List.push(String.split(String.repeat(\"x\", 65536), \"\"), \"y\")",
        "List.concat(String.split(String.repeat(\"x\", 65536), \"\"), [\"y\"])",
    ] {
        assert!(
            session.evaluate(source).unwrap_err().contains("limit"),
            "{source}"
        );
    }
    assert_eq!(
        session
            .evaluate("String.repeat(\"\", 9223372036854775807)")
            .unwrap(),
        "\"\" : String\n"
    );
}

#[test]
fn file_reads_bound_allocation_and_reject_unrepresentable_bytes() {
    let path = std::env::temp_dir().join(format!("fern-repl-read-{}", std::process::id()));
    let name = format!("{:?}", path.to_string_lossy());
    let source = format!("println(Result.unwrap_or(File.read({name}), \"bad\"))");
    std::fs::write(&path, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    assert!(
        Session::default()
            .evaluate(&source)
            .unwrap_err()
            .contains("limit")
    );
    std::fs::write(&path, [0xFF]).unwrap();
    assert_eq!(Session::default().evaluate(&source).unwrap(), "bad\n");
    std::fs::write(&path, b"good\0ignored").unwrap();
    assert_eq!(Session::default().evaluate(&source).unwrap(), "bad\n");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sql_close_preserves_the_explicit_native_only_boundary() {
    let error = output("Result.is_ok(sql.close(1))").unwrap_err();
    assert!(
        error.contains("sql.close is available in native programs"),
        "{error}"
    );
    assert!(!error.contains("fern_sql_close"), "{error}");
}
