use check::editor::{Query, analyze};
use morrow_compiler::{Span, Type, check, parse};

fn facts(
    source: &str,
    marked: &str,
    binding: Option<&str>,
    function: Option<&str>,
) -> check::editor::Facts {
    let start = source.find(marked).unwrap();
    let binding = binding.map(|name| {
        let start = source.find(name).unwrap();
        Span {
            start,
            end: start + name.len(),
        }
    });
    analyze(
        &parse::parse(source).unwrap(),
        Query {
            occurrence: Span {
                start,
                end: start + marked.len(),
            },
            binding,
            function: function.map(str::to_owned),
        },
    )
    .unwrap()
}

#[test]
fn declarations_publish_unused_generic_schemes_and_requirements() {
    let source = "fn identity(x): x\nfn twice(x): x+x\nfn main(): ()\n";
    let info = facts(source, "identity", None, Some("identity"))
        .function
        .unwrap();
    assert_eq!(info.parameters.len(), 1);
    assert_eq!(info.parameters[0], info.result);
    assert!(!info.generics.is_empty());
    let info = facts(source, "twice", None, Some("twice"))
        .function
        .unwrap();
    assert!(
        info.requirements
            .iter()
            .any(|r| r.message.contains("addition"))
    );
}

#[test]
fn binders_keep_source_origins_and_finalize_context_inference() {
    let source = "fn main():\n    let count=1\n    let count=\"text\"\n    println(count)\n";
    let start = source.rfind("count").unwrap();
    let origin = source.find("count=\"").unwrap();
    let info = analyze(
        &parse::parse(source).unwrap(),
        Query {
            occurrence: Span {
                start,
                end: start + 5,
            },
            binding: Some(Span {
                start: origin,
                end: origin + 5,
            }),
            function: None,
        },
    )
    .unwrap();
    assert_eq!(info.value.unwrap(), Type::String);
    let source = "fn first([head,..tail]: List(Int)) -> Int: head\nfn first([]: List(Int)) -> Int: 0\nfn main(): println(first([4]))\n";
    assert_eq!(
        facts(source, "head", Some("head"), None).value,
        Some(Type::Int)
    );
}

#[test]
fn calls_publish_their_instantiated_callable_type() {
    let source = "fn identity(x): x\nfn main(): println(identity(1))\n";
    let start = source.rfind("identity").unwrap();
    let info = analyze(
        &parse::parse(source).unwrap(),
        Query {
            occurrence: Span {
                start,
                end: start + 8,
            },
            binding: None,
            function: Some("identity".into()),
        },
    )
    .unwrap();
    assert_eq!(
        info.value,
        Some(Type::Function(vec![Type::Int], Box::new(Type::Int)))
    );
    assert_ne!(info.function.unwrap().parameters[0], Type::Int);
}

#[test]
fn record_members_use_instantiated_layouts_and_source_fields() {
    let source =
        "type Box(a):\n    value: a\nfn main():\n    let box=Box(4)\n    println(box.value)\n";
    let start = source.rfind("value").unwrap();
    let info = analyze(
        &parse::parse(source).unwrap(),
        Query {
            occurrence: Span {
                start,
                end: start + 5,
            },
            binding: None,
            function: None,
        },
    )
    .unwrap();
    assert_eq!(info.value, Some(Type::Int));
    assert_eq!(info.members.len(), 1);
    assert_eq!(info.members[0].name, "value");
    assert_eq!(info.members[0].ty, Type::Int);
    assert!(info.members[0].origin.is_some());
}

#[test]
fn no_metadata_escapes_failed_ordinary_checks_or_invalid_queries() {
    let program = parse::parse("fn identity(x): x\nfn main(): missing\n").unwrap();
    assert!(
        analyze(
            &program,
            Query {
                occurrence: Span { start: 3, end: 11 },
                binding: None,
                function: Some("identity".into())
            }
        )
        .is_err()
    );
    let program = parse::parse("fn main(): ()\n").unwrap();
    assert!(
        analyze(
            &program,
            Query {
                occurrence: Span { start: 9, end: 4 },
                binding: None,
                function: None
            }
        )
        .is_err()
    );
}

#[test]
fn malformed_host_spans_cannot_overflow_source_fact_selection() {
    let mut program = parse::parse("fn value(x: Int) -> Int: x\nfn main(): ()\n").unwrap();
    program.functions[0].span = Span {
        start: 0,
        end: usize::MAX,
    };
    assert!(
        analyze(
            &program,
            Query {
                occurrence: Span { start: 3, end: 8 },
                binding: None,
                function: Some("value".into())
            }
        )
        .is_err()
    );
    let program = parse::parse("fn main(): ()\n").unwrap();
    assert!(
        analyze(
            &program,
            Query {
                occurrence: Span {
                    start: usize::MAX,
                    end: usize::MAX
                },
                binding: None,
                function: None
            }
        )
        .is_err()
    );
}

#[test]
fn metadata_budget_can_refuse_large_valid_signatures_without_partial_facts() {
    let tuple = format!("({})", vec!["Int"; 1024].join(","));
    let parameters = (0..8)
        .map(|i| format!("x{i}: {tuple}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("fn wide({parameters}) -> (): ()\nfn main(): ()\n");
    let program = parse::parse(&source).unwrap();
    check::check(&program).unwrap();
    let result = analyze(
        &program,
        Query {
            occurrence: Span { start: 3, end: 7 },
            binding: None,
            function: Some("wide".into()),
        },
    );
    assert!(result.unwrap_err().message.contains("metadata limit"));
}
