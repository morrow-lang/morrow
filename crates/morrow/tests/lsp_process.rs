//! Real compiler process: complete framed protocol and versioned unsaved edits.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn offset(source: &str, position: &Value) -> usize {
    let line = position["line"].as_u64().unwrap() as usize;
    let character = position["character"].as_u64().unwrap() as usize;
    let prefix: usize = source.split_inclusive('\n').take(line).map(str::len).sum();
    let mut units = 0;
    for (index, ch) in source[prefix..].char_indices() {
        if units == character {
            return prefix + index;
        }
        units += ch.len_utf16();
        assert!(units <= character, "edit split a surrogate pair");
    }
    assert_eq!(units, character);
    source.len()
}

fn apply(source: &str, workspace: &Value, uri: &str, version: u64) -> String {
    let changes = workspace["documentChanges"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0]["textDocument"],
        json!({"uri":uri,"version":version})
    );
    let mut edits = Vec::new();
    for edit in changes[0]["edits"].as_array().unwrap() {
        edits.push((
            offset(source, &edit["range"]["start"]),
            offset(source, &edit["range"]["end"]),
            edit["newText"].as_str().unwrap(),
        ));
    }
    assert!(edits.windows(2).all(|pair| pair[0].1 <= pair[1].0));
    let mut output = source.to_owned();
    for (start, end, text) in edits.into_iter().rev() {
        output.replace_range(start..end, text);
    }
    output
}

#[test]
fn real_lsp_preserves_complete_lifecycle_navigation_and_versioned_edits() {
    let requests: Vec<Value> = serde_json::from_str(include_str!("lsp-session.json")).unwrap();
    let mut input = Vec::new();
    for request in &requests {
        let body = serde_json::to_vec(request).unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.extend(body);
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_morrow"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout
            .by_ref()
            .take(1_048_577)
            .read_to_end(&mut output)
            .unwrap();
        output
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("LSP exceeded bounded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .take(65537)
        .read_to_end(&mut stderr)
        .unwrap();
    assert!(stderr.is_empty(), "{stderr:?}");
    let output = reader.join().unwrap();
    assert!(output.len() <= 1_048_576);
    let mut remaining = output.as_slice();
    let mut messages = Vec::new();
    while !remaining.is_empty() {
        let end = remaining
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let header = std::str::from_utf8(&remaining[..end]).unwrap();
        let size: usize = header
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        assert!(remaining.len() >= end + 4 + size);
        let value: Value = serde_json::from_slice(&remaining[end + 4..end + 4 + size]).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        messages.push(value);
        remaining = &remaining[end + 4 + size..];
    }
    let response = |id| {
        let selected: Vec<_> = messages
            .iter()
            .filter(|message| message["id"] == id)
            .collect();
        assert_eq!(selected.len(), 1);
        selected[0]
    };
    let capabilities = &response(json!(1))["result"]["capabilities"];
    assert!(capabilities.get("completionProvider").is_some());
    assert_eq!(capabilities["renameProvider"]["prepareProvider"], true);
    assert!(
        capabilities["codeActionProvider"]["codeActionKinds"]
            .as_array()
            .unwrap()
            .contains(&json!("source.fixAll.morrow"))
    );
    assert!(
        response(json!(2))["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "add")
    );
    assert_eq!(response(json!(3))["error"]["code"], -32803);
    assert_eq!(
        response(json!(4))["result"],
        json!({"placeholder":"add","range":{"start":{"line":4,"character":4},"end":{"line":4,"character":7}}})
    );
    let valid = requests[5]["params"]["contentChanges"][0]["text"]
        .as_str()
        .unwrap();
    let uri = requests[5]["params"]["textDocument"]["uri"]
        .as_str()
        .unwrap();
    assert_eq!(
        response(json!(5))["result"]["documentChanges"][0]["edits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        apply(valid, &response(json!(5))["result"], uri, 2),
        valid.replace("add", "sum")
    );
    let dirty = requests[8]["params"]["textDocument"]["text"]
        .as_str()
        .unwrap();
    let uri = requests[8]["params"]["textDocument"]["uri"]
        .as_str()
        .unwrap();
    let actions = response(json!(6))["result"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["kind"], "source.fixAll.morrow");
    assert!(actions[0].get("command").is_none());
    assert_eq!(
        apply(dirty, &actions[0]["edit"], uri, 1),
        dirty.replace("  \n", "\n")
    );
    assert_eq!(response(json!(7))["result"], json!([]));
    assert!(messages.iter().any(|message| {
        message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == uri
            && message["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["message"].as_str().unwrap().contains("Result"))
    }));
    assert!(response(json!(8))["result"].is_null());
}
