use morrow_compiler::lsp;

fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
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
fn argument_labels_do_not_resolve_to_same_spelled_caller_locals() {
    let source =
        "fn f(value:Int)->Int:value\nfn main():\n    let value=2\n    println(f(val§ue:3))\n";
    let result = query(source, "definition");
    assert!(result.contains("\"character\":5,\"line\":0"), "{result}");
}
#[test]
fn explicit_external_label_does_not_replace_a_pattern_binding() {
    let source = "fn f(input value:Int)->Int:val§ue\nfn main():println(f(input:3))\n";
    let result = query(source, "definition");
    assert!(result.contains("\"character\":11,\"line\":0"), "{result}");
}

#[test]
fn external_label_declarations_do_not_refer_to_unrelated_globals() {
    let source =
        "fn input()->Int:1\nfn f(in§put value:Int)->Int:value\nfn main():println(f(input:3))\n";
    let result = query(source, "definition");
    assert!(result.contains("\"character\":5,\"line\":1"), "{result}");
}

#[test]
fn reordered_labels_and_pipe_holes_have_exact_parameter_identity() {
    for call in ["sum(right:2, le§ft:1)", "1 |> sum(right:2, le§ft:_)"] {
        let source =
            format!("fn sum(left:Int,right:Int)->Int:left+right\nfn main():println({call})\n");
        let result = query(&source, "definition");
        assert!(result.contains("\"character\":7,\"line\":0"), "{result}");
        let hover = query(&source, "hover");
        assert!(hover.contains("left: Int"), "{hover}");
        assert!(!hover.contains("fn sum"), "{hover}");
    }
}
#[test]
fn clause_labels_use_first_contributing_interface_and_keep_inner_bindings() {
    let source = "fn choose(enabled true:Bool)->Int:1\nfn choose(enabled false:Bool)->Int:0\nfn main():println(choose(ena§bled:true))\n";
    let result = query(source, "definition");
    assert!(result.contains("\"character\":10,\"line\":0"), "{result}");
    let declaration = source
        .replace("ena§bled:true", "enabled:true")
        .replace("enabled false", "ena§bled false");
    assert!(query(&declaration, "definition").contains("\"character\":10,\"line\":0"));
}
#[test]
fn label_hover_preserves_declared_generic_identity() {
    let source = "fn same(left:a,right:a)->a:left\nfn main():println(same(le§ft:1,right:2))\n";
    let result = query(source, "hover");
    assert!(result.contains("left: a"), "{result}");
    assert!(!result.contains("left: Int"), "{result}");
}
#[test]
fn label_identity_uses_utf16_ranges() {
    let source = "fn select(🌿 value:Int)->Int:value\nfn main():println(select(§🌿:1))\n";
    let result = query(source, "definition");
    assert!(result.contains("\"character\":10,\"line\":0"), "{result}");
    assert!(result.contains("\"character\":12,\"line\":0"), "{result}");
}
#[test]
fn invalid_or_erased_calls_never_invent_source_label_targets() {
    for body in [
        "let f=(value)->value\n    println(f(value:1))",
        "let call=f\n    println(call(value:1))",
        "println(f(unknown:1))",
        "let value=1\n    println(value |> f(unknown:_))",
        "println(f(value:1))\n    missing()",
    ] {
        let source = format!("fn f(value:Int)->Int:value\nfn main():\n    {body}\n");
        let marked = if source.contains("unknown:") {
            source.replace("unknown:", "un§known:")
        } else {
            source.replace("value:1", "val§ue:1")
        };
        for method in ["definition", "hover"] {
            let result = query(&marked, method);
            assert!(result.contains("\"result\":null"), "{marked}: {result}");
        }
    }
}

#[test]
fn inherited_and_parenthesized_binding_labels_keep_exact_identifier_spans() {
    let inherited = "fn size(None:Option(Int))->Int:0\nfn size(input Some(_):Option(Int))->Int:1\nfn main():println(size(in§put:Some(1)))\n";
    let result = query(inherited, "definition");
    assert!(result.contains("\"character\":8,\"line\":1"), "{result}");
    let grouped = "fn f((value):Int)->Int:value\nfn main():println(f(val§ue:1))\n";
    let result = query(grouped, "definition");
    assert!(result.contains("\"character\":6,\"line\":0"), "{result}");
    assert!(result.contains("\"character\":11,\"line\":0"), "{result}");
}
