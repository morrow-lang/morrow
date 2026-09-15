//! Semantic rename and actionable editor edits must never mutate accepted buffers.
use super::*;

const URI: &str = "untitled:rename.mr";
fn server(source: &str) -> Server {
    let mut server = Server {
        state: State::Running,
        client_edits: capabilities::ClientEdits {
            document_changes: true,
            prepare_rename: true,
            literal_actions: true,
        },
        documents: BTreeMap::new(),
        published: BTreeMap::new(),
    };
    server.documents.insert(
        URI.into(),
        Document {
            source: source.into(),
            version: 7,
        },
    );
    server
}
fn request(source: &str, marker: &str, new_name: &str) -> Json {
    let at = source.find(marker).unwrap();
    object([
        ("textDocument", object([("uri", string(URI))])),
        ("position", position(source, at)),
        ("newName", string(new_name)),
    ])
}
fn rewritten(server: &Server, edit: &Json) -> String {
    let changes = field(edit, "documentChanges").unwrap().array().unwrap();
    assert_eq!(changes.len(), 1);
    let document = field(&changes[0], "textDocument").unwrap();
    assert_eq!(field(document, "version").unwrap().integer().unwrap(), 7);
    assert_eq!(field(document, "uri").unwrap().string().unwrap(), URI);
    let mut source = server.documents[URI].source.clone();
    for edit in field(&changes[0], "edits")
        .unwrap()
        .array()
        .unwrap()
        .iter()
        .rev()
    {
        let change = object([
            ("range", field(edit, "range").unwrap().clone()),
            ("text", field(edit, "newText").unwrap().clone()),
        ]);
        apply_change(&mut source, &change).unwrap();
    }
    source
}

#[test]
fn rename_selects_binding_identity_and_excludes_comments_strings_and_shadowing() {
    let source = "fn main():\n    let value = 1\n    println(value)\n    let value = 2\n    println(value)\n    println(\"value\") # value\n";
    let server = server(source);
    let edit = server
        .rename(&request(source, "value = 1", "count"))
        .unwrap();
    assert_eq!(
        rewritten(&server, &edit),
        source.replacen("value", "count", 2)
    );
    assert_eq!(server.documents[URI].source, source);
}

#[test]
fn rename_unicode_parameters_patterns_and_closure_captures_use_utf16() {
    for source in [
        "fn value(🌿: Int) -> Int: 🌿 + 1\nfn main(): println(value(1))\n",
        "fn main():\n    let 🌿 = 1\n    let closure = () -> 🌿\n    println(closure())\n",
        "fn main():\n    let (🌿, other) = (1, 2)\n    println(🌿 + other)\n",
        "fn main():\n    for 🌿 in [1, 2]: println(🌿)\n",
    ] {
        let server = server(source);
        let edit = server.rename(&request(source, "🌿", "leaf")).unwrap();
        assert_eq!(rewritten(&server, &edit), source.replace("🌿", "leaf"));
    }
}

#[test]
fn rename_updates_private_function_clauses_and_recursive_calls() {
    let source =
        "fn count(0: Int) -> 0\nfn count(n: Int) -> count(n - 1)\nfn main(): println(count(2))\n";
    let server = server(source);
    let edit = server.rename(&request(source, "count", "length")).unwrap();
    assert_eq!(rewritten(&server, &edit), source.replace("count", "length"));
}

#[test]
fn rename_rejects_same_type_capture_in_both_directions() {
    for source in [
        "fn main():\n    let first = 1\n    let second = 2\n    println(first)\n    println(second)\n",
        "fn main():\n    let first = 1\n    let closure = (second: Int) -> first + second\n    println(closure(2))\n",
    ] {
        let server = server(source);
        let error = server
            .rename(&request(source, "first", "second"))
            .unwrap_err();
        assert_eq!(error.0, -32803);
        assert!(
            error.1.contains("capture") || error.1.contains("invalid"),
            "{error:?}"
        );
        assert_eq!(server.documents[URI].source, source);
    }
}

#[test]
fn rename_rejects_invalid_names_missing_symbols_public_exports_and_members() {
    let source = "fn main():\n    let value = 1\n    println(value)\n";
    for name in [
        "",
        "_",
        "if",
        "a.b",
        "two words",
        "a\nfn injected():()",
        "List",
    ] {
        assert!(
            server(source)
                .rename(&request(source, "value", name))
                .is_err(),
            "{name}"
        );
    }
    for (source, marker) in [
        ("fn main(): println(1)\n", "println"),
        (
            "pub fn value() -> Int: 1\nfn main(): println(value())\n",
            "value",
        ),
        ("fn main(): println(\"value\")\n", "value"),
        (
            "type Box:\n    value: Int\nfn main(): println(Box(1).value)\n",
            "value",
        ),
    ] {
        assert!(
            server(source)
                .rename(&request(source, marker, "other"))
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn prepare_rename_returns_exact_range_and_same_name_is_empty() {
    let source = "fn main():\n    let 🌿 = 1\n    println(🌿)\n";
    let server = server(source);
    let params = request(source, "🌿", "🌿");
    let prepared = server.prepare_rename(&params).unwrap();
    assert_eq!(
        field(&prepared, "placeholder").unwrap().string().unwrap(),
        "🌿"
    );
    let range = field(&prepared, "range").unwrap();
    assert_eq!(
        field(field(range, "start").unwrap(), "character")
            .unwrap()
            .integer()
            .unwrap(),
        8
    );
    assert_eq!(
        field(field(range, "end").unwrap(), "character")
            .unwrap()
            .integer()
            .unwrap(),
        10
    );
    let result = server.rename(&params).unwrap();
    assert!(
        field(&result, "documentChanges")
            .unwrap()
            .array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn code_actions_return_versioned_canonical_edits_and_respect_kind_filter() {
    let source = "fn main():println(1)\n";
    let server = server(source);
    let mut params = request(source, "main", "unused");
    if let Json::Object(fields) = &mut params {
        fields.insert(
            "range".into(),
            navigation::source_range(
                source,
                Span {
                    start: 0,
                    end: source.len(),
                },
            ),
        );
        fields.insert(
            "context".into(),
            object([("diagnostics", Json::Array(vec![]))]),
        );
    }
    let actions = server.code_actions(&params).unwrap();
    let actions = actions.array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        field(&actions[0], "kind").unwrap().string().unwrap(),
        "source.fixAll.morrow"
    );
    let edit = field(&actions[0], "edit").unwrap();
    assert_eq!(
        rewritten(&server, edit),
        crate::format::format(source).unwrap()
    );
    assert!(actions[0].get("command").is_none());
    if let Json::Object(fields) = &mut params {
        fields.insert(
            "context".into(),
            object([
                ("diagnostics", Json::Array(vec![])),
                ("only", Json::Array(vec![string("quickfix")])),
            ]),
        );
    }
    assert!(
        server
            .code_actions(&params)
            .unwrap()
            .array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rename_private_alias_preserves_type_namespace_and_rejects_collisions() {
    let source = "type Count = Int\nfn double(value: Count) -> Count: value + value\nfn main(): println(double(2))\n";
    let server = server(source);
    let edit = server.rename(&request(source, "Count", "Amount")).unwrap();
    assert_eq!(rewritten(&server, &edit), source.replace("Count", "Amount"));
    assert!(server.rename(&request(source, "Count", "Int")).is_err());
}

#[test]
fn rename_protocol_routes_advertised_capabilities_and_errors() {
    let mut server = server("fn main():\n    let value = 1\n    println(value)\n");
    server.state = State::New;
    let mut output = Vec::new();
    server
        .request(
            "initialize",
            number(1),
            &supported_initialize(),
            &mut output,
        )
        .unwrap();
    let frame = read_frame(&mut std::io::Cursor::new(&output))
        .unwrap()
        .unwrap();
    let response = JsonParser::parse(&frame).unwrap();
    let capabilities = field(field(&response, "result").unwrap(), "capabilities").unwrap();
    assert!(capabilities.get("renameProvider").is_some());
    assert!(capabilities.get("codeActionProvider").is_some());
    for method in [
        "textDocument/rename",
        "textDocument/prepareRename",
        "textDocument/codeAction",
    ] {
        output.clear();
        server
            .request(method, number(2), &Json::Null, &mut output)
            .unwrap();
        let frame = read_frame(&mut std::io::Cursor::new(&output))
            .unwrap()
            .unwrap();
        let response = JsonParser::parse(&frame).unwrap();
        assert_eq!(
            field(field(&response, "error").unwrap(), "code")
                .unwrap()
                .integer()
                .unwrap(),
            -32602
        );
    }
}

#[test]
fn rename_bounds_references_and_does_not_offer_edits_for_invalid_source() {
    let mut source = "fn main():\n    let value = 1\n".to_string();
    source.push_str(&"    println(value)\n".repeat(513));
    let error = server(&source)
        .rename(&request(&source, "value", "count"))
        .unwrap_err();
    assert!(error.1.contains("budget"));
    let source = "fn main():\n    let value = 1\n    println(unknown)\n";
    assert!(
        server(source)
            .prepare_rename(&request(source, "value", "count"))
            .is_err()
    );
}

#[test]
fn rename_uses_unsaved_module_graph_and_rejects_imported_declarations() {
    let temp = std::env::temp_dir().join(format!("morrow-rename-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let dependency = temp.join("math.mr");
    let entry = temp.join("main.mr");
    std::fs::write(&dependency, "this disk source is stale and invalid").unwrap();
    std::fs::write(&entry, "also stale").unwrap();
    let source = "import math\nfn main():\n    let value = math.double(2)\n    println(value)\n";
    let uri = path_uri(&entry);
    let mut server = server(source);
    server.documents.clear();
    server.documents.insert(
        uri.clone(),
        Document {
            source: source.into(),
            version: 19,
        },
    );
    server.documents.insert(
        path_uri(&dependency),
        Document {
            source: "pub fn double(value: Int) -> Int: value + value\n".into(),
            version: 4,
        },
    );
    let params = |marker, name| {
        let mut params = request(source, marker, name);
        if let Json::Object(fields) = &mut params {
            fields.insert("textDocument".into(), object([("uri", string(&uri))]));
        }
        params
    };
    let edit = server.rename(&params("value", "count")).unwrap();
    let changes = field(&edit, "documentChanges").unwrap().array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        field(field(&changes[0], "textDocument").unwrap(), "version")
            .unwrap()
            .integer()
            .unwrap(),
        19
    );
    assert_eq!(
        field(&changes[0], "edits").unwrap().array().unwrap().len(),
        2
    );
    assert!(server.rename(&params("double", "twice")).is_err());
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), "also stale");
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rename_protocol_returns_the_same_checked_versioned_edit() {
    let source = "fn main():\n    let value = 1\n    println(value)\n";
    let mut server = server(source);
    let mut output = Vec::new();
    server
        .request(
            "textDocument/rename",
            number(42),
            &request(source, "value", "count"),
            &mut output,
        )
        .unwrap();
    let frame = read_frame(&mut std::io::Cursor::new(output))
        .unwrap()
        .unwrap();
    let response = JsonParser::parse(&frame).unwrap();
    assert_eq!(
        rewritten(&server, field(&response, "result").unwrap()),
        source.replace("value", "count")
    );
}

#[test]
fn code_actions_are_empty_for_clean_or_unparseable_source_and_validate_ranges() {
    for source in ["fn main():\n    println(1)\n", "fn main(:\n"] {
        let server = server(source);
        let params = object([
            ("textDocument", object([("uri", string(URI))])),
            (
                "range",
                navigation::source_range(
                    source,
                    Span {
                        start: 0,
                        end: source.len(),
                    },
                ),
            ),
            ("context", object([("diagnostics", Json::Array(vec![]))])),
        ]);
        assert!(
            server
                .code_actions(&params)
                .unwrap()
                .array()
                .unwrap()
                .is_empty()
        );
    }
    let source = "fn main():println(1)\n";
    let server = server(source);
    let mut params = object([
        ("textDocument", object([("uri", string(URI))])),
        (
            "range",
            object([
                ("start", position(source, source.len())),
                ("end", position(source, 0)),
            ]),
        ),
        ("context", object([("diagnostics", Json::Array(vec![]))])),
    ]);
    assert_eq!(server.code_actions(&params).unwrap_err().0, -32602);
    if let Json::Object(fields) = &mut params {
        fields.insert(
            "range".into(),
            navigation::source_range(
                source,
                Span {
                    start: 0,
                    end: source.len(),
                },
            ),
        );
        fields.insert("context".into(), object([("diagnostics", Json::Null)]));
    }
    assert_eq!(server.code_actions(&params).unwrap_err().0, -32602);
}

#[test]
fn rename_rejects_exported_parameter_labels_and_partial_labeled_call_edits() {
    for source in [
        "pub fn expose(value: Int) -> Int: value\nfn main(): println(expose(1))\n",
        "fn expose(value: Int) -> Int: value\nfn main(): println(expose(value: 1))\n",
    ] {
        assert!(
            server(source)
                .rename(&request(source, "value", "count"))
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn rename_includes_interpolation_expressions_but_excludes_member_selectors() {
    let source = "type Box:\n    value: Int\nfn main():\n    let value = 1\n    let box = Box(2)\n    println(\"value {value}\")\n    println(box.value + value)\n";
    let server = server(source);
    let edit = server
        .rename(&request(source, "value = 1", "count"))
        .unwrap();
    let expected = source
        .replace("let value =", "let count =")
        .replace("{value}", "{count}")
        .replace("+ value)", "+ count)");
    assert_eq!(rewritten(&server, &edit), expected);
}

/// Inspect negotiated capabilities using the same initialization path as the wire server.
fn initialize_with(server: &mut Server, capabilities: Json) -> Json {
    server.state = State::New;
    let mut output = Vec::new();
    server
        .request(
            "initialize",
            number(1),
            &object([("capabilities", capabilities)]),
            &mut output,
        )
        .unwrap();
    let frame = read_frame(&mut std::io::Cursor::new(output))
        .unwrap()
        .unwrap();
    JsonParser::parse(&frame)
        .unwrap()
        .get("result")
        .unwrap()
        .get("capabilities")
        .unwrap()
        .clone()
}

#[test]
fn minimal_clients_keep_formatting_but_never_receive_unnegotiated_edits() {
    let source = "fn main():println(1)\n";
    let disabled = object([("documentChanges", Json::Bool(false))]);
    let disabled = object([("workspaceEdit", disabled)]);
    for capabilities in [object([]), object([("workspace", disabled)])] {
        let mut server = server(source);
        let caps = initialize_with(&mut server, capabilities);
        assert!(matches!(
            caps.get("renameProvider"),
            Some(Json::Bool(false)) | None
        ));
        assert!(matches!(
            caps.get("codeActionProvider"),
            Some(Json::Bool(false)) | None
        ));
        assert!(matches!(
            caps.get("documentFormattingProvider"),
            Some(Json::Bool(true))
        ));
        for method in [
            "textDocument/rename",
            "textDocument/prepareRename",
            "textDocument/codeAction",
        ] {
            let mut output = Vec::new();
            server
                .request(
                    method,
                    number(2),
                    &request(source, "main", "other"),
                    &mut output,
                )
                .unwrap();
            let frame = read_frame(&mut std::io::Cursor::new(output))
                .unwrap()
                .unwrap();
            let response = JsonParser::parse(&frame).unwrap();
            assert_eq!(
                field(field(&response, "error").unwrap(), "code")
                    .unwrap()
                    .integer()
                    .unwrap(),
                -32803
            );
            assert!(
                field(field(&response, "error").unwrap(), "message")
                    .unwrap()
                    .string()
                    .unwrap()
                    .contains("capabilit")
            );
        }
        let params = object([
            ("textDocument", object([("uri", string(URI))])),
            (
                "options",
                object([("tabSize", number(4)), ("insertSpaces", Json::Bool(true))]),
            ),
        ]);
        assert_eq!(
            server.formatting(&params).unwrap().array().unwrap().len(),
            1
        );
    }
}

#[test]
fn rename_prepare_and_literal_actions_negotiate_independently() {
    let source = "fn main():\n    let value = 1\n    println(value)\n";
    let mut server = server(source);
    let workspace = object([(
        "workspaceEdit",
        object([("documentChanges", Json::Bool(true))]),
    )]);
    let caps = initialize_with(&mut server, object([("workspace", workspace.clone())]));
    assert!(matches!(caps.get("renameProvider"), Some(Json::Bool(true))));
    assert!(matches!(
        caps.get("codeActionProvider"),
        Some(Json::Bool(false)) | None
    ));
    assert!(server.rename(&request(source, "value", "count")).is_ok());
    assert!(
        server
            .prepare_rename(&request(source, "value", "count"))
            .is_err()
    );
    let text_document = object([
        ("rename", object([("prepareSupport", Json::Bool(true))])),
        (
            "codeAction",
            object([(
                "codeActionLiteralSupport",
                object([(
                    "codeActionKind",
                    object([("valueSet", Json::Array(vec![string("source.fixAll")]))]),
                )]),
            )]),
        ),
    ]);
    let caps = initialize_with(
        &mut server,
        object([("textDocument", text_document.clone())]),
    );
    assert!(matches!(
        caps.get("renameProvider"),
        Some(Json::Bool(false)) | None
    ));
    assert!(matches!(
        caps.get("codeActionProvider"),
        Some(Json::Bool(false)) | None
    ));
    let caps = initialize_with(
        &mut server,
        object([("workspace", workspace), ("textDocument", text_document)]),
    );
    assert!(matches!(
        caps.get("renameProvider").unwrap().get("prepareProvider"),
        Some(Json::Bool(true))
    ));
    assert!(
        caps.get("codeActionProvider")
            .unwrap()
            .get("codeActionKinds")
            .is_some()
    );
    assert!(
        server
            .prepare_rename(&request(source, "value", "count"))
            .is_ok()
    );
}

/// Model a modern editor's explicit support for the three negotiated edit features.
fn supported_initialize() -> Json {
    JsonParser::parse(r#"{"capabilities":{"workspace":{"workspaceEdit":{"documentChanges":true}},"textDocument":{"rename":{"prepareSupport":true},"codeAction":{"codeActionLiteralSupport":{"codeActionKind":{"valueSet":["source.fixAll"]}}}}}}"#).unwrap()
}

#[test]
fn malformed_capabilities_fail_closed_without_enabling_edit_payloads() {
    for capabilities in [
        r#"{"workspace":{"workspaceEdit":{"documentChanges":"true"}}}"#,
        r#"{"workspace":{"workspaceEdit":{"documentChanges":true}},"textDocument":{"rename":{"prepareSupport":"true"},"codeAction":{"codeActionLiteralSupport":true}}}"#,
        r#"{"workspace":{"workspaceEdit":{"documentChanges":true}},"textDocument":{"codeAction":{"codeActionLiteralSupport":{"codeActionKind":{"valueSet":[null]}}}}}"#,
    ] {
        let mut server = server("fn main(): println(1)\n");
        let caps = initialize_with(&mut server, JsonParser::parse(capabilities).unwrap());
        assert!(matches!(
            caps.get("codeActionProvider"),
            Some(Json::Bool(false))
        ));
        assert!(
            caps.get("renameProvider")
                .unwrap()
                .get("prepareProvider")
                .is_none()
        );
        assert!(matches!(caps.get("hoverProvider"), Some(Json::Bool(true))));
    }
}
