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
fn hover_reports_final_types_docs_and_generic_requirements() {
    let response = query(
        "@doc \"\"\"Identity docs.\"\"\"\nfn ident§ity(x): x\n",
        "hover",
    );
    assert!(response.contains("Identity docs."), "{response}");
    assert!(response.contains("fn identity(x: a) -> a"), "{response}");
    assert!(!response.contains("$inferred"), "{response}");
    let response = query("fn twi§ce(x): x+x\n", "hover");
    assert!(response.contains("addition"), "{response}");
}

#[test]
fn constants_have_value_signatures_in_hover_and_completion() {
    let declaration = "@doc \"\"\"Computed configuration.\"\"\"\nconst ans§wer = comptime:\n    40 + 2\nfn main(): println(answer)\n";
    let response = query(declaration, "hover");
    assert!(response.contains("const answer: Int"), "{response}");
    assert!(!response.contains("fn answer"), "{response}");
    let response = query(
        "const answer = comptime:\n    42\nfn main(): println(ans§wer)\n",
        "completion",
    );
    let value: serde_json::Value = serde_json::from_str(&response).unwrap();
    let items = value["result"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{response}"));
    let item = items
        .iter()
        .find(|item| item["label"] == "answer")
        .unwrap_or_else(|| panic!("{response}"));
    assert_eq!(item["kind"], 21, "constant completion kind");
}

#[test]
fn local_hover_respects_shadowing_and_interpolation() {
    let response = query(
        "fn main():\n    let value=1\n    let value=\"text\"\n    println(\"{val§ue}\")\n",
        "hover",
    );
    assert!(response.contains("value: String"), "{response}");
    assert!(query("fn main(): println(\"value§\")\n", "hover").contains("\"result\":null"));
    assert!(query("fn main(): # value§\n    ()\n", "hover").contains("\"result\":null"));
}

#[test]
fn typed_members_on_valid_source_have_details_and_field_definitions() {
    let source =
        "type Box(a):\n    value: a\nfn main():\n    let box=Box(4)\n    println(box.val§ue)\n";
    let response = query(source, "completion");
    assert!(response.contains("\"label\":\"value\""), "{response}");
    assert!(response.contains("\"detail\":\"Int\""), "{response}");
    let response = query(source, "hover");
    assert!(response.contains("value: Int"), "{response}");
    let response = query(source, "definition");
    assert!(response.contains("\"line\":1"), "{response}");
}

#[test]
fn invalid_current_types_do_not_reuse_hover_facts() {
    let response = query("fn good§(x: Int) -> Int: x\nfn main(): missing\n", "hover");
    assert!(response.contains("\"result\":null"), "{response}");
    let response = query("fn main():\n    let box=1\n    box.val§ue\n", "completion");
    assert!(!response.contains("\"label\":\"value\""), "{response}");
}

#[test]
fn hover_scopes_cover_clauses_closures_match_for_with_and_let_else() {
    for (source, expected) in [
        (
            "fn first([head,..tail]: List(Int)) -> Int: head§\nfn first([]: List(Int)) -> Int: 0\n",
            "head: Int",
        ),
        (
            "fn main():\n    let outer=\"text\"\n    let f=()->out§er\n    println(f())\n",
            "outer: String",
        ),
        (
            "fn main():\n    let value: Option(Int)=Some(1)\n    println(match value:\n        Some(item) -> it§em\n        None -> 0)\n",
            "item: Int",
        ),
        (
            "fn main():\n    for (index, value) in [\"a\"].enumerate():\n        println(val§ue)\n",
            "value: String",
        ),
        (
            "fn load() -> Result(Int, String): Ok(1)\nfn main():\n    println(with\n        value <- load()\n    do\n        val§ue\n    else\n        Err(_) -> 0)\n",
            "value: Int",
        ),
        (
            "fn first(value: Option(String)) -> String:\n    let Some(item)=value else: return \"none\"\n    it§em\n",
            "item: String",
        ),
    ] {
        let response = query(source, "hover");
        assert!(response.contains(expected), "{source}\n{response}");
    }
}

#[test]
fn nested_and_callable_fields_preserve_receiver_types() {
    let source = "type Inner:\n    count: Int\ntype Outer:\n    inner: Inner\n    callback: (Int) -> String\nfn main():\n    let model=Outer(Inner(1), (x: Int)->\"ok\")\n    println(model.in§ner.count)\n";
    let response = query(source, "hover");
    assert!(response.contains("inner: Inner"), "{response}");
    let source = source.replace("model.in§ner.count", "model.call§back(1)");
    let response = query(&source, "completion");
    assert!(response.contains("(Int) -> String"), "{response}");
    let response = query(
        "fn main():\n    let values=[1]\n    let pairs=values.enum§erate()\n    println(List.len(pairs))\n",
        "completion",
    );
    assert!(response.contains("\"label\":\"enumerate\""), "{response}");
}

#[test]
fn hover_ranges_and_doc_text_remain_literal_and_utf16_exact() {
    let response = query(
        "@doc \"\"\"<b>raw</b> [run](command:x) ```\"\"\"\r\nfn 🌿§(x: Int) -> Int: x\r\n",
        "hover",
    );
    assert!(response.contains("\"kind\":\"plaintext\""), "{response}");
    assert!(
        response.contains("<b>raw</b> [run](command:x) ```"),
        "{response}"
    );
    assert!(
        response.contains("\"character\":5,\"line\":1"),
        "{response}"
    );
    let huge = format!("@doc \"\"\"{}\"\"\"\nfn huge§(): ()\n", "🦀".repeat(20_000));
    let response = query(&huge, "hover");
    assert!(response.len() < 70_000, "{}", response.len());
    assert!(
        response.contains("[excerpt]"),
        "documentation truncation must be explicit"
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
fn dependency_overlay_signatures_docs_and_aliases_refresh_without_stale_facts() {
    let project = Project::new();
    let model = project.file(
        "model.fn",
        "@doc \"\"\"Disk docs\"\"\"\npub fn value() -> Int: 1\n",
    );
    project.file("api.fn", "pub import model.{value}\n");
    let source = "import api as long_alias\nfn main(): println(long_alias.value())\n";
    let main = project.file("main.fn", source);
    let messages = session(vec![
        opening(&main, source),
        request(&main, "disk", "hover", 1, 35),
        opening(
            &model,
            "@doc \"\"\"New docs\"\"\"\npub fn value() -> String: \"new\"\n",
        ),
        request(&main, "overlay", "hover", 1, 35),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":{}}}}}}}"#,
            quote(&model)
        ),
        request(&main, "closed", "hover", 1, 35),
    ]);
    let disk = answer(&messages, "disk");
    assert!(disk.contains("Disk docs"), "{disk}");
    assert!(disk.contains("-> Int"), "{disk}");
    let changed = answer(&messages, "overlay");
    assert!(changed.contains("New docs"), "{changed}");
    assert!(changed.contains("-> String"), "{changed}");
    assert!(answer(&messages, "closed").contains("Disk docs"));
}

#[test]
fn duplicate_imported_type_leaf_names_use_resolver_identity() {
    let project = Project::new();
    project.file(
        "first.fn",
        "@doc \"\"\"First type\"\"\"\npub type Box:\n    value: Int\n",
    );
    project.file(
        "second.fn",
        "@doc \"\"\"Second type\"\"\"\npub type Box:\n    value: String\n",
    );
    let source = "import first as first\nimport second as second\nfn describe(x: second.Box) -> (): ()\nfn main(): ()\n";
    let main = project.file("main.fn", source);
    let messages = session(vec![
        opening(&main, source),
        request(&main, "type", "hover", 2, 23),
    ]);
    let result = answer(&messages, "type");
    assert!(result.contains("Second type"), "{result}");
    assert!(!result.contains("First type"), "{result}");
}

#[test]
fn inferred_local_variables_keep_their_enclosing_signature_names() {
    let result = query("fn second(first, second): sec§ond\n", "hover");
    assert!(result.contains("second: b"), "{result}");
}

#[test]
fn call_receivers_and_tuple_slots_offer_checked_members() {
    let source =
        "type Box:\n    value: Int\nfn make() -> Box: Box(1)\nfn main(): println(make().val§ue)\n";
    let response = query(source, "completion");
    assert!(response.contains("\"label\":\"value\""), "{response}");
    let source = "fn main():\n    let pair=(1, \"text\")\n    println(pair.1§)\n";
    assert!(query(source, "hover").contains("1: String"));
    assert!(query(source, "completion").contains("\"label\":\"1\""));
    assert!(query("fn main(): println(1.5§)\n", "hover").contains("1.5: Float"));
}

#[test]
fn nested_lambda_parameter_origin_is_not_overwritten_by_its_outer_let() {
    let result = query(
        "fn main():\n    let callback=(value: Int)->val§ue+1\n    println(callback(1))\n",
        "hover",
    );
    assert!(result.contains("value: Int"), "{result}");
    assert!(!result.contains("value: (Int)"), "{result}");
}

#[test]
fn aliased_generic_call_hover_retains_the_concrete_use_signature() {
    let project = Project::new();
    project.file("model.fn", "pub fn identity(x: a) -> a: x\n");
    let source = "import model as longer_alias\nfn main(): println(longer_alias.identity(1))\n";
    let main = project.file("main.fn", source);
    let result = answer(
        &session(vec![
            opening(&main, source),
            request(&main, "call", "hover", 1, 36),
        ]),
        "call",
    );
    assert!(result.contains("At this use: (Int) -> Int"), "{result}");
}

#[test]
fn hover_wire_budget_counts_json_escape_expansion() {
    let source = format!(
        "@doc \"\"\"{}\"\"\"\nfn escaped§(): ()\n",
        "\u{1}".repeat(17_000)
    );
    let response = query(&source, "hover");
    assert!(response.contains("escaped"), "{response}");
    assert!(
        response.len() < 65_536,
        "encoded hover was {} bytes",
        response.len()
    );
    assert!(response.contains("[excerpt]"));
}

#[test]
fn synthetic_library_entry_does_not_steal_trailing_local_scope() {
    let response = query(
        "fn helper():\n    let local=1\n    ()\n    §\n",
        "completion",
    );
    assert!(response.contains("\"label\":\"local\""), "{response}");
}

#[test]
fn hover_documentation_belongs_to_its_exact_declaration() {
    let source = "@doc \"\"\"Type documentation.\"\"\"\ntype Box:\n    Wrap(Int)\n@doc \"\"\"Function documentation.\"\"\"\nfn Box(value: Int) -> Int: value\nfn main(): println(Box(1))\n";
    for (marked, expected, excluded) in [
        (
            source.replace("type Box", "type Bo§x"),
            "Type documentation.",
            "Function documentation.",
        ),
        (
            source.replace("fn Box", "fn Bo§x"),
            "Function documentation.",
            "Type documentation.",
        ),
        (
            source.replace("println(Box", "println(Bo§x"),
            "Function documentation.",
            "Type documentation.",
        ),
    ] {
        let response = query(&marked, "hover");
        assert!(response.contains(expected), "{response}");
        assert!(!response.contains(excluded), "{response}");
    }
}

#[test]
fn traits_have_documented_declaration_hover_and_bound_navigation() {
    let source =
        "@doc \"\"\"A readable value.\"\"\"\ntrait Lab§el(a):\n    fn label(value: a) -> String\n";
    let response = query(source, "hover");
    assert!(response.contains("trait Label(a)"), "{response}");
    assert!(response.contains("A readable value."), "{response}");
    let response = query(
        "trait Label(a):\n    fn label(value: a) -> String\nfn describe(value: a) -> String where Lab§el(a): label(value)\n",
        "definition",
    );
    assert!(response.contains("\"line\":0"), "{response}");
    assert!(response.contains("\"character\":6"), "{response}");
}

#[test]
fn trait_method_and_implementation_locals_show_public_source_names() {
    let response = query(
        "trait Label(a):\n    fn lab§el(value: a) -> String\n",
        "hover",
    );
    assert!(
        response.contains("fn label(value: a) -> String"),
        "{response}"
    );
    let source = "trait Label(a):\n    fn label(value: a) -> String\ntype Item:\n    number: Int\nimpl Label(Item):\n    fn label(value: Item) -> String: \"{val§ue.number}\"\n";
    let response = query(source, "hover");
    assert!(response.contains("value: Item"), "{response}");
    assert!(!response.contains("$impl"), "{response}");
    let response = query(
        "trait Label(a):\n    fn label(value: a) -> String\nfn desc§ribe(value: a) -> String where Label(a): label(value)\n",
        "hover",
    );
    assert!(response.contains("where Label(a)"), "{response}");
}

#[test]
fn trait_bound_completion_lists_trait_names_without_internal_methods() {
    let response = query(
        "trait Label(a):\n    fn label(value: a) -> String\nimpl Label(Int):\n    fn label(value: Int) -> String: \"{value}\"\nfn describe(value: a) -> String where Lab§el(a): label(value)\n",
        "completion",
    );
    let value: serde_json::Value = serde_json::from_str(&response).unwrap();
    let items = value["result"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{response}"));
    let item = items
        .iter()
        .find(|item| item["label"] == "Label")
        .unwrap_or_else(|| panic!("{response}"));
    assert_eq!(item["kind"], 8);
    assert!(!response.contains("$impl"), "{response}");
}

#[test]
fn inferred_trait_requirements_name_the_actual_trait() {
    let response = query(
        "trait Label(a):\n    fn label(value: a) -> String\nfn desc§ribe(value): label(value)\n",
        "hover",
    );
    assert!(response.contains("Requires a: Label"), "{response}");
}
