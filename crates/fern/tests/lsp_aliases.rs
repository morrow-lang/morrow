use fern_compiler::lsp;

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
fn definition(marked: &str, line: usize, character: usize) {
    let result = query(marked, "definition");
    assert!(
        result.contains("\"uri\":\"untitled:test\""),
        "{marked}: {result}"
    );
    assert!(
        result.contains(&format!(
            "\"start\":{{\"character\":{character},\"line\":{line}}}"
        )),
        "{result}"
    );
}

#[test]
fn alias_declarations_and_target_references_have_source_identity() {
    definition(
        "type Id = Int\nfn read(x: Id§) -> Int: x\nfn main(): ()\n",
        0,
        5,
    );
    definition(
        "type Id = Int\ntype Pair = (Id§, Id)\nfn main(): ()\n",
        0,
        5,
    );
    let result = query(
        "type Id = Int\nfn read(x: I§d) -> Int: x\nfn main(): ()\n",
        "completion",
    );
    assert!(result.contains("\"label\":\"Id\""), "{result}");
}
#[test]
fn alias_boundaries_do_not_extend_preceding_function_locals() {
    let result = query(
        "fn before():\n    let secret=1\n    secret\ntype Alias = sec§ret\nfn main(): ()\n",
        "completion",
    );
    assert!(!result.contains("\"label\":\"secret\""), "{result}");
}
