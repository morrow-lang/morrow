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
fn definitions_respect_shadowing_initializers_and_clause_binders() {
    definition(
        "fn main():\n    let x=1\n    let x=x§+1\n    println(x)\n",
        1,
        8,
    );
    definition(
        "fn f(0: Int) -> 0\nfn f(x: Int) if x§>0 -> x\nfn f(_: Int) -> 0\nfn main(): println(f(2))\n",
        1,
        5,
    );
    definition(
        "fn main():\n    let x=1\n    let callback=(x: Int)->x§+1\n    println(callback(2))\n",
        2,
        18,
    );
}
#[test]
fn completion_is_scoped_and_builtin_prefixes_work_in_incomplete_source() {
    let result = query(
        "fn main():\n    let nearby=1\n    println(near§by)\n",
        "completion",
    );
    assert!(result.contains("\"label\":\"nearby\""), "{result}");
    assert!(result.contains("\"textEdit\""), "{result}");
    let result = query("fn main():\n    List.§\n", "completion");
    assert!(result.contains("\"label\":\"map\""), "{result}");
    assert!(!result.contains("\"label\":\"File.read\""));
    assert!(query("fn main(): # List.§\n", "completion").contains("\"items\":[]"));
    assert!(query("fn main(): println(\"List.§\")\n", "completion").contains("\"items\":[]"));
}
#[test]
fn navigation_uses_utf16_and_ignores_plain_strings() {
    definition("fn main():\n    let 🌿=1\n    println(🌿§)\n", 1, 8);
    assert!(query("fn main(): println(\"fn main§\")\n", "definition").contains("\"result\":null"));
}

#[test]
fn navigation_respects_match_loop_with_and_let_else_scope_boundaries() {
    definition(
        "fn main():\n    let outer=3\n    match [1,2]:\n        [head,..tail] if head§>0 -> println(head)\n        _ -> println(outer)\n",
        3,
        9,
    );
    definition(
        "fn main():\n    let value=[1]\n    for value in value§:\n        println(value)\n",
        1,
        8,
    );
    definition(
        "fn main():\n    for value in [1]:\n        println(value§)\n",
        1,
        8,
    );
    definition(
        "fn main():\n    let value=9\n    let Some(value)=None else: return println(value§)\n    println(value)\n",
        1,
        8,
    );
    definition(
        "fn main():\n    let Some(value)=Some(1) else: return ()\n    println(value§)\n",
        1,
        13,
    );
    definition(
        "fn main():\n    with\n        first <- Ok(1),\n        second <- Ok(first§)\n    do\n        println(second)\n    else\n        Err(_) -> ()\n",
        2,
        8,
    );
    definition(
        "fn main():\n    let first=9\n    with\n        first <- Ok(1)\n    do\n        println(first)\n    else\n        Err(_) -> println(first§)\n",
        1,
        8,
    );
}

#[test]
fn definition_does_not_confuse_record_members_with_same_named_functions() {
    let result = query(
        "fn member() -> Int: 9\ntype Box:\n    member: Int\nfn make() -> Box: Box(1)\nfn main(): println(make().member§)\n",
        "definition",
    );
    assert!(
        result.contains("\"start\":{\"character\":4,\"line\":2}"),
        "{result}"
    );
    let result = query(
        "fn member() -> Int: 9\ntype Box:\n    member: Int\nfn main():\n    let value=Box(1)\n    println(value.member§)\n",
        "definition",
    );
    assert!(
        result.contains("\"start\":{\"character\":4,\"line\":2}"),
        "{result}"
    );
    definition(
        "fn main():\n    let value=1\n    let callback=()->value§\n    println(callback())\n",
        1,
        8,
    );
}

#[test]
fn sibling_arm_and_future_bindings_are_absent_from_completion() {
    let result = query(
        "fn main():\n    match [1]:\n        [secret] -> println(secret)\n        _ -> println(sec§)\n    let second=2\n",
        "completion",
    );
    assert!(!result.contains("\"label\":\"secret\""), "{result}");
    assert!(!result.contains("\"label\":\"second\""), "{result}");
    definition(
        "fn main():\n    let x=1\n    let x§=2\n    println(x)\n",
        2,
        8,
    );
}

struct Project(std::path::PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-navigation-{}-{}",
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
fn imported_aliases_reexports_and_unsaved_sources_retain_definition_identity() {
    let project = Project::new();
    let model = project.file(
        "model.mr",
        "pub fn value() -> Int: 1\nfn hidden() -> Int: 2\n",
    );
    project.file("api.mr", "pub import model.{value}\n");
    let source = "import api as api\nfn main(): println(api.value())\n";
    let main = project.file("main.mr", source);
    let messages = session(vec![
        opening(&main, source),
        request(&main, "disk", "definition", 1, 27),
        opening(
            &model,
            "# unsaved\npub fn value() -> Int: 3\nfn hidden() -> Int: 2\n",
        ),
        request(&main, "overlay", "definition", 1, 27),
        request(&main, "members", "completion", 1, 27),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":{}}}}}}}"#,
            quote(&model)
        ),
        request(&main, "closed", "definition", 1, 27),
    ]);
    let disk = answer(&messages, "disk");
    assert!(disk.contains(&quote(&model)), "{disk}");
    assert!(disk.contains("\"line\":0"), "{disk}");
    let overlay = answer(&messages, "overlay");
    assert!(overlay.contains("\"line\":1"), "{overlay}");
    let members = answer(&messages, "members");
    assert!(members.contains("\"label\":\"value\""), "{members}");
    assert!(!members.contains("hidden"), "{members}");
    assert!(answer(&messages, "closed").contains("\"line\":0"));
}
#[test]
fn semantic_request_errors_and_source_changes_do_not_reuse_stale_positions() {
    let source = "fn main():\n    let x=1\n    println(x)\n";
    let messages = session(vec![
        opening("untitled:test", source),
        request("untitled:test", "old", "definition", 2, 13),
        request("untitled:test", "bad", "definition", 999, 0),
        request("untitled:missing", "missing", "completion", 0, 0),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"untitled:test","version":2}},"contentChanges":[{{"text":{}}}]}}}}"#,
            quote("fn main(): unknown(\n")
        ),
        request("untitled:test", "changed", "definition", 0, 15),
    ]);
    assert!(answer(&messages, "old").contains("\"uri\":\"untitled:test\""));
    assert!(answer(&messages, "bad").contains("-32602"));
    assert!(answer(&messages, "missing").contains("-32602"));
    assert!(answer(&messages, "changed").contains("\"result\":null"));
}

#[test]
fn interpolation_and_lambda_declarations_have_exact_source_definitions() {
    definition(
        "fn main():\n    let value=1\n    println(\"text {value§}\")\n",
        1,
        8,
    );
    definition(
        "fn main():\n    let callback=(value§: Int)->value\n    println(callback(1))\n",
        1,
        18,
    );
    definition(
        "fn main():\r\n    let 🌿=1\r\n    println(\"😀\", 🌿§)\r\n",
        1,
        8,
    );
}
#[test]
fn completion_is_bounded_and_does_not_split_unicode_replacement_ranges() {
    let mut source = String::from("fn main():\n");
    for i in 0..300 {
        source.push_str(&format!("    let item{i}=1\n"));
    }
    source.push_str("    println(item§)\n");
    let result = query(&source, "completion");
    assert_eq!(result.matches("\"label\":").count(), 256);
    assert!(result.contains("\"isIncomplete\":true"));
    let result = query(
        "fn main():\n    let 🌿leaf=1\n    println(🌿§leaf)\n",
        "completion",
    );
    assert!(result.contains("\"newText\":\"🌿leaf\""), "{result}");
    assert!(
        result.contains("\"end\":{\"character\":18,\"line\":2}"),
        "{result}"
    );
}
#[test]
fn newly_open_unsaved_dependencies_supply_navigation_without_disk_writes() {
    let project = Project::new();
    let source = "import fresh\nfn main(): println(fresh.value())\n";
    let main = project.file("main.mr", source);
    let fresh = format!(
        "file://{}/fresh.mr",
        project.0.canonicalize().unwrap().display()
    );
    let messages = session(vec![
        opening(&main, source),
        request(&main, "missing", "definition", 1, 29),
        opening(&fresh, "pub fn value() -> Int: 3\n"),
        request(&main, "fresh", "definition", 1, 29),
    ]);
    assert!(answer(&messages, "missing").contains("\"result\":null"));
    assert!(answer(&messages, "fresh").contains(&quote(&fresh)));
    assert!(!project.0.join("fresh.mr").exists());
}

#[test]
fn pattern_declarations_and_record_declarations_do_not_resolve_unrelated_names() {
    definition(
        "fn main():\n    let Some(value§)=Some(1) else: return ()\n    println(value)\n",
        1,
        13,
    );
    definition(
        "fn main():\n    with\n        first§ <- Ok(1)\n    do\n        println(first)\n    else\n        Err(_) -> ()\n",
        2,
        8,
    );
    definition(
        "fn member() -> Int: 1\ntype Box:\n    member§: Int\nfn main(): ()\n",
        2,
        4,
    );
}
#[test]
fn qualified_annotations_and_pipe_targets_resolve_through_import_aliases() {
    let project = Project::new();
    let model = project.file(
        "model.mr",
        "pub type Item:\n    value: Int\npub fn size(value: Int) -> Int: value\n",
    );
    let source = "import model as m\nfn read(value: m.Item) -> Int: value.value\nfn main(): println(1 |> m.size())\n";
    let main = project.file("main.mr", source);
    let messages = session(vec![
        opening(&main, source),
        request(&main, "type", "definition", 1, 20),
        request(&main, "pipe", "definition", 2, 28),
    ]);
    for id in ["type", "pipe"] {
        let result = answer(&messages, id);
        assert!(result.contains(&quote(&model)), "{result}");
    }
}

#[test]
fn completion_on_an_indented_blank_line_retains_current_block_locals() {
    let result = query("fn main():\n    let nearby=1\n    §\n", "completion");
    assert!(result.contains("\"label\":\"nearby\""), "{result}");
    let result = query(
        "fn earlier():\n    let secret=1\n    secret\nfn main():\n    §()\n",
        "completion",
    );
    assert!(!result.contains("\"label\":\"secret\""), "{result}");
}

#[test]
fn completion_payload_bytes_are_bounded_for_very_long_source_identifiers() {
    let source = format!("fn {}()->Int:1\nfn main():\n    §()\n", "x".repeat(530_000));
    let result = query(&source, "completion");
    assert!(
        result.len() < 1024 * 1024,
        "response bytes: {}",
        result.len()
    );
    assert!(result.contains("\"isIncomplete\":true"));
}

#[test]
fn compiler_owned_combinators_are_available_in_builtin_completion() {
    for (namespace, expected) in [
        ("List", vec!["find", "push", "concat"]),
        ("Option", vec!["map", "unwrap_or"]),
        ("Result", vec!["map", "and_then", "unwrap_or_else"]),
    ] {
        let result = query(&format!("fn main():\n    {namespace}.§\n"), "completion");
        for name in expected {
            assert!(
                result.contains(&format!("\"label\":\"{name}\"")),
                "{result}"
            );
        }
    }
}

#[test]
fn source_aliases_are_not_shadowed_by_canonical_module_names() {
    let project = Project::new();
    let model = project.file("model.mr", "pub fn value() -> Int: 1\n");
    let source = "import model as m\nfn main():\n    let model=3\n    println(m.value())\n";
    let main = project.file("main.mr", source);
    let messages = session(vec![
        opening(&main, source),
        request(&main, "member", "definition", 3, 17),
        request(&main, "alias", "definition", 3, 12),
    ]);
    for id in ["member", "alias"] {
        let result = answer(&messages, id);
        assert!(result.contains(&quote(&model)), "{result}");
        assert!(
            result.contains("\"start\":{\"character\":7,\"line\":0}"),
            "{result}"
        );
    }
}

#[test]
fn lambda_definition_ranges_exclude_annotations_and_allow_type_navigation() {
    definition(
        "type Box:\n    value: Int\nfn main():\n    let callback=(value: Box§)->value.value\n    println(callback(Box(1)))\n",
        0,
        5,
    );
    let result = query(
        "fn main():\n    let callback=(value: Int)->value§\n    println(callback(1))\n",
        "definition",
    );
    assert!(
        result.contains("\"start\":{\"character\":18,\"line\":1}"),
        "{result}"
    );
    assert!(
        result.contains("\"end\":{\"character\":23,\"line\":1}"),
        "{result}"
    );
}

#[test]
fn line_comment_completion_stays_excluded_at_its_terminal_caret() {
    for source in ["fn main(): # comment§", "fn main(): # comment§\n"] {
        let result = query(source, "completion");
        assert!(result.contains("\"items\":[]"), "{result}");
    }
    assert!(!query("fn main(): () /* comment */§\n", "completion").contains("\"items\":[]"));
}

#[test]
fn nested_blank_completion_preserves_only_the_current_indentation_scope() {
    for source in [
        "fn main():\n    if true:\n        let inside=1\n        §\n",
        "fn main():\n    if true:\n        let inside=1\n        # still inside\n        §\n",
        "fn main():\n    for inside in [1]:\n        println(inside)\n        §\n",
        "fn main():\n    match [1]:\n        [inside] ->\n            println(inside)\n            §\n        _ -> ()\n",
    ] {
        let result = query(source, "completion");
        assert!(
            result.contains("\"label\":\"inside\""),
            "{source}: {result}"
        );
    }
    for source in [
        "fn main():\n    if true:\n        let inside=1\n    §\n",
        "fn main():\n    if true:\n        let inside=1\n    else:\n        let other=2\n        §\n",
    ] {
        let result = query(source, "completion");
        assert!(
            !result.contains("\"label\":\"inside\""),
            "{source}: {result}"
        );
    }
}

#[test]
fn untitled_constructor_aliases_resolve_and_complete_like_file_modules() {
    definition(
        "type Choice:\n    Item(Int)\nfn main(): Choice.Item§(1)\n",
        1,
        4,
    );
    let result = query(
        "type Choice:\n    Item(Int)\nfn main(): Choice.It§em(1)\n",
        "completion",
    );
    assert!(result.contains("\"label\":\"Item\""), "{result}");
}
