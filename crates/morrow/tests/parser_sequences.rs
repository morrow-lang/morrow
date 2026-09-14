use morrow_compiler::{format, parse};

#[test]
fn sequence_patterns_roundtrip_across_matching_and_binding_constructs() {
    for source in [
        "fn main():\n    match [1,2,3]:\n        [] -> 0\n        [first, ..rest] if first > 0 -> List.len(rest)\n        [_, .._] -> 0\n",
        "fn main():\n    let (first, ..tail) = (1, true, \"🌿\")\n    let (..all) = (1,)\n    let (first, .._) = (1,2)\n    let [first, ..tail] = [1,2] else: return 0\n    println(first)\n",
        "fn main():\n    for [..items] in [[1], []]: println(List.len(items))\n    for (head, ..tail) in [(1,true)]: println(head)\n    with [..values] <- load() do List.len(values)\n",
        "fn main():\n    match Some(([1], (true, \"🌿\"))):\n        Some(([first, ..rest], (flag, ..tail))) -> first\n        _ -> 0\n",
        "fn main():\n    match [1,2]:\n        [\n            first, # first element\n            ..tail, # the suffix\n        ] -> first\n        _ -> 0\n",
        "fn main():\n    match ():\n        (..empty) -> 0\n    match (1,):\n        (one,) -> one\n",
    ] {
        let canonical =
            format::format(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn rest_patterns_reject_ambiguous_middle_nested_and_duplicate_rest() {
    for pattern in [
        "[..a, b]",
        "[a, ..b, ..c]",
        "[..Some(a)]",
        "[..(a,b)]",
        "[..1]",
        "[..]",
        "[a ..rest]",
        "(a, ..tail, b)",
        "(..tail, ..rest)",
        "(..Some(a))",
        "[...rest]",
    ] {
        let source = format!("fn main():\n    match [1]:\n        {pattern} -> 0\n");
        let error = parse::parse(&source).unwrap_err();
        assert!(
            error.span.start <= error.span.end && error.span.end <= source.len(),
            "{error:?}"
        );
    }
}

#[test]
fn sequence_prefix_and_recursive_depth_limits_are_explicit() {
    for (open, close, rest) in [
        ("[", "]", ""),
        ("[", "]", ", ..tail"),
        ("(", ")", ", ..tail"),
    ] {
        let pattern = format!("{open}{}{rest}{close}", vec!["_"; 129].join(","));
        let source = format!("fn main():\n    match value:\n        {pattern} -> 0\n");
        assert!(parse::parse(&source).unwrap_err().message.contains("limit"));
    }
    let source = format!(
        "fn main():\n    match value:\n        {}_{} -> 0\n",
        "[".repeat(140),
        "]".repeat(140)
    );
    assert!(parse::parse(&source).unwrap_err().message.contains("limit"));
}

#[test]
fn sequence_ast_preserves_suffix_bindings_singletons_and_original_spans() {
    use morrow_compiler::ast::{ExprKind, PatternKind};
    let source = "fn main(): match value:\n    [🌿, ..tail] -> 0\n    (first, .._) -> 0\n    (only,) -> 0\n    () -> 0\n    [..all] -> 0\n    (..all) -> 0\n";
    let program = parse::parse(source).unwrap();
    let ExprKind::Match { arms, .. } = &program.functions[0].body.kind else {
        panic!("expected match")
    };
    let PatternKind::List {
        prefix,
        rest: Some(rest),
    } = &arms[0].pattern.kind
    else {
        panic!("expected list prefix")
    };
    assert!(matches!(&prefix[0].kind, PatternKind::Bind(name) if name == "🌿"));
    assert!(matches!(&rest.kind, PatternKind::Bind(name) if name == "tail"));
    assert_eq!(
        &source[arms[0].pattern.span.start..arms[0].pattern.span.end],
        "[🌿, ..tail]"
    );
    assert_eq!(&source[rest.span.start..rest.span.end], "tail");
    assert!(
        matches!(&arms[1].pattern.kind, PatternKind::TupleRest {prefix,rest} if prefix.len()==1 && matches!(rest.kind,PatternKind::Wildcard))
    );
    assert!(matches!(&arms[2].pattern.kind, PatternKind::Tuple(fields) if fields.len()==1));
    assert!(matches!(&arms[3].pattern.kind, PatternKind::Tuple(fields) if fields.is_empty()));
    assert!(matches!(&arms[4].pattern.kind, PatternKind::List {prefix,..} if prefix.is_empty()));
    assert!(
        matches!(&arms[5].pattern.kind, PatternKind::TupleRest {prefix,..} if prefix.is_empty())
    );
}

#[test]
fn sequence_prefixes_are_bounded_and_truncated_unicode_never_panics() {
    let source = "fn main(): match value:\n    Some(([🌿, ..rest], (flag, ..tail))) if List.len(rest)>0 -> 1\n    _ -> 0\n";
    for (end, _) in source
        .char_indices()
        .chain(std::iter::once((source.len(), ' ')))
    {
        if let Err(error) = parse::parse(&source[..end]) {
            assert!(
                error.span.start <= error.span.end && error.span.end <= end,
                "{error:?}"
            );
        }
    }
    let prefix = vec!["_"; 128].join(",");
    for pattern in [
        format!("[{prefix}]"),
        format!("[{prefix},..tail]"),
        format!("({prefix},..tail)"),
    ] {
        parse::parse(&format!("fn main(): match value:\n    {pattern} -> 0\n")).unwrap();
    }
}
