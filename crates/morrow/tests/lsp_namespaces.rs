use morrow_compiler::lsp;

fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\u{1}', "\\u0001")
    )
}
fn run(messages: Vec<String>) -> Vec<String> {
    use std::fmt::Write;
    let mut input = String::new();
    for message in messages {
        write!(
            &mut input,
            "Content-Length: {}\r\n\r\n{message}",
            message.len()
        )
        .unwrap();
    }
    let mut output = Vec::new();
    lsp::serve(std::io::Cursor::new(input), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    text.split("Content-Length: ")
        .skip(1)
        .map(|frame| frame.split_once("\r\n\r\n").unwrap().1.to_owned())
        .collect()
}
fn query(marked: &str, method: &str) -> String {
    let offset = marked.find('§').unwrap();
    let before = &marked[..offset];
    let line = before.bytes().filter(|b| *b == b'\n').count();
    let character = before.rsplit('\n').next().unwrap().encode_utf16().count();
    let source = marked.replace('§', "");
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"untitled:test","version":1,"text":{}}}}}}}"#,
            quote(&source)
        ),
        format!(
            r#"{{"jsonrpc":"2.0","id":"query","method":"textDocument/{method}","params":{{"textDocument":{{"uri":"untitled:test"}},"position":{{"line":{line},"character":{character}}}}}}}"#
        ),
        r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#.into(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.into(),
    ];
    run(messages)
        .into_iter()
        .find(|m| m.contains("\"id\":\"query\""))
        .unwrap()
}

#[test]
fn type_annotations_and_value_uses_keep_distinct_namespaces() {
    let source = "@doc \"\"\"The type.\"\"\"\ntype Box:\n    Wrap(Int)\n@doc \"\"\"The function.\"\"\"\nfn Box(value: Int) -> Int: value\nfn keep(value: Box) -> Box: value\nfn main():\n    let Box=3\n    let item: Box = Wrap(1)\n    println(Box)\n";
    for text in [
        source.replace("value: Box", "value: Bo§x"),
        source.replace("-> Box", "-> Bo§x"),
        source.replace("item: Box", "item: Bo§x"),
    ] {
        let hover = query(&text, "hover");
        assert!(
            hover.contains("type Box") && hover.contains("The type."),
            "{hover}"
        );
        assert!(!hover.contains("The function."), "{hover}");
        let definition = query(&text, "definition");
        assert!(definition.contains("\"line\":1"), "{definition}");
    }
    let hover = query(&source.replace("println(Box)", "println(Bo§x)"), "hover");
    assert!(
        hover.contains("Box: Int") && !hover.contains("The type."),
        "{hover}"
    );
}

#[test]
fn unannotated_arrow_body_is_value_syntax_after_type_lookahead() {
    let source = "type Box:\n    Wrap(Int)\nfn Box(x: Int) -> Int: x\nfn value() -> Bo§x(1)\nfn main(): println(value())\n";
    let response = query(source, "hover");
    assert!(
        response.contains("fn Box") && !response.contains("type Box"),
        "{response}"
    );
}

#[test]
fn type_docs_do_not_leak_to_undocumented_same_named_function() {
    let source =
        "@doc \"\"\"Type only.\"\"\"\ntype Box:\n    Wrap(Int)\nfn Bo§x(x: Int) -> Int: x\n";
    let response = query(source, "hover");
    assert!(
        response.contains("fn Box") && !response.contains("Type only."),
        "{response}"
    );
}
