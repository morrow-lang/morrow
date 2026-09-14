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
struct Project(std::path::PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-label-navigation-{}-{}",
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
fn module_labels_follow_original_alias_identity_and_live_overlays() {
    let project = Project::new();
    let disk = "pub fn choose(enabled value:Bool)->Int:if value:1 else:0\n";
    let live = "\n# current buffer\npub fn choose(enabled value:Bool)->Int:if value:1 else:0\n";
    let model = project.file("model.fn", disk);
    project.file("api.fn", "pub import model.{choose}\n");
    let source = "import api as m\nfn main():\n    let model=3\n    println(m.choose(enabled:true))\n    println(true |> m.choose(enabled:_))\n";
    let main = project.file("main.fn", source);
    let mut messages = vec![opening(&model, live), opening(&main, source)];
    for line in [3, 4] {
        let col = source.lines().nth(line).unwrap().find("enabled").unwrap() + 2;
        messages.push(request(
            &main,
            &format!("def{line}"),
            "definition",
            line,
            col,
        ));
        messages.push(request(&main, &format!("hover{line}"), "hover", line, col));
    }
    messages.push(format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":{}}}}}}}"#,quote(&model)));
    let col = source.lines().nth(3).unwrap().find("enabled").unwrap() + 2;
    messages.push(request(&main, "closed", "definition", 3, col));
    let output = session(messages);
    for line in [3, 4] {
        let result = answer(&output, &format!("def{line}"));
        assert!(result.contains(&model), "{result}");
        assert!(result.contains("\"character\":14,\"line\":2"), "{result}");
        assert!(answer(&output, &format!("hover{line}")).contains("enabled: Bool"));
    }
    assert!(answer(&output, "closed").contains("\"character\":14,\"line\":0"));
}

#[test]
fn stale_or_invalid_dependency_buffers_cannot_publish_label_facts() {
    let project = Project::new();
    let source = "import model as m\nfn main():println(m.choose(enabled:true))\n";
    let good = "pub fn choose(enabled value:Bool)->Int:if value:1 else:0\n";
    let model = project.file("model.fn", good);
    let main = project.file("main.fn", source);
    let col = source.lines().nth(1).unwrap().find("enabled").unwrap() + 2;
    let change = |version, text: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":{},"version":{version}}},"contentChanges":[{{"text":{}}}]}}}}"#,
            quote(&model),
            quote(text)
        )
    };
    let output = session(vec![
        opening(&model, good),
        opening(&main, source),
        change(3, "pub fn choose(enabled value:Bool)->Int:missing()\n"),
        request(&main, "bad", "definition", 1, col),
        change(2, good),
        request(&main, "stale", "hover", 1, col),
        change(4, good),
        request(&main, "fixed", "definition", 1, col),
    ]);
    assert!(answer(&output, "bad").contains("\"result\":null"));
    assert!(answer(&output, "stale").contains("\"result\":null"));
    assert!(answer(&output, "fixed").contains(&model));
}

#[test]
fn private_and_lexically_shadowed_module_interfaces_are_not_guessed() {
    for (declaration, body) in [
        (
            "fn choose(enabled value:Bool)->Int:1\n",
            "println(m.choose(enabled:true))",
        ),
        (
            "pub fn choose(enabled value:Bool)->Int:1\n",
            "let m=1\n    println(m.choose(enabled:true))",
        ),
    ] {
        let project = Project::new();
        project.file("model.fn", declaration);
        let source = format!("import model as m\nfn main():\n    {body}\n");
        let main = project.file("main.fn", &source);
        let line = source.lines().count() - 1;
        let col = source.lines().nth(line).unwrap().find("enabled").unwrap() + 2;
        let output = session(vec![
            opening(&main, &source),
            request(&main, "query", "definition", line, col),
        ]);
        assert!(answer(&output, "query").contains("\"result\":null"));
    }
}
