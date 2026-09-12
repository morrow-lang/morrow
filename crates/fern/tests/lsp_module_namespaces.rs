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

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn query(project: &Project, marked: &str, method: &str) -> String {
    let offset = marked.find('§').unwrap();
    let before = &marked[..offset];
    let line = before.bytes().filter(|b| *b == b'\n').count();
    let character = before.rsplit('\n').next().unwrap().encode_utf16().count();
    let source = marked.replace('§', "");
    let path = project.write("main.fn", &source);
    let uri = format!("file://{}", path.display());
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"version":1,"text":{}}}}}}}"#,
            quote(&uri),
            quote(&source)
        ),
        format!(
            r#"{{"jsonrpc":"2.0","id":"query","method":"textDocument/{method}","params":{{"textDocument":{{"uri":{}}},"position":{{"line":{line},"character":{character}}}}}}}"#,
            quote(&uri)
        ),
        r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#.into(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.into(),
    ];
    run(messages)
        .into_iter()
        .find(|s| s.contains("\"id\":\"query\""))
        .unwrap()
}
#[test]
fn import_selector_returns_type_then_value_locations() {
    let p = Project::new();
    p.write(
        "ids.fn",
        "module ids\npub type Id=Int\npub fn Id(x:Int)->Int:x\n",
    );
    let result = query(
        &p,
        "import ids.{Id§}\nfn main():println(Id(1))\n",
        "definition",
    );
    assert!(result.contains("\"result\":["), "{result}");
    assert_eq!(result.matches("\"uri\":").count(), 2, "{result}");
    assert!(
        result.find("\"line\":1").unwrap() < result.find("\"line\":2").unwrap(),
        "{result}"
    );
}
#[test]
fn qualified_annotation_and_call_choose_namespace_and_own_documentation() {
    let p = Project::new();
    p.write("ids.fn","module ids\n@doc \"\"\"Type docs.\"\"\"\npub type Id=Int\n@doc \"\"\"Function docs.\"\"\"\npub fn Id(x:Int)->Int:x\n");
    let source = "import ids as m\nfn main():\n    let ids=3\n    let value:m.Id=m.Id(ids)\n    println(value)\n";
    let ty = query(&p, &source.replace(":m.Id", ":m.Id§"), "hover");
    assert!(
        ty.contains("Type docs.") && !ty.contains("Function docs."),
        "{ty}"
    );
    let value = query(&p, &source.replace("=m.Id(", "=m.Id§("), "hover");
    assert!(
        value.contains("Function docs.") && !value.contains("Type docs."),
        "{value}"
    );
}
#[test]
fn record_selector_deduplicates_owner_but_newtype_retains_constructor_anchor() {
    let p = Project::new();
    p.write(
        "ids.fn",
        "module ids\npub type Point:\n    x:Int\npub newtype Id=Id(Int)\n",
    );
    let result = query(&p, "import ids.{Point§}\nfn main():()\n", "definition");
    assert_eq!(result.matches("\"uri\":").count(), 1, "{result}");
    let result = query(&p, "import ids.{Id§}\nfn main():()\n", "definition");
    assert_eq!(result.matches("\"uri\":").count(), 2, "{result}");
}
#[test]
fn private_sibling_is_never_offered_in_wrong_namespace() {
    let p = Project::new();
    p.write(
        "ids.fn",
        "module ids\npub type Token=Int\nfn Token(x:Int)->Int:x\n",
    );
    let result = query(
        &p,
        "import ids\nfn main():println(ids.To§ken(1))\n",
        "completion",
    );
    assert!(!result.contains("\"label\":\"Token\""), "{result}");
}

#[test]
fn selector_visibility_tracks_current_overlay_and_close_without_stale_locations() {
    let p = Project::new();
    let dependency = p.write("ids.fn", "pub type Id=Int\nfn Id(x:Int)->Int:x\n");
    let source = "import ids.{Id}\nfn main():()\n";
    let main = p.write("main.fn", source);
    let main_uri = format!("file://{}", main.display());
    let dependency_uri = format!("file://{}", dependency.display());
    let open = |uri: &str, text: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"version":1,"text":{}}}}}}}"#,
            quote(uri),
            quote(text)
        )
    };
    let request = |id: i32| {
        format!(
            r#"{{"jsonrpc":"2.0","id":"q{id}","method":"textDocument/definition","params":{{"textDocument":{{"uri":{}}},"position":{{"line":0,"character":13}}}}}}"#,
            quote(&main_uri)
        )
    };
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into(),
        open(&main_uri, source),
        request(1),
        open(
            &dependency_uri,
            "pub type Id=Int\npub fn Id(x:Int)->Int:x\n",
        ),
        request(2),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":{}}}}}}}"#,
            quote(&dependency_uri)
        ),
        request(3),
        r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#.into(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.into(),
    ];
    let responses = run(messages);
    for (id, expected) in [(1, 1), (2, 2), (3, 1)] {
        let response = responses
            .iter()
            .find(|text| text.contains(&format!("\"id\":\"q{id}\"")))
            .unwrap();
        assert_eq!(response.matches("\"uri\":").count(), expected, "{response}");
    }
}

#[test]
fn unicode_import_selectors_use_exact_utf16_ranges_and_real_delimiters() {
    let p = Project::new();
    p.write(
        "ids.fn",
        "pub type Δείκτης=Int\npub fn Δείκτης(x:Int)->Int:x\n",
    );
    let result = query(
        &p,
        "import /* { fake } */ ids.{Δείκτης§}\nfn main():()\n",
        "definition",
    );
    assert_eq!(result.matches("\"uri\":").count(), 2, "{result}");
    assert!(result.contains("\"character\":9"), "{result}");
    assert!(result.contains("\"character\":7"), "{result}");
}

#[test]
fn type_completion_ignores_value_shadowing_and_selectors_offer_type_aliases() {
    let p = Project::new();
    p.write("ids.fn", "pub type Token=Int\npub fn value()->Int:1\n");
    let result = query(
        &p,
        "import ids\nfn main():\n    let ids=3\n    let value:ids.To§ken=1\n    println(value+ids)\n",
        "completion",
    );
    assert!(result.contains("\"label\":\"Token\""), "{result}");
    assert!(result.contains("\"kind\":7"), "{result}");
    let result = query(&p, "import ids.{To§ken}\nfn main():()\n", "completion");
    assert!(result.contains("\"label\":\"Token\""), "{result}");
}
