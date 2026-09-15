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

fn labels(response: &str, expected: &[&str]) {
    for name in expected {
        assert!(
            response.contains(&format!("\"label\":\"{name}\"")),
            "{response}"
        );
    }
}

#[test]
fn incomplete_library_cannot_gain_evidence_from_a_fabricated_main() {
    let prefix = "type Box:\n    value:Int\nfn inspect(box:Box)->Unit:\n    let ignored=main()\n";
    let response = query(&format!("{prefix}    println(box.§)\n"), "completion");
    assert!(!response.contains("\"detail\":\"Int\""), "{response}");
    let response = query(&format!("{prefix}    println(bo§x.value)\n"), "hover");
    assert!(response.contains("\"result\":null"), "{response}");
    let source = "type Box:\n    value:Int\nfn inspect(box:Box)->Unit:\n    let main=() -> ()\n    main()\n    println(box.§)\n";
    let response = query(source, "completion");
    labels(&response, &["value"]);
    assert!(response.contains("\"detail\":\"Int\""), "{response}");
}

#[test]
fn library_diagnostics_report_free_main_references() {
    let responses = run(vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"untitled:library","version":1,"text":{}}}}}}}"#,
            quote("fn helper()->Unit:main()\n")
        ),
        r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#.into(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.into(),
    ]);
    assert!(
        responses
            .iter()
            .any(|message| message.contains("\"diagnostics\":[{") && message.contains("main")),
        "{responses:?}"
    );
}

#[test]
fn incomplete_record_member_uses_receiver_type_and_original_edit_range() {
    let prefix = "type Box(a):\n    value: a\nfn main():\n    let box=Box(1)\n";
    let response = query(&format!("{prefix}    println(box.§)\n"), "completion");
    labels(&response, &["value"]);
    assert!(response.contains("\"detail\":\"Int\""), "{response}");
    assert!(
        response.contains("\"character\":16,\"line\":4"),
        "{response}"
    );
    let response = query(&format!("{prefix}    println(box.va§)\n"), "completion");
    labels(&response, &["value"]);
    assert!(response.contains("\"newText\":\"value\""), "{response}");
    assert!(
        response.contains("\"character\":16,\"line\":4"),
        "{response}"
    );
    assert!(
        response.contains("\"character\":18,\"line\":4"),
        "{response}"
    );
}

#[test]
fn call_tuple_and_list_receivers_use_only_real_members() {
    let response = query(
        "type Box:\n    value: Int\nfn make() -> Box: Box(1)\nfn main(): println(make().§)\n",
        "completion",
    );
    labels(&response, &["value"]);
    let response = query("fn main(): println((1, \"text\").§)\n", "completion");
    labels(&response, &["0", "1"]);
    let response = query("fn main(): println([1,2].enu§)\n", "completion");
    labels(&response, &["enumerate"]);
    assert!(!response.contains("\"label\":\"map\""), "{response}");
}

#[test]
fn recovery_retains_current_lexical_scopes_in_clauses_and_lambdas() {
    for source in [
        "type Box:\n    value: Int\nfn inspect(Some(box): Option(Box)) -> Int: box.§\nfn inspect(None: Option(Box)) -> Int: 0\nfn main(): ()\n",
        "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    let callback = () -> box.§\n    println(callback())\n",
        "type Box:\n    value: Int\nfn main():\n    for box in [Box(1)]:\n        println(box.§)\n",
    ] {
        labels(&query(source, "completion"), &["value"]);
    }
}

#[test]
fn recovery_does_not_guess_unknown_receivers_or_hide_other_errors() {
    for source in [
        "type Box:\n    value: Int\nfn mystery(receiver): receiver.§\nfn main(): ()\n",
        "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    println(box.§)\n    missing\n",
        "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    println(box.§)\n    println(box.)\n",
        "type Box:\n    value: Int\nfn main(): println(missing.§)\n",
    ] {
        let response = query(source, "completion");
        assert!(!response.contains("\"label\":\"value\""), "{response}");
    }
}

#[test]
fn literal_text_comments_and_fake_field_names_are_not_recovery_sites() {
    for source in [
        "type Box:\n    value: Int\nfn main(): println(\"box.§\")\n",
        "type Box:\n    value: Int\nfn main(): () # box.§\n",
        "type Box:\n    value: Int\nfn main(): println(1.0§)\n",
    ] {
        assert!(!query(source, "completion").contains("\"label\":\"value\""));
    }
}

#[test]
fn interpolation_holes_and_unicode_ranges_use_original_source_coordinates() {
    let source =
        "type Box:\n    café: Int\nfn main():\n    let 🌿=Box(1)\n    println(\"{🌿.ca§}\")\n";
    let response = query(source, "completion");
    labels(&response, &["café"]);
    assert!(
        response.contains("\"character\":17,\"line\":4"),
        "{response}"
    );
    assert!(
        response.contains("\"character\":19,\"line\":4"),
        "{response}"
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
fn dependency_overlays_refresh_hole_receiver_types_without_stale_facts() {
    let project = Project::new();
    let model = project.file(
        "model.mr",
        "pub type Box:\n    value: Int\npub fn make() -> Box: Box(1)\n",
    );
    let source = "import model as m\nfn main(): println(m.make().)\n";
    let main = project.file("main.mr", source);
    let character = source.lines().nth(1).unwrap().find(".)").unwrap() + 1;
    let messages = session(vec![
        opening(&main, source),
        request(&main, "disk", "completion", 1, character),
        opening(
            &model,
            "pub type Box:\n    text: String\npub fn make() -> Box: Box(\"new\")\n",
        ),
        request(&main, "overlay", "completion", 1, character),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":{},"version":2}},"contentChanges":[{{"text":{}}}]}}}}"#,
            quote(&model),
            quote("pub type Box:\n    text: String\npub fn make() -> Box: missing\n")
        ),
        request(&main, "invalid", "completion", 1, character),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":{}}}}}}}"#,
            quote(&model)
        ),
        request(&main, "closed", "completion", 1, character),
    ]);
    labels(&answer(&messages, "disk"), &["value"]);
    let overlay = answer(&messages, "overlay");
    labels(&overlay, &["text"]);
    assert!(!overlay.contains("\"label\":\"value\""), "{overlay}");
    let invalid = answer(&messages, "invalid");
    assert!(!invalid.contains("\"label\":\"text\""), "{invalid}");
    labels(&answer(&messages, "closed"), &["value"]);
}

#[test]
fn maximum_record_recovery_respects_the_existing_source_field_limit() {
    let mut source = String::from("type Wide:\n");
    for field in 0..255 {
        source.push_str(&format!("    field{field:03}: Int\n"));
    }
    let tail = "fn inspect(value: Wide) -> Int: value.§\nfn main(): ()\n";
    let valid = format!("{source}{tail}");
    morrow_compiler::check::check(
        &morrow_compiler::parse::parse(&valid.replace('§', "field000")).unwrap(),
    )
    .unwrap();
    let response = query(&valid, "completion");
    assert_eq!(response.matches("\"label\":").count(), 255, "{response}");
    assert!(response.contains("\"isIncomplete\":false"), "{response}");
    source.push_str("    field255: Int\n");
    let response = query(&format!("{source}{tail}"), "completion");
    assert!(!response.contains("\"label\":\"field000\""), "{response}");
}

#[test]
fn hole_results_cannot_anchor_receivers_helpers_or_caller_signatures() {
    for source in [
        "type Box:\n    value: Int\nfn helper(receiver): receiver.§\nfn main(): println(helper(Box(1)))\n",
        "type Box:\n    value: Int\nfn main():\n    let callback = (receiver) -> receiver.§\n    println(callback(Box(1)))\n",
        "type Box:\n    value: Int\nfn helper(x): x\nfn main():\n    let box=Box(1)\n    let unused=[]\n    println(helper(box.§))\n",
    ] {
        assert!(!query(source, "completion").contains("\"label\":\"value\""));
    }
}

#[test]
fn unrelated_result_and_control_flow_errors_remain_errors_with_a_hole() {
    for source in [
        "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    let result: Result(Int, String)=Ok(1)\n    println(box.§)\n",
        "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    defer return ()\n    println(box.§)\n",
        "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    return ()\n    missing\n    println(box.§)\n",
    ] {
        assert!(!query(source, "completion").contains("\"label\":\"value\""));
    }
}

#[test]
fn partial_selector_replaces_the_whole_current_token_and_preserves_crlf() {
    let response = query(
        "type Box:\r\n    value: Int\r\nfn main():\r\n    let box=Box(1)\r\n    println(box.va§bad)\r\n",
        "completion",
    );
    labels(&response, &["value"]);
    assert!(
        response.contains("\"character\":16,\"line\":4"),
        "{response}"
    );
    assert!(
        response.contains("\"character\":21,\"line\":4"),
        "{response}"
    );
}

#[test]
fn complete_members_do_not_mask_errors_and_invalid_hover_remains_empty() {
    let source = "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    let bad: String=box.value§\n";
    assert!(!query(source, "completion").contains("\"label\":\"value\""));
    let source = "type Box:\n    value: Int\nfn main(): println(Box(1).§)\n";
    assert!(query(source, "hover").contains("\"result\":null"));
    assert!(query(source, "definition").contains("\"result\":null"));
}

#[test]
fn source_aliases_and_valid_control_flow_retain_concrete_receiver_evidence() {
    for source in [
        "type Box:\n    value: Int\ntype Alias=Box\nfn inspect(value: Alias) -> Int: value.§\nfn main(): ()\n",
        "type Box:\n    value: Int\nfn main():\n    let box=Box(1)\n    defer println(1)\n    println(box.§)\n    return ()\n",
        "type Box:\n    value: Int\nfn helper(value): value\nfn main():\n    let box=helper(Box(1))\n    println(helper(box.§))\n",
    ] {
        labels(&query(source, "completion"), &["value"]);
    }
}

#[test]
fn opaque_native_values_and_annotation_holes_have_no_invented_members() {
    for source in [
        "fn inspect(value: Tui.Tree) -> Unit: println(value.§)\nfn main(): ()\n",
        "fn inspect(value: json.Value) -> Unit: println(value.§)\nfn main(): ()\n",
        "type Box:\n    value: Int\nfn inspect(value: Box.§) -> Unit: ()\nfn main(): ()\n",
        "type Box:\n    value: Int\nfn inspect(value: Box) -> Int: value.va§\nfn main(): println(missing)\n",
    ] {
        let response = query(source, "completion");
        assert!(!response.contains("\"label\":\"value\""), "{response}");
        assert!(!response.contains("\"label\":\"children\""), "{response}");
    }
}

#[test]
fn let_else_and_with_bindings_are_checked_in_their_actual_scopes() {
    for source in [
        "type Box:\n    value: Int\nfn main():\n    let value: Option(Box)=Some(Box(1))\n    let Some(box)=value else: return ()\n    println(box.§)\n",
        "type Box:\n    value: Int\nfn main():\n    let value: Result(Box,String)=Ok(Box(1))\n    with\n        box <- value\n    do\n        println(box.§)\n    else\n        Err(_) -> ()\n",
    ] {
        labels(&query(source, "completion"), &["value"]);
    }
    let source = "type Box:\n    value: Int\nfn main():\n    let value: Result(Box,String)=Ok(Box(1))\n    with\n        box <- value\n    do\n        ()\n    else\n        Err(_) -> println(box.§)\n";
    assert!(!query(source, "completion").contains("\"label\":\"value\""));
}

#[test]
fn recovered_receiver_retains_import_identity_when_its_canonical_root_is_local() {
    let project = Project::new();
    project.file(
        "model.mr",
        "pub type Box:\n    value: Int\npub fn make() -> Box: Box(1)\n",
    );
    let source = "import model as m\nfn main():\n    let model=1\n    println(m.make().)\n";
    let main = project.file("main.mr", source);
    let character = source.lines().nth(3).unwrap().find(".)").unwrap() + 1;
    let messages = session(vec![
        opening(&main, source),
        request(&main, "query", "completion", 3, character),
    ]);
    labels(&answer(&messages, "query"), &["value"]);
}

#[test]
fn member_recovery_retains_required_labels_in_unaffected_source() {
    let prefix = "type Box:\n    value:Int\nfn add(left:Int,right:Int)->Int:left+right\n";
    for bad in [
        "fn unrelated()->Int:add(1,2)\n",
        "fn unrelated()->Int:1 |> add(right:2)\n",
    ] {
        let source = format!("{prefix}{bad}fn main():println(Box(1).§)\n");
        let response = query(&source, "completion");
        assert!(!response.contains("\"label\":\"value\""), "{response}");
    }
    let good =
        format!("{prefix}fn unrelated()->Int:add(left:1,right:2)\nfn main():println(Box(1).§)\n");
    labels(&query(&good, "completion"), &["value"]);
}
