use fern_compiler::lsp;

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
struct Project(std::path::PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fern-navigation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, source: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, source).unwrap();
        format!("file://{}", path.canonicalize().unwrap().display())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn opening(uri: &str, source: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"version":1,"text":{}}}}}}}"#,
        quote(uri),
        quote(source)
    )
}
fn request(uri: &str, id: &str, method: &str, line: usize, character: usize) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"method":"textDocument/{method}","params":{{"textDocument":{{"uri":{} }},"position":{{"line":{line},"character":{character}}}}}}}"#,
        quote(id),
        quote(uri)
    )
}
fn session(mut messages: Vec<String>) -> Vec<String> {
    messages.insert(
        0,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into(),
    );
    messages.push(r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#.into());
    messages.push(r#"{"jsonrpc":"2.0","method":"exit"}"#.into());
    run(messages)
}
fn answer(messages: &[String], id: &str) -> String {
    messages
        .iter()
        .find(|m| m.contains(&format!("\"id\":{}", quote(id))))
        .unwrap()
        .clone()
}

#[test]
fn canonical_root_locals_do_not_change_alias_hover_or_definition_targets() {
    let project = Project::new();
    let model = project.file(
        "model.fn",
        "@doc \"\"\"Global docs.\"\"\"\npub fn value(x: a) -> a: x\n",
    );
    let source = "import model as m\nfn main():\n    let model=3\n    let callable=m.value\n    let result=m.value(model)\n    let piped=model |> m.value()\n    println(callable(result)+piped)\n";
    let main = project.file("main.fn", source);
    let mut requests = vec![opening(&main, source)];
    for line in [3, 4, 5] {
        let character = source.lines().nth(line).unwrap().find("m.value").unwrap() + 4;
        requests.push(request(
            &main,
            &format!("hover{line}"),
            "hover",
            line,
            character,
        ));
        requests.push(request(
            &main,
            &format!("definition{line}"),
            "definition",
            line,
            character,
        ));
    }
    let messages = session(requests);
    for line in [3, 4, 5] {
        let hover = answer(&messages, &format!("hover{line}"));
        assert!(
            hover.contains("Global docs.") && hover.contains("fn model.value"),
            "{hover}"
        );
        assert!(answer(&messages, &format!("definition{line}")).contains(&model));
    }
    let hover = answer(&messages, "hover4");
    assert!(hover.contains("At this use: (Int) -> Int"), "{hover}");
}
