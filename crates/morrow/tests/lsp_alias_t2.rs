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
fn alias_hover_keeps_source_declaration_docs_and_final_function_types() {
    let result = query(
        "@doc \"\"\"An identifier.\"\"\"\npub type Id§ = Int\nfn read(x: Id) -> Id: x\n",
        "hover",
    );
    assert!(result.contains("type Id = Int"), "{result}");
    assert!(result.contains("An identifier."), "{result}");
    let result = query("type Id = Int\nfn re§ad(x: Id) -> Id: x\n", "hover");
    assert!(result.contains("fn read(x: Int) -> Int"), "{result}");
    assert!(
        !result.contains("Infer") && !result.contains("$rigid"),
        "{result}"
    );
}
#[test]
fn type_aliases_and_local_values_keep_distinct_cursor_namespaces() {
    let result = query(
        "type Id = Int\nfn main():\n    let Id = \"local\"\n    let value: Id§ = 42\n    println(Id)\n",
        "definition",
    );
    assert!(result.contains("\"line\":0"), "{result}");
    let result = query(
        "type Id = Int\nfn main():\n    let Id = \"local\"\n    let value: Id = 42\n    println(Id§)\n",
        "hover",
    );
    assert!(result.contains("Id: String"), "{result}");
    let result = query("type Id = Int\nfn main(): I§\n", "completion");
    assert!(!result.contains("\"label\":\"Id\""), "{result}");
}
#[test]
fn an_alias_owns_its_docs_without_leaking_them_to_the_next_function() {
    let source = "@doc \"\"\"Alias-only docs.\"\"\"\ntype Alias = Int\nfn follow§ing(): 42\n";
    let result = query(source, "hover");
    assert!(result.contains("fn following() -> Int"), "{result}");
    assert!(!result.contains("Alias-only docs."), "{result}");
}
#[test]
fn project_docs_preserve_alias_signatures_and_own_source_docs() {
    use morrow_compiler::documentation::{Output, SourceDocument, render_project};
    let source = "@doc \"\"\"Alias-only docs.\"\"\"\npub type Id = Int\nfn following(): 42\n";
    let docs = render_project(
        &[SourceDocument {
            path: "values.fn",
            source,
        }],
        "Aliases",
        Output::Markdown,
    )
    .unwrap();
    assert!(docs.contains("pub type Id = Int"), "{docs}");
    assert_eq!(docs.matches("Alias-only docs.").count(), 1);
    assert!(docs.find("Alias-only docs.").unwrap() < docs.find("### following").unwrap());
}
