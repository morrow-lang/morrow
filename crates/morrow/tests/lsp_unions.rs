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
fn typed_pattern_binders_define_and_hover_at_the_inner_binding_token() {
    let source = "fn size(x:Int | String)->Int:\n    match x:\n        número:Int -> número\n        text:String -> String.len(text)\n";
    let definition = query(&source.replace("-> número", "-> nú§mero"), "definition");
    assert!(
        definition.contains("\"start\":{\"character\":8,\"line\":2}"),
        "{definition}"
    );
    assert!(
        definition.contains("\"end\":{\"character\":14,\"line\":2}"),
        "{definition}"
    );
    let hover = query(&source.replace("-> número", "-> nú§mero"), "hover");
    assert!(hover.contains("número: Int"), "{hover}");
    let completion = query(&source.replace("-> número", "-> nú§mero"), "completion");
    assert!(completion.contains("\"label\":\"número\""), "{completion}");
}
#[test]
fn narrowed_record_members_have_current_hover_and_definition_origins() {
    let source = "type Row:\n    value:Int\nfn size(x:Int | Row)->Int:\n    match x:\n        n:Int -> n\n        row:Row -> row.value\n";
    let definition = query(&source.replace("row.value", "row.val§ue"), "definition");
    assert!(
        definition.contains("\"start\":{\"character\":4,\"line\":1}"),
        "{definition}"
    );
    let hover = query(&source.replace("row.value", "row.val§ue"), "hover");
    assert!(hover.contains("value: Int"), "{hover}");
}
#[test]
fn union_signature_hover_is_plain_source_syntax_without_generated_tags() {
    let response = query("fn ident§ity(x:String | Int)->Int | String:x\n", "hover");
    assert!(
        response.contains("fn identity(x: Int | String) -> Int | String"),
        "{response}"
    );
    assert!(!response.contains("UnionInject") && !response.contains("$inferred"));
}
#[test]
fn typed_annotation_uses_type_namespace_and_binder_scope_ends_with_its_arm() {
    let source = "type Text=String\nfn text(x:Int)->Int:x\nfn size(x:Int | Text)->Int:\n    match x:\n        text:Int -> text\n        other:Text -> String.len(other)\n";
    let definition = query(&source.replace("other:Text", "other:Te§xt"), "definition");
    assert!(
        definition.contains("\"start\":{\"character\":5,\"line\":0}"),
        "{definition}"
    );
    let definition = query(&source.replace("-> text", "-> te§xt"), "definition");
    assert!(
        definition.contains("\"start\":{\"character\":8,\"line\":4}"),
        "{definition}"
    );
}
#[test]
fn incomplete_members_use_only_concretely_narrowed_current_receivers() {
    let source = "type Row:\n    value:Int\nfn size(x:Int | Row)->Int:\n    match x:\n        n:Int -> n\n        row:Row -> row.va§\n";
    let result = query(source, "completion");
    assert!(result.contains("\"label\":\"value\""), "{result}");
    assert!(result.contains("\"detail\":\"Int\""), "{result}");
    let invalid = source.replace("n:Int -> n", "n:Int -> missing");
    let result = query(&invalid, "completion");
    assert!(!result.contains("\"label\":\"value\""), "{result}");
}
