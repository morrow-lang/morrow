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

fn names(response: &str, expected: &[&str]) {
    for name in expected {
        assert!(
            response.contains(&format!("\"label\":\"{name}\"")),
            "{response}"
        );
    }
}
#[test]
fn empty_and_partial_arguments_offer_declared_external_names() {
    let prefix = "fn sum(left:Int,right:Int)->Int:left+right\n";
    for call in ["sum(§)", "sum(le§)", "sum(right:2,le§)"] {
        let response = query(
            &format!("{prefix}fn main():println({call})\n"),
            "completion",
        );
        names(&response, &["left"]);
        assert!(response.contains("\"newText\":\"left: \""), "{response}");
    }
}
#[test]
fn occupied_slots_and_written_suffixes_are_excluded() {
    let prefix = "fn render(text:String,left:Int,right:Int)->String:text\n";
    let response = query(
        &format!("{prefix}fn main():println(render(\"x\",§,right:2))\n"),
        "completion",
    );
    names(&response, &["left"]);
    for name in ["text", "right"] {
        assert!(
            !response.contains(&format!("\"label\":\"{name}\"")),
            "{response}"
        );
    }
}
#[test]
fn existing_label_edits_preserve_colon_value_and_utf16_coordinates() {
    let response = query(
        "fn select(🌿 value:Int)->Int:value\nfn main():println(select(§🌿:1))\n",
        "completion",
    );
    names(&response, &["🌿"]);
    assert!(response.contains("\"newText\":\"🌿\""), "{response}");
    assert!(
        response.contains(
            "\"end\":{\"character\":27,\"line\":1},\"start\":{\"character\":25,\"line\":1}"
        ),
        "{response}"
    );
}
#[test]
fn external_patterns_and_pipe_holes_use_source_interfaces() {
    let response = query(
        "fn choose(enabled true:Bool)->Int:1\nfn choose(enabled false:Bool)->Int:0\nfn main():println(choose(ena§))\n",
        "completion",
    );
    names(&response, &["enabled"]);
    let response = query(
        "fn sum(left:Int,right:Int)->Int:left+right\nfn main():println(1 |> sum(right:2,le§))\n",
        "completion",
    );
    names(&response, &["left"]);
}
#[test]
fn local_callables_and_text_do_not_inherit_unrelated_interfaces() {
    let prefix = "fn choose(enabled:Bool)->Int:1\n";
    for body in [
        "let choose=(x)->x\n    choose(ena§)",
        "let call=choose\n    call(ena§)",
        "println(\"choose(ena§)\")",
        "() # choose(ena§)",
        "println(ena§)",
    ] {
        let response = query(&format!("{prefix}fn main():\n    {body}\n"), "completion");
        assert!(!response.contains("\"label\":\"enabled\""), "{response}");
    }
}

#[test]
fn unclosed_calls_recover_only_a_real_final_argument_selector() {
    let prefix = "fn sum(left:Int,right:Int)->Int:left+right\n";
    for call in ["sum(§", "sum(le§", "sum(right:2,le§", "println(sum(le§"] {
        let response = query(&format!("{prefix}fn main():{call}"), "completion");
        names(&response, &["left"]);
    }
    for call in [
        "sum(\"le§",
        "sum(left:§",
        "sum([le§",
        "sum(le§\nfn unrelated(): ()",
    ] {
        let response = query(&format!("{prefix}fn main():{call}"), "completion");
        assert!(!response.contains("\"newText\":\"left: \""), "{response}");
    }
}
#[test]
fn inconsistent_source_interfaces_do_not_publish_arbitrary_names() {
    for prefix in [
        "fn f(a:Int)->Int:a\nfn f(b:Int)->Int:b\n",
        "fn f(a:Int,b:Int)->Int:a\nfn f(a:Int)->Int:a\n",
        "fn f(a:Int,a:Int)->Int:a\n",
        "fn f(a:Int)->Int:a\nfn other():()\nfn f(a:Int)->Int:a\n",
    ] {
        let response = query(&format!("{prefix}fn main():f(§)\n"), "completion");
        assert!(!response.contains("\"newText\":\"a: \""), "{response}");
    }
}
#[test]
fn explicit_pipe_placeholders_reserve_their_original_source_slots() {
    let prefix = "fn f(first:Int,second:Int,third:Int)->Int:first\n";
    for (call, wanted, forbidden) in [
        ("1 |> f(_,§,third:3)", "second", "first"),
        ("1 |> f(third:_,§)", "first", "third"),
    ] {
        let response = query(&format!("{prefix}fn main():{call}\n"), "completion");
        names(&response, &[wanted]);
        assert!(
            !response.contains(&format!("\"newText\":\"{forbidden}: \"")),
            "{response}"
        );
    }
}

struct Project(std::path::PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fern-label-site-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        format!("file://{}", path.canonicalize().unwrap().display())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn open(uri: &str, source: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"version":1,"text":{}}}}}}}"#,
        quote(uri),
        quote(source)
    )
}
fn complete(uri: &str, id: &str, marked: &str) -> String {
    let before = &marked[..marked.find('§').unwrap()];
    let line = before.bytes().filter(|b| *b == b'\n').count();
    let character = before.rsplit('\n').next().unwrap().encode_utf16().count();
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"method":"textDocument/completion","params":{{"textDocument":{{"uri":{}}},"position":{{"line":{line},"character":{character}}}}}}}"#,
        quote(id),
        quote(uri)
    )
}
fn file_session(mut messages: Vec<String>) -> Vec<String> {
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
fn module_aliases_use_current_overlay_interfaces_and_restore_disk_on_close() {
    let project = Project::new();
    let api = project.file("api.fn", "pub fn real(external value:Int)->Int:value\n");
    let marked = "import api as lib\nfn main():lib.real(§)\n";
    let main = project.file("main.fn", &marked.replace('§', ""));
    let messages = file_session(vec![
        open(&main, &marked.replace('§', "")),
        complete(&main, "disk", marked),
        open(&api, "pub fn real(updated value:Int)->Int:value\n"),
        complete(&main, "overlay", marked),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":{}}}}}}}"#,
            quote(&api)
        ),
        complete(&main, "closed", marked),
    ]);
    names(&answer(&messages, "disk"), &["external"]);
    let overlay = answer(&messages, "overlay");
    names(&overlay, &["updated"]);
    assert!(!overlay.contains("\"label\":\"external\""), "{overlay}");
    names(&answer(&messages, "closed"), &["external"]);
}
#[test]
fn selected_reexports_and_canonical_names_preserve_source_lexical_identity() {
    let project = Project::new();
    project.file(
        "api.fn",
        "pub fn real(external value:Int)->Int:value\nfn hidden(secret value:Int)->Int:value\n",
    );
    project.file("bridge.fn", "pub import api.{real}\n");
    for (marked, wanted) in [
        (
            "import api as lib\nfn main():\n    let api=1\n    lib.real(§)\n",
            true,
        ),
        ("import bridge.{real}\nfn main():real(§)\n", true),
        (
            "import api as lib\nfn main():\n    let lib=1\n    lib.real(§)\n",
            false,
        ),
        ("import api as lib\nfn main():lib.hidden(§)\n", false),
    ] {
        let main = project.file("main.fn", &marked.replace('§', ""));
        let messages = file_session(vec![
            open(&main, &marked.replace('§', "")),
            complete(&main, "q", marked),
        ]);
        let response = answer(&messages, "q");
        assert_eq!(
            response.contains("\"newText\":\"external: \""),
            wanted,
            "{response}"
        );
        assert!(!response.contains("\"newText\":\"secret: \""), "{response}");
    }
}
#[test]
fn unclosed_imported_calls_and_new_unsaved_dependencies_use_current_sources() {
    let project = Project::new();
    let marked = "import fresh.{real}\nfn main():real(ex§";
    let main = project.file("main.fn", "fn main():()\n");
    let fresh = format!(
        "file://{}",
        project.0.canonicalize().unwrap().join("fresh.fn").display()
    );
    let messages = file_session(vec![
        open(&fresh, "pub fn real(external value:Int)->Int:value\n"),
        open(&main, &marked.replace('§', "")),
        complete(&main, "q", marked),
    ]);
    names(&answer(&messages, "q"), &["external"]);
    assert!(!project.0.join("fresh.fn").exists());
}
#[test]
fn every_lexical_binding_scope_erases_source_function_labels() {
    let prefix = "fn f(external value:Int)->Int:value\n";
    for body in [
        "let f=(x)->x\n    f(ex§)",
        "let callback=(f)->f(ex§)\n    ()",
        "match Some(1):\n        Some(f)->f(ex§)\n        None->()",
        "for f in [1]: f(ex§)",
        "with\n        f <- Ok(1),\n    do\n        f(ex§)\n    else\n        _->()",
    ] {
        let response = query(&format!("{prefix}fn main():\n    {body}\n"), "completion");
        assert!(
            !response.contains("\"newText\":\"external: \""),
            "{response}"
        );
    }
    names(
        &query(
            &format!("{prefix}fn main():\n    let f=f(ex§)\n    ()\n"),
            "completion",
        ),
        &["external"],
    );
}

#[test]
fn malformed_other_arguments_do_not_offer_incompatible_source_edits() {
    let prefix = "fn f(left:Int,right:Int)->Int:left+right\n";
    for call in [
        "f(§,1)",
        "f(left:1,left:2,§)",
        "f(unknown:1,§)",
        "f(left:1,2,§)",
    ] {
        let response = query(&format!("{prefix}fn main():{call}\n"), "completion");
        assert!(!response.contains("\"newText\":\"right: \""), "{response}");
        assert!(!response.contains("\"newText\":\"left: \""), "{response}");
    }
}
#[test]
fn suggestions_do_not_claim_types_requiredness_or_successful_checking() {
    let source = "fn f(external value:Int)->Bool:missing\nfn main():f(ex§)\n";
    let response = query(source, "completion");
    names(&response, &["external"]);
    assert!(response.contains("Source parameter name"), "{response}");
    assert!(
        !response.contains("required") && !response.contains("-> Bool"),
        "{response}"
    );
    let parsed = fern_compiler::parse::parse(&source.replace('§', "")).unwrap();
    assert!(fern_compiler::check::check(&parsed).is_err());
}
#[test]
fn oversized_label_output_is_explicitly_truncated() {
    let params = (0..250)
        .map(|i| format!("p{i}{}:Int", "x".repeat(2200)))
        .collect::<Vec<_>>()
        .join(",");
    let response = query(
        &format!("fn f({params})->Int:0\nfn main():f(§)\n"),
        "completion",
    );
    assert!(
        response.contains("\"isIncomplete\":true"),
        "{}",
        response.len()
    );
    assert!(response.len() < 1024 * 1024, "{}", response.len());
}
#[test]
fn unsafe_text_and_unrelated_malformed_syntax_never_become_label_sites() {
    let prefix = "fn f(external value:Int)->Int:value\n";
    for tail in [
        "fn main():f(\"ex§\")\n",
        "fn main():f( /* ex§ */ )\n",
        "fn main():f(\"\"\"ex§\"\"\")\n",
        "fn main():f(ex§)\nfn broken(\n",
        "fn main():f(ex§) # comment\nfn bad(): @\n",
    ] {
        let response = query(&format!("{prefix}{tail}"), "completion");
        assert!(
            !response.contains("\"newText\":\"external: \""),
            "{response}"
        );
    }
}

#[test]
fn source_label_suggestions_preserve_unrelated_lexical_value_completion() {
    let response = query(
        "fn f(external value:Int)->Int:value\nfn main():\n    let nearby=1\n    f(near§by)\n",
        "completion",
    );
    assert!(response.contains("\"label\":\"nearby\""), "{response}");
}
#[test]
fn reserved_or_colliding_value_declarations_have_no_source_interface() {
    for declaration in [
        "fn println(secret:Int)->Int:secret",
        "fn Some(secret:Int)->Int:secret",
        "type Box:\n    Box(Int)\nfn Box(secret:Int)->Int:secret",
    ] {
        let name = if declaration.starts_with("type") {
            "Box"
        } else if declaration.contains("println") {
            "println"
        } else {
            "Some"
        };
        let response = query(
            &format!("{declaration}\nfn main():{name}(se§)\n"),
            "completion",
        );
        assert!(!response.contains("\"newText\":\"secret: \""), "{response}");
    }
}

#[test]
fn malformed_current_dependency_clears_previously_available_labels() {
    let project = Project::new();
    let api = project.file("api.fn", "pub fn real(external value:Int)->Int:value\n");
    let marked = "import api as lib\nfn main():lib.real(§)\n";
    let main = project.file("main.fn", &marked.replace('§', ""));
    let changed = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":{},"version":2}},"contentChanges":[{{"text":"pub fn broken("}}]}}}}"#,
        quote(&api)
    );
    let messages = file_session(vec![
        open(&main, &marked.replace('§', "")),
        open(&api, "pub fn real(external value:Int)->Int:value\n"),
        complete(&main, "before", marked),
        changed,
        complete(&main, "after", marked),
    ]);
    names(&answer(&messages, "before"), &["external"]);
    assert!(!answer(&messages, "after").contains("Source parameter name"));
}
#[test]
fn crlf_and_mid_identifier_edits_replace_only_the_whole_label() {
    let response = query(
        "fn f(external value:Int)->Int:value\r\nfn main():f(ex§ternal:1)\r\n",
        "completion",
    );
    names(&response, &["external"]);
    assert!(response.contains("\"newText\":\"external\""), "{response}");
    assert!(
        response.contains(
            "\"end\":{\"character\":20,\"line\":1},\"start\":{\"character\":12,\"line\":1}"
        ),
        "{response}"
    );
}
