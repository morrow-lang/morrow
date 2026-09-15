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
fn newtype_owner_constructor_and_payload_references_have_distinct_source_targets() {
    definition(
        "newtype Box = Packed(Int)\nfn read(x: Box§) -> Int: x.0\nfn main(): ()\n",
        0,
        8,
    );
    definition(
        "newtype Box = Packed(Int)\nfn main(): println(Packed§(1).0)\n",
        0,
        14,
    );
    definition(
        "type Raw=Int\nnewtype Box=Packed(Raw§)\nfn main(): ()\n",
        0,
        5,
    );
    let result = query(
        "newtype Box=Packed(Int)\nfn main(): println(Pac§ked(1).0)\n",
        "completion",
    );
    assert!(result.contains("\"label\":\"Packed\""), "{result}");
}

#[test]
fn newtype_hover_keeps_owner_documentation_separate_from_same_spelled_function() {
    let source = "@doc \"\"\"Type documentation.\"\"\"\nnewtype Box=Packed(Int)\n@doc \"\"\"Function documentation.\"\"\"\nfn Box(value: Int) -> Int: value\nfn read(value: Box) -> Int: value.0\nfn main(): println(Box(1))\n";
    let owner = query(&source.replace("value: Box)", "value: Box§)"), "hover");
    assert!(owner.contains("newtype Box = Packed(Int)"), "{owner}");
    assert!(owner.contains("Type documentation."), "{owner}");
    assert!(!owner.contains("Function documentation."), "{owner}");
    let function = query(&source.replace("Box(1)", "Box§(1)"), "hover");
    assert!(function.contains("fn Box("), "{function}");
    assert!(function.contains("Function documentation."), "{function}");
    assert!(!function.contains("Type documentation."), "{function}");
}

#[test]
fn instantiated_newtype_accessor_hover_and_completion_use_the_inner_type() {
    let result = query(
        "newtype Box(a)=Packed(a)\nfn main():\n    let value=Packed(1)\n    println(value.0§)\n",
        "hover",
    );
    assert!(result.contains("0: Int"), "{result}");
    let result = query(
        "newtype Box(a)=Packed(a)\nfn main():\n    let value=Packed(1)\n    println(value.§0)\n",
        "completion",
    );
    assert!(result.contains("\"label\":\"0\""), "{result}");
    assert!(result.contains("\"detail\":\"Int\""), "{result}");
}

#[test]
fn shared_type_constructor_spelling_and_accessor_definition_are_unambiguous() {
    let prefix = "@doc \"\"\"A distinct identifier.\"\"\"\nnewtype UserId = UserId(Int)\n";
    let owner = query(
        &format!("{prefix}fn read(value: UserId§) -> Int: value.0\nfn main(): ()\n"),
        "hover",
    );
    assert!(owner.contains("newtype UserId = UserId(Int)"), "{owner}");
    let constructor = query(
        &format!("{prefix}fn main(): println(UserId§(1).0)\n"),
        "hover",
    );
    assert!(
        constructor.contains("UserId: (Int) -> UserId"),
        "{constructor}"
    );
    assert!(
        constructor.contains("A distinct identifier."),
        "{constructor}"
    );
    definition(
        "newtype Box = Packed(Int)\nfn main(): println(Packed(1).0§)\n",
        0,
        21,
    );
}

#[test]
fn imported_newtype_type_and_constructor_navigate_to_distinct_original_tokens() {
    let directory = std::env::temp_dir().join(format!("morrow-lsp-newtype-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let declaration = "pub newtype Wrapper(a) = Packed(a)\n";
    std::fs::write(directory.join("ids.mr"), declaration).unwrap();
    let source = "import ids as api\nfn read(value: api.Wrapper(Int)) -> Int: value.0\nfn main(): println(api.Packed(1).0)\n";
    std::fs::write(directory.join("main.mr"), source).unwrap();
    let uri = format!(
        "file://{}",
        directory.join("main.mr").canonicalize().unwrap().display()
    );
    let target = format!(
        "file://{}",
        directory.join("ids.mr").canonicalize().unwrap().display()
    );
    let mut messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"version":1,"text":{}}}}}}}"#,
            quote(&uri),
            quote(source)
        ),
    ];
    for (line, word) in [(1, "Wrapper"), (2, "Packed")] {
        let character = source.lines().nth(line).unwrap().find(word).unwrap() + 1;
        messages.push(format!(r#"{{"jsonrpc":"2.0","id":"{word}","method":"textDocument/definition","params":{{"textDocument":{{"uri":{}}},"position":{{"line":{line},"character":{character}}}}}}}"#,quote(&uri)));
    }
    messages.push(r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#.into());
    messages.push(r#"{"jsonrpc":"2.0","method":"exit"}"#.into());
    let responses = run(messages);
    std::fs::remove_dir_all(directory).unwrap();
    for word in ["Wrapper", "Packed"] {
        let response = responses
            .iter()
            .find(|response| response.contains(&format!("\"id\":\"{word}\"")))
            .unwrap();
        assert!(response.contains(&quote(&target)), "{response}");
        let character = declaration.find(word).unwrap();
        assert!(
            response.contains(&format!(
                "\"start\":{{\"character\":{character},\"line\":0}}"
            )),
            "{response}"
        );
    }
}

#[test]
fn incomplete_newtype_members_preserve_recovery_proof_and_unrelated_errors() {
    for source in [
        "newtype UserId=UserId(Int)\nfn main():\n    let value=UserId(7)\n    value.§\n",
        "newtype UserId=UserId(Int)\nnewtype Box(a)=Box(a)\nfn main():\n    let value=UserId(7)\n    let wrapped=Box(value.§)\n    ()\n",
    ] {
        let completion = query(source, "completion");
        assert!(completion.contains("\"label\":\"0\""), "{completion}");
        assert!(completion.contains("\"detail\":\"Int\""), "{completion}");
    }
    let invalid = "newtype UserId=UserId(Int)\nfn main():\n    let value=UserId(7)\n    let bad: Int=true\n    value.§\n";
    let completion = query(invalid, "completion");
    assert!(!completion.contains("\"label\":\"0\""), "{completion}");
}
