use morrow_compiler::lsp;

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
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"languageId":"morrow","version":1,"text":{}}}}}}}"#,
        quote(uri),
        quote(source)
    )
}

#[test]
fn initialize_and_shutdown_obey_lifecycle() {
    let (result, output) = run(&[initialize().into(), shutdown().into(), exit().into()]);
    assert!(result.is_ok());
    assert_eq!(output.len(), 2);
    assert!(output[0].contains("textDocumentSync"));
    assert!(output[0].contains("utf-16"));
    assert!(output[1].contains(r#""result":null"#));
    assert!(run(&[exit().into()]).0.is_err());
    let (_, output) = run(&[
        r#"{"jsonrpc":"2.0","id":"before","method":"unknown"}"#.into(),
        initialize().into(),
        shutdown().into(),
        r#"{"jsonrpc":"2.0","id":3,"method":"unknown"}"#.into(),
        exit().into(),
    ]);
    assert!(output[0].contains("-32002"));
    assert!(output[3].contains("-32600"));
}

#[test]
fn open_change_close_publish_and_clear_diagnostics() {
    let change = |version, text: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"file:///demo.mr","version":{version}}},"contentChanges":[{{"text":{}}}]}}}}"#,
            quote(text)
        )
    };
    let close = r#"{"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":"file:///demo.mr"}}}"#;
    let (result, output) = run(&[
        initialize().into(),
        open("file:///demo.mr", "fn main(): unknown"),
        change(2, "fn main(): println(42)"),
        change(1, "bad stale source"),
        close.into(),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok(), "{result:?}");
    let diagnostics: Vec<_> = output
        .iter()
        .filter(|message| message.contains("publishDiagnostics"))
        .collect();
    assert_eq!(diagnostics.len(), 3);
    assert!(diagnostics[0].contains("unknown"));
    assert!(diagnostics[1].contains(r#""diagnostics":[]"#));
    assert!(diagnostics[1].contains(r#""version":2"#));
    assert!(diagnostics[2].contains(r#""diagnostics":[]"#));
}

#[test]
fn diagnostic_positions_count_utf16_not_utf8_bytes() {
    let source = "fn main(): \"🌿\" == missing";
    let column = source[..source.find("missing").unwrap()]
        .encode_utf16()
        .count();
    let (result, output) = run(&[
        initialize().into(),
        open("file:///🌿.mr", source),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok());
    assert!(
        output[1].contains(&format!(r#""start":{{"character":{column},"line":0}}"#)),
        "{}",
        output[1]
    );
    assert!(output[1].contains(&format!(r#""end":{{"character":{},"line":0}}"#, column + 7)));
    assert!(output[1].contains("file:///🌿.mr"));
}

#[test]
fn incremental_changes_apply_sequential_utf16_ranges() {
    let source = "fn main(): \"🌿\" == missing";
    let column = source[..source.find("missing").unwrap()]
        .encode_utf16()
        .count();
    let change = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"file:///demo.mr","version":2}},"contentChanges":[{{"range":{{"start":{{"line":0,"character":{column}}},"end":{{"line":0,"character":{}}}}},"text":"\"green\""}}]}}}}"#,
        column + 7
    );
    let (result, output) = run(&[
        initialize().into(),
        open("file:///demo.mr", source),
        change,
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok());
    assert!(output[2].contains(r#""diagnostics":[]"#), "{}", output[2]);
}

#[test]
fn json_escapes_surrogate_pairs_and_nested_capabilities_work() {
    let init = r#"{"jsonrpc":"2.0","id":"\ud83c\udf3f","method":"initialize","params":{"capabilities":{"x":[true,false,null,-12,1.5e+2,{"escaped":"a\tb\r\n\b\f\/\u00e4"}]}}}"#;
    let (result, output) = run(&[init.into(), shutdown().into(), exit().into()]);
    assert!(result.is_ok());
    assert!(output[0].contains(r#""id":"🌿""#));
    for malformed in [
        r#"{"a":"\ud800"}"#,
        r#"{"a":"\udc00"}"#,
        r#"{"a":01}"#,
        r#"{"a":1e}"#,
        r#"{"a":1,"a":2}"#,
        r#"{"a":[1,]}"#,
    ] {
        let (result, output) = run(&[
            malformed.into(),
            initialize().into(),
            shutdown().into(),
            exit().into(),
        ]);
        assert!(result.is_ok());
        assert!(output[0].contains("-32700"), "{malformed}: {output:?}");
    }
}

#[test]
fn framing_failures_and_excessive_json_depth_are_bounded() {
    for input in [
        b"Content-Length: -1\r\n\r\n".as_slice(),
        b"Content-Length: 999999999\r\n\r\n",
        b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
        b"Content-Length: 5\r\n\r\n{}",
        b"X-Test: yes\r\n\r\n{}",
    ] {
        assert!(lsp::serve(std::io::Cursor::new(input), Vec::new()).is_err());
    }
    let huge_header = vec![b'A'; 9000];
    assert!(lsp::serve(std::io::Cursor::new(huge_header), Vec::new()).is_err());
    let nested = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    let (result, output) = run(&[
        nested,
        initialize().into(),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok());
    assert!(output[0].contains("-32700"));
}

#[test]
fn library_buffers_are_checked_without_requiring_a_user_main() {
    let (result, output) = run(&[
        initialize().into(),
        open("file:///library.mr", "pub fn answer() -> Int: 42\n"),
        open("file:///broken.mr", "pub fn answer() -> Int: unknown\n"),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok());
    assert!(output[1].contains(r#""diagnostics":[]"#));
    assert!(output[2].contains("unknown"));
    assert!(!output[2].contains("requires a main"));
}

#[test]
fn malformed_utf16_edits_leave_document_and_version_unchanged() {
    let source = "fn main(): \"🌿\" == missing";
    let emoji = source[..source.find('🌿').unwrap()].encode_utf16().count();
    let bad = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"file:///demo.mr","version":2}},"contentChanges":[{{"range":{{"start":{{"line":0,"character":{}}},"end":{{"line":0,"character":{}}}}},"text":""}}]}}}}"#,
        emoji + 1,
        emoji + 2
    );
    let good = r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///demo.mr","version":2},"contentChanges":[{"text":"fn main(): println(1)"}]}}"#;
    let (result, output) = run(&[
        initialize().into(),
        open("file:///demo.mr", source),
        bad,
        good.into(),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok());
    assert!(output[2].contains("surrogate pair"));
    assert!(output[3].contains(r#""diagnostics":[]"#));
}

#[test]
fn failed_transport_writes_stop_without_attempting_more_messages() {
    struct FailDiagnostics {
        failed: bool,
        writes_after_failure: usize,
    }
    impl std::io::Write for FailDiagnostics {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.failed {
                self.writes_after_failure += 1;
                return Err(std::io::Error::other("closed"));
            }
            if bytes
                .windows("publishDiagnostics".len())
                .any(|part| part == b"publishDiagnostics")
            {
                self.failed = true;
                return Err(std::io::Error::other("closed"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let input: Vec<u8> = [
        initialize().to_owned(),
        open("file:///demo.mr", "fn main(): 0"),
        shutdown().into(),
        exit().into(),
    ]
    .iter()
    .flat_map(|message| frame(message))
    .collect();
    let mut writer = FailDiagnostics {
        failed: false,
        writes_after_failure: 0,
    };
    assert!(lsp::serve(std::io::Cursor::new(input), &mut writer).is_err());
    assert!(writer.failed);
    assert_eq!(writer.writes_after_failure, 0);
}

#[test]
fn oversized_buffers_are_rejected_without_poisoning_open_state() {
    let huge = " ".repeat(1024 * 1024 + 1);
    let (result, output) = run(&[
        initialize().into(),
        open("file:///demo.mr", &huge),
        open("file:///demo.mr", "fn main(): 0"),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok());
    assert!(output[1].contains("source limit"));
    assert!(output[2].contains(r#""diagnostics":[]"#));
}

#[test]
fn malformed_later_edit_rolls_back_the_entire_batch() {
    let bad_batch = r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///demo.mr","version":2},"contentChanges":[{"text":"fn main(): 0"},{"range":{"start":{"line":9,"character":0},"end":{"line":9,"character":0}},"text":"bad"}]}}"#;
    let good_batch = r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///demo.mr","version":2},"contentChanges":[{"range":{"start":{"line":0,"character":11},"end":{"line":0,"character":18}},"rangeLength":7,"text":"0"}]}}"#;
    let (result, output) = run(&[
        initialize().into(),
        open("file:///demo.mr", "fn main(): missing"),
        bad_batch.into(),
        good_batch.into(),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(result.is_ok());
    assert!(output[2].contains("outside document"));
    assert!(output[3].contains(r#""diagnostics":[]"#), "{}", output[3]);
}

#[test]
fn all_json_prefixes_recover_without_panics_or_corrupt_framing() {
    let source = r#"{"jsonrpc":"2.0","id":"🌿\ud83c\udf3f","method":"unknown","params":[null,false,true,-42.5e-1,{"a":"escaped\n\u001f"}]}"#;
    let mut messages = vec![initialize().to_owned()];
    for (end, _) in source.char_indices().filter(|(end, _)| *end > 0) {
        messages.push(source[..end].to_owned());
    }
    messages.extend([shutdown().to_owned(), exit().to_owned()]);
    let (result, output) = run(&messages);
    assert!(result.is_ok());
    assert_eq!(output.len(), messages.len() - 1);
    assert!(
        output
            .iter()
            .skip(1)
            .take(output.len() - 2)
            .all(|message| message.contains("-32700"))
    );
}

struct Project(std::path::PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-lsp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn file(&self, name: &str, source: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, source).unwrap();
        format!("file://{}", path.display())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn replace(uri: &str, version: i64, source: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":{},"version":{version}}},"contentChanges":[{{"text":{}}}]}}}}"#,
        quote(uri),
        quote(source)
    )
}

#[test]
fn imported_open_buffers_override_disk_and_recover_diagnostics() {
    let project = Project::new();
    let main_source = "import math\nfn main(): println(math.value())\n";
    let good = "module math\npub fn value() -> Int: 7\n";
    let main = project.file("main.mr", main_source);
    let math = project.file("math.mr", good);
    let (_, output) = run(&[
        initialize().into(),
        open(&main, main_source),
        open(&math, good),
        replace(&math, 2, "module math\npub fn value() -> Int: true\n"),
        replace(&math, 3, good),
        shutdown().into(),
        exit().into(),
    ]);
    let publications: Vec<_> = output
        .iter()
        .filter(|m| m.contains("publishDiagnostics"))
        .collect();
    assert!(
        publications[0].contains(r#""diagnostics":[]"#),
        "{output:?}"
    );
    let errors: Vec<_> = publications
        .iter()
        .filter(|m| !m.contains(r#""diagnostics":[]"#))
        .collect();
    assert!(!errors.is_empty(), "{output:?}");
    assert!(errors.iter().all(|m| m.contains(&math)), "{output:?}");
    assert!(
        publications.last().unwrap().contains(r#""diagnostics":[]"#),
        "{output:?}"
    );
    assert!(publications.last().unwrap().contains(r#""version":3"#));
}

#[test]
fn imported_disk_errors_have_original_uri_utf16_ranges_and_private_access() {
    let project = Project::new();
    let main_source = "import math\nfn main(): println(math.value())\n";
    let main = project.file("main.mr", main_source);
    let math = project.file(
        "math.mr",
        "module math\npub fn value() -> Int: String.len(\"🌿\") + missing\n",
    );
    let (_, output) = run(&[
        initialize().into(),
        open(&main, main_source),
        shutdown().into(),
        exit().into(),
    ]);
    let error = output
        .iter()
        .find(|m| m.contains("missing"))
        .expect("imported error");
    assert!(error.contains(&math), "{output:?}");
    assert!(error.contains(r#""character":42,"line":1"#), "{output:?}");
    project.file("math.mr", "module math\nfn value() -> Int: 7\n");
    let (_, output) = run(&[
        initialize().into(),
        open(&main, main_source),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(
        output
            .iter()
            .any(|m| m.contains(&main) && m.contains("private")),
        "{output:?}"
    );
}

#[test]
fn closing_imported_buffer_reloads_disk_and_clears_its_error() {
    let project = Project::new();
    let main_source = "import math\nfn main(): println(math.value())\n";
    let main = project.file("main.mr", main_source);
    let math = project.file("math.mr", "module math\npub fn value() -> Int: 7\n");
    let close = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":{}}}}}}}"#,
        quote(&math)
    );
    let (_, output) = run(&[
        initialize().into(),
        open(&main, main_source),
        open(&math, "module math\npub fn value() -> Int: missing\n"),
        close,
        shutdown().into(),
        exit().into(),
    ]);
    assert!(
        output
            .iter()
            .any(|m| m.contains(&math) && m.contains("missing")),
        "{output:?}"
    );
    let last = output
        .iter()
        .rev()
        .find(|m| m.contains("publishDiagnostics"))
        .unwrap();
    assert!(
        last.contains(&math) && last.contains(r#""diagnostics":[]"#),
        "{output:?}"
    );
}

#[test]
fn new_percent_encoded_file_buffers_resolve_disk_imports_without_creating_files() {
    let project = Project::new();
    project.file("math.mr", "module math\npub fn value() -> Int: 7\n");
    let path = project.0.join("new file.mr");
    let uri = format!("file://{}", path.display()).replace(' ', "%20");
    let (_, output) = run(&[
        initialize().into(),
        open(&uri, "import math\nfn main(): println(math.value())\n"),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(output[1].contains(r#""diagnostics":[]"#), "{output:?}");
    assert!(output[1].contains(&uri));
    assert!(!path.exists());
}

#[test]
fn dependency_api_changes_invalidate_callers_and_imported_parse_errors_are_located() {
    let project = Project::new();
    let source = "import math\nfn main(): println(math.value(1))\n";
    let main = project.file("main.mr", source);
    let math = project.file("math.mr", "module math\npub fn value(x: Int) -> Int: x\n");
    let (_, output) = run(&[
        initialize().into(),
        open(&main, source),
        open(&math, "module math\npub fn value(x: String) -> String: x\n"),
        replace(&math, 2, "module math\npub fn value(:\n"),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(
        output
            .iter()
            .any(|m| m.contains(&main) && m.contains("Int") && m.contains("String")),
        "{output:?}"
    );
    assert!(
        output.iter().any(|m| m.contains(&math)
            && m.contains(r#""line":1"#)
            && !m.contains(r#""diagnostics":[]"#)),
        "{output:?}"
    );
}

#[test]
fn opening_new_imported_buffer_clears_missing_module_without_disk_mutation() {
    let project = Project::new();
    let source = "import math\nfn main(): println(math.value())\n";
    let main = project.file("main.mr", source);
    let path = project.0.join("math.mr");
    let math = format!("file://{}", path.display());
    let (_, output) = run(&[
        initialize().into(),
        open(&main, source),
        open(&math, "module math\npub fn value() -> Int: 7\n"),
        shutdown().into(),
        exit().into(),
    ]);
    let reports: Vec<_> = output
        .iter()
        .filter(|m| m.contains(&main) && m.contains("publishDiagnostics"))
        .collect();
    assert!(
        reports[0].contains("expected exactly one module file"),
        "{output:?}"
    );
    assert!(
        reports.last().unwrap().contains(r#""diagnostics":[]"#),
        "{output:?}"
    );
    assert!(!path.exists());
}

#[test]
fn file_uri_aliases_and_malformed_escapes_do_not_replace_open_buffers() {
    let project = Project::new();
    let uri = project.file("main.mr", "fn main(): ()\n");
    let alias = uri.replace("main.mr", "%6Dain.mr");
    let (_, output) = run(&[
        initialize().into(),
        open(&uri, "fn main(): ()\n"),
        open(&alias, "fn main(): missing\n"),
        open("file:///broken%XX.mr", "fn main(): ()\n"),
        replace(&uri, 2, "fn main(): ()\n"),
        shutdown().into(),
        exit().into(),
    ]);
    assert!(
        output
            .iter()
            .any(|m| m.contains("already open under another URI")),
        "{output:?}"
    );
    assert!(
        output.iter().any(|m| m.contains("invalid percent escape")),
        "{output:?}"
    );
    assert!(
        !output
            .iter()
            .any(|m| m.contains("publishDiagnostics") && m.contains("missing"))
    );
}
