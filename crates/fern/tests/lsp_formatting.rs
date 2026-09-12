use fern_compiler::lsp;

fn frame(message: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{message}", message.len()).into_bytes()
}

fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

fn initialize() -> &'static str {
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
}
fn shutdown() -> &'static str {
    r#"{"jsonrpc":"2.0","id":99,"method":"shutdown"}"#
}
fn exit() -> &'static str {
    r#"{"jsonrpc":"2.0","method":"exit"}"#
}

fn run(messages: &[String]) -> (Result<(), String>, Vec<String>) {
    let input: Vec<u8> = messages.iter().flat_map(|message| frame(message)).collect();
    let mut output = Vec::new();
    let result = lsp::serve(std::io::Cursor::new(input), &mut output);
    let mut messages = Vec::new();
    let mut remaining = output.as_slice();
    while !remaining.is_empty() {
        let end = remaining
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let header = std::str::from_utf8(&remaining[..end]).unwrap();
        let length: usize = header
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        messages.push(
            std::str::from_utf8(&remaining[end + 4..end + 4 + length])
                .unwrap()
                .to_owned(),
        );
        remaining = &remaining[end + 4 + length..];
    }
    (result, messages)
}

fn open(uri: &str, source: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"languageId":"fern","version":1,"text":{}}}}}}}"#,
        quote(uri),
        quote(source)
    )
}

const URI: &str = "untitled:format.fn";
fn formatting(id: i32, options: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{URI}"}},"options":{options}}}}}"#
    )
}
fn request(id: i32) -> String {
    formatting(id, r#"{"tabSize":4,"insertSpaces":true}"#)
}
fn response(output: &[String], id: i32) -> &str {
    output
        .iter()
        .find(|s| s.contains(&format!(r#""id":{id},"#)))
        .unwrap()
}
fn change(version: i32, source: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{URI}","version":{version}}},"contentChanges":[{{"text":{}}}]}}}}"#,
        quote(source)
    )
}
#[test]
fn capability_and_canonical_whole_document_edit_use_utf16() {
    for source in [
        "fn main():println(\"🌿\")",
        "#🌿\r\nfn main():println(1)\r\n",
        "fn main():println(1)\n",
    ] {
        let canonical = fern_compiler::format::format(source).unwrap();
        let (status, output) = run(&[
            initialize().into(),
            open(URI, source),
            request(10),
            shutdown().into(),
            exit().into(),
        ]);
        assert!(status.is_ok());
        assert!(
            output[0].contains(r#""documentFormattingProvider":true"#),
            "{}",
            output[0]
        );
        let line = source.bytes().filter(|b| *b == b'\n').count();
        let column = source.rsplit('\n').next().unwrap().encode_utf16().count();
        let expected = format!(
            r#"{{"id":10,"jsonrpc":"2.0","result":[{{"newText":{},"range":{{"end":{{"character":{column},"line":{line}}},"start":{{"character":0,"line":0}}}}}}]}}"#,
            quote(&canonical)
        );
        assert_eq!(response(&output, 10), expected);
    }
}
#[test]
fn formatting_is_read_only_until_client_applies_the_edit() {
    let source = "fn main():println(1)";
    let canonical = fern_compiler::format::format(source).unwrap();
    let (status, output) = run(&[
        initialize().into(),
        open(URI, source),
        request(10),
        request(11),
        change(2, &canonical),
        request(12),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(status.is_ok());
    assert_eq!(
        response(&output, 10).replace("\"id\":10", "\"id\":11"),
        response(&output, 11)
    );
    assert!(response(&output, 12).contains(r#""result":[]"#));
    assert_eq!(
        output
            .iter()
            .filter(|s| s.contains("publishDiagnostics"))
            .count(),
        2
    );
}
#[test]
fn latest_accepted_buffer_wins_and_stale_changes_cannot_restore_old_formatting() {
    let clean = fern_compiler::format::format("fn main():println(1)").unwrap();
    let (status, output) = run(&[
        initialize().into(),
        open(URI, "fn main():println(0)"),
        change(3, &clean),
        change(2, "fn main():println(2)"),
        request(10),
        change(4, "fn main(:"),
        request(11),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(status.is_ok());
    assert!(response(&output, 10).contains(r#""result":[]"#));
    assert!(
        response(&output, 11).contains(r#""code":-32803"#),
        "{}",
        response(&output, 11)
    );
    assert!(!response(&output, 11).contains("newText"));
}
#[test]
fn formatting_needs_syntax_but_no_types_imports_backend_or_disk_source() {
    let uri = "file:///absent/fern-format/main.fn";
    let source = "import absent\nfn main():unknown(1)";
    let expected = fern_compiler::format::format(source).unwrap();
    let (status, output) = run(&[
        initialize().into(),
        open(uri, source),
        request(10).replace(URI, uri),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(status.is_ok());
    assert!(
        response(&output, 10).contains(&format!("\"newText\":{}", quote(&expected))),
        "{}",
        response(&output, 10)
    );
}
#[test]
fn invalid_options_fail_without_changes_or_server_shutdown() {
    for options in [
        "null",
        "[]",
        "{}",
        r#"{"tabSize":0,"insertSpaces":true}"#,
        r#"{"tabSize":2147483648,"insertSpaces":true}"#,
        r#"{"tabSize":1.5,"insertSpaces":true}"#,
        r#"{"tabSize":4,"insertSpaces":"yes"}"#,
        r#"{"tabSize":4,"insertSpaces":true,"trimFinalNewlines":1}"#,
        r#"{"tabSize":4,"insertSpaces":true,"extra":[]}"#,
    ] {
        let (status, output) = run(&[
            initialize().into(),
            open(URI, "fn main():1"),
            formatting(10, options),
            request(11),
            shutdown().into(),
            exit().into(),
        ]);
        assert!(status.is_ok());
        assert!(
            response(&output, 10).contains(r#""code":-32602"#),
            "{}",
            response(&output, 10)
        );
        assert!(response(&output, 11).contains("newText"));
    }
}
#[test]
fn client_preferences_do_not_override_canonical_fern_style() {
    let source = "fn main():println(1)";
    let (status, output) = run(&[
        initialize().into(),
        open(URI, source),
        formatting(
            10,
            r#"{"tabSize":2,"insertSpaces":false,"trimTrailingWhitespace":false,"insertFinalNewline":false,"trimFinalNewlines":false,"custom":"literal","number":-1}"#,
        ),
        request(11),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(status.is_ok());
    assert_eq!(
        response(&output, 10).replace("\"id\":10", "\"id\":11"),
        response(&output, 11)
    );
    assert!(response(&output, 10).contains("newText"));
}
#[test]
fn request_obeys_lifecycle_and_requires_an_open_document() {
    let (status, output) = run(&[
        request(10),
        initialize().into(),
        request(11),
        shutdown().into(),
        request(12),
        exit().into(),
    ]);
    assert!(status.is_ok());
    for (id, code) in [(10, -32002), (11, -32602), (12, -32600)] {
        assert!(
            response(&output, id).contains(&format!("\"code\":{code}")),
            "{}",
            response(&output, id)
        );
    }
}

#[test]
fn executable_protocol_formats_without_native_backend() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let source = "fn main():println(\"🌿\")";
    let mut child = Command::new(env!("CARGO_BIN_EXE_fern"))
        .arg("lsp")
        .env("FERN_QBE", "/absent/backend")
        .env("FERN_RUNTIME_LIB", "/absent/runtime")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input: Vec<u8> = [
        initialize().into(),
        open(URI, source),
        request(10),
        shutdown().into(),
        exit().into(),
    ]
    .iter()
    .flat_map(|message: &String| frame(message))
    .collect();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(r#""documentFormattingProvider":true"#));
    assert!(text.contains(&format!(
        "\"newText\":{}",
        quote(&fern_compiler::format::format(source).unwrap())
    )));
}
